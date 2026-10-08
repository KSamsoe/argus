//! Loading a plugin's library. Cargo's output is copied first, to
//! `.argus/plugins/loaded/`, and the copy is what loads: Windows keeps a
//! loaded library's file locked, and the next build must be able to write
//! over cargo's. The copy goes when the library is unloaded, which is when
//! the last system running its code is replaced.
//!
//! One build loaded in a process is shared: a fork, or a test's second
//! world, runs the same library (a plugin holds no state, §7).

use crate::{codes, PluginError};
use argus_plugin::abi;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::SystemTime;

/// One system as the plugin declares it, copied out of the library.
#[derive(Clone, Debug)]
pub struct SystemSpec {
    /// Name.
    pub name: String,
    /// Stage name.
    pub stage: String,
    /// Doc.
    pub doc: String,
    /// Project-relative `file:line` of the declaration.
    pub source: String,
    /// Systems it runs after.
    pub after: Vec<String>,
    /// Components read.
    pub reads: Vec<String>,
    /// Components written.
    pub writes: Vec<String>,
    /// Components spawned, inserted, or removed.
    pub spawns: Vec<String>,
    /// Its random stream.
    pub rng: Option<String>,
    /// [`abi::flags`].
    pub flags: u32,
    /// An [`abi::clock`] binding.
    pub clock: u32,
    pub(crate) run: abi::RunFn,
    pub(crate) user: usize,
}

/// A component layout the plugin reads columns as.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutSpec {
    /// Component name.
    pub name: String,
    /// Row size.
    pub size: usize,
    /// Row alignment.
    pub align: usize,
    /// `(name, type, offset)`.
    pub fields: Vec<(String, u32, usize)>,
    /// Project-relative `file:line`.
    pub source: String,
}

/// A loaded plugin library.
pub struct Plugin {
    /// The crate's name, as `argus.toml` lists it.
    pub name: String,
    /// Its systems.
    pub systems: Vec<SystemSpec>,
    /// The layouts it reads columns as.
    pub layouts: Vec<LayoutSpec>,
    /// Cargo's library this was loaded from.
    pub built: PathBuf,
    stamp: Stamp,
    /// The copy loaded, which goes when the library does; none for a
    /// release build's library, loaded where it is.
    copy: Option<PathBuf>,
    #[cfg(not(target_arch = "wasm32"))]
    lib: Option<libloading::Library>,
}

impl std::fmt::Debug for Plugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plugin")
            .field("name", &self.name)
            .field("systems", &self.systems.len())
            .field("built", &self.built)
            .finish()
    }
}

impl Plugin {
    /// Whether `dll` is the build this was loaded from, unchanged.
    pub fn is_build(&self, dll: &Path) -> bool {
        self.built == dll && Stamp::of(dll).is_some_and(|s| s == self.stamp)
    }

    /// The file the process loaded.
    pub fn loaded_from(&self) -> &Path {
        self.copy.as_deref().unwrap_or(&self.built)
    }
}

impl Drop for Plugin {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        drop(self.lib.take());
        if let Some(c) = &self.copy {
            let _ = std::fs::remove_file(c);
        }
    }
}

/// A file's size and modification time: a new build is a new stamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: SystemTime,
}

impl Stamp {
    fn of(p: &Path) -> Option<Stamp> {
        let m = std::fs::metadata(p).ok()?;
        Some(Stamp {
            len: m.len(),
            modified: m.modified().ok()?,
        })
    }
}

static LOADED: Mutex<Vec<Weak<Plugin>>> = Mutex::new(Vec::new());
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Load `dll`, the build of the plugin `name` of the project at `root`:
/// a copy of it when `copy` (cargo will build over it), else the file
/// itself (a release build's, which nothing rebuilds).
pub fn load(root: &Path, name: &str, dll: &Path, copy: bool) -> Result<Arc<Plugin>, PluginError> {
    let err = |message: String| PluginError {
        plugin: name.to_string(),
        phase: "load",
        file: format!("plugins/{name}"),
        line: None,
        message,
        code: codes::LOAD,
    };
    if cfg!(target_arch = "wasm32") {
        return Err(PluginError {
            code: codes::NOT_HERE,
            ..err("plugins are loaded on the desktop; the page cannot load a library".into())
        });
    }
    let stamp = Stamp::of(dll).ok_or_else(|| {
        err(format!(
            "{} is not there: the plugin is not built",
            dll.display()
        ))
    })?;
    let mut loaded = LOADED.lock().unwrap_or_else(|e| e.into_inner());
    loaded.retain(|w| w.strong_count() > 0);
    if let Some(p) = loaded
        .iter()
        .filter_map(Weak::upgrade)
        .find(|p| p.built == dll && p.stamp == stamp)
    {
        return Ok(p);
    }
    if !copy {
        let p = Arc::new(open(name, dll, stamp, None).map_err(err)?);
        loaded.push(Arc::downgrade(&p));
        return Ok(p);
    }
    let dir = root.join(".argus").join("plugins").join("loaded");
    std::fs::create_dir_all(&dir)
        .map_err(|e| err(format!("cannot make {}: {e}", dir.display())))?;
    // Copies left by a process that ended without unloading.
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for f in rd.flatten() {
            let n = f.file_name().to_string_lossy().to_string();
            let mine = loaded
                .iter()
                .filter_map(Weak::upgrade)
                .any(|p| p.copy.as_deref() == Some(f.path().as_path()));
            if n.starts_with(&format!("{name}-")) && !mine {
                let _ = std::fs::remove_file(f.path());
            }
        }
    }
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let copy = dir.join(format!(
        "{name}-{}-{n}.{}",
        std::process::id(),
        std::env::consts::DLL_EXTENSION
    ));
    std::fs::copy(dll, &copy).map_err(|e| {
        err(format!(
            "cannot copy {} to {}: {e}",
            dll.display(),
            copy.display()
        ))
    })?;
    let p = open(name, dll, stamp, Some(copy)).map_err(err)?;
    let p = Arc::new(p);
    loaded.push(Arc::downgrade(&p));
    Ok(p)
}

#[cfg(target_arch = "wasm32")]
fn open(_: &str, _: &Path, _: Stamp, _: Option<PathBuf>) -> Result<Plugin, String> {
    Err("plugins are loaded on the desktop".into())
}

#[cfg(not(target_arch = "wasm32"))]
fn open(name: &str, dll: &Path, stamp: Stamp, copy: Option<PathBuf>) -> Result<Plugin, String> {
    // SAFETY: loading runs the library's initialisers; a plugin is the
    // project's own code, built from its source by its own cargo.
    let lib = match unsafe { libloading::Library::new(copy.as_deref().unwrap_or(dll)) } {
        Ok(l) => l,
        Err(e) => {
            if let Some(c) = &copy {
                let _ = std::fs::remove_file(c);
            }
            return Err(format!("{} does not load: {e}", dll.display()));
        }
    };
    let mut p = Plugin {
        name: name.to_string(),
        systems: Vec::new(),
        layouts: Vec::new(),
        built: dll.to_path_buf(),
        stamp,
        copy,
        lib: None,
    };
    // SAFETY: the symbol is the plugin! macro's entry, of this type.
    let entry: Result<libloading::Symbol<abi::EntryFn>, _> =
        unsafe { lib.get(abi::ENTRY.as_bytes()) };
    let entry = match entry {
        Ok(e) => *e,
        Err(_) => {
            p.lib = Some(lib);
            return Err(format!(
                "{} exports no {}: is it declared with argus_plugin::plugin!?",
                dll.display(),
                abi::ENTRY
            ));
        }
    };
    // SAFETY: the entry builds its declaration once and returns it; it
    // lives as long as the library.
    let decl = unsafe { entry() };
    if decl.is_null() {
        p.lib = Some(lib);
        return Err(format!(
            "{}'s {} returned nothing",
            dll.display(),
            abi::ENTRY
        ));
    }
    // SAFETY: non-null, the library's own, alive while `lib` is.
    let d = unsafe { &*decl };
    if d.abi != abi::ABI_VERSION {
        p.lib = Some(lib);
        return Err(format!(
            "{name} was built against plugin ABI {}, and this engine speaks {}: build it again against this engine's argus-plugin",
            d.abi,
            abi::ABI_VERSION
        ));
    }
    let base = format!("plugins/{name}");
    let at = |s: abi::Str| {
        // SAFETY: the declaration's text lives as long as the library.
        let s = unsafe { s.as_str() }.replace('\\', "/");
        if Path::new(&s).is_absolute() || s.starts_with('/') {
            s
        } else {
            format!("{base}/{s}")
        }
    };
    let strs = |l: abi::Strs| -> Vec<String> {
        // SAFETY: as above.
        unsafe { l.as_slice() }
            .iter()
            .map(|s| unsafe { s.as_str() }.to_string())
            .collect()
    };
    // SAFETY: as above, for every list the declaration points at.
    let systems = unsafe { std::slice::from_raw_parts(d.systems, d.systems_len) };
    for s in systems {
        // SAFETY: as above.
        let text = |x: abi::Str| unsafe { x.as_str() }.to_string();
        let rng = text(s.rng);
        p.systems.push(SystemSpec {
            name: text(s.name),
            stage: text(s.stage),
            doc: text(s.doc),
            source: at(s.source),
            after: strs(s.after),
            reads: strs(s.reads),
            writes: strs(s.writes),
            spawns: strs(s.spawns),
            rng: (!rng.is_empty()).then_some(rng),
            flags: s.flags,
            clock: s.clock,
            run: s.run,
            user: s.user as usize,
        });
    }
    // SAFETY: as above.
    let layouts = unsafe { std::slice::from_raw_parts(d.components, d.components_len) };
    for l in layouts {
        // SAFETY: as above.
        let fields = unsafe { std::slice::from_raw_parts(l.fields, l.fields_len) }
            .iter()
            .map(|f| (unsafe { f.name.as_str() }.to_string(), f.ty, f.offset))
            .collect();
        p.layouts.push(LayoutSpec {
            // SAFETY: as above.
            name: unsafe { l.name.as_str() }.to_string(),
            size: l.size,
            align: l.align,
            fields,
            source: at(l.source),
        });
    }
    p.lib = Some(lib);
    Ok(p)
}
