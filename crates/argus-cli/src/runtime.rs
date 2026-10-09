//! The game runtimes an install carries (distribution item 1), so that
//! releasing a game needs no Rust: `argus build` puts the game's packs
//! beside a runtime built once with the engine.
//!
//! An install's `runtime/` sits beside its `bin/` (or is `$ARGUS_RUNTIME`):
//!
//! - `<windows|linux>/standard/`: `argus-game` with every module but Steam,
//!   for any game that does not name Steam (a module only runs for a game
//!   whose `argus.toml` lists it);
//! - `<windows|linux>/steam/`: the same with Steam, and Steam's library;
//! - `web/`: the wasm module and its glue in `pkg/`, the page, its script.
//!
//! Each holds `runtime.json`: the version and revision it was built from,
//! its target, variant, and modules. `tools/release/stage.sh` makes them.
//! A checkout has no `runtime/`, and `argus build` compiles from source.

use serde_json::{json, Value as Json};
use std::path::{Path, PathBuf};

/// What each runtime directory says about itself.
pub const META: &str = "runtime.json";

/// The modules every desktop runtime has; the Steam variant adds Steam.
pub const STANDARD: &[&str] = &["rigid"];

/// A runtime found for a build.
#[derive(Debug)]
pub struct Runtime {
    /// Its directory.
    pub dir: PathBuf,
    /// Its `runtime.json`.
    pub meta: Json,
}

impl Runtime {
    /// What a build's report says about it.
    pub fn report(&self) -> Json {
        json!({
            "prebuilt": true,
            "dir": crate::build::plain(&self.dir),
            "version": self.meta["version"],
            "revision": self.meta["revision"],
            "variant": self.meta["variant"],
        })
    }
}

/// The install's runtime directory, when there is one: `$ARGUS_RUNTIME`,
/// else `runtime/` beside the directory this program is in.
pub fn root() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("ARGUS_RUNTIME") {
        if !d.is_empty() {
            return Some(PathBuf::from(d));
        }
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.parent()?.join("runtime");
    dir.is_dir().then_some(dir)
}

/// The desktop variant a game needs: `steam` when it names Steam, else
/// `standard`.
pub fn variant(modules: &[String]) -> &'static str {
    if modules.iter().any(|m| m == "steam") {
        "steam"
    } else {
        "standard"
    }
}

/// The runtime for `target` (`windows`, `linux`, `web`) and the game's
/// modules, from the install's runtimes. None when there are no runtimes
/// (a checkout); an error when there are, but not one this build can use.
pub fn find(target: &str, modules: &[String]) -> Result<Option<Runtime>, String> {
    let Some(root) = root() else {
        return Ok(None);
    };
    let dir = if target == "web" {
        root.join("web")
    } else {
        root.join(target).join(variant(modules))
    };
    let meta_file = dir.join(META);
    let text = std::fs::read_to_string(&meta_file).map_err(|_| {
        let have = installed(&root);
        format!(
            "the install has no {} runtime in {} (it has: {}); reinstall Argus with that platform, or build from the engine's source with --from-source",
            if target == "web" { "web".to_string() } else { format!("{target} {}", variant(modules)) },
            crate::build::plain(&root),
            if have.is_empty() { "none".to_string() } else { have.join(", ") }
        )
    })?;
    let meta: Json = serde_json::from_str(&text)
        .map_err(|e| format!("{}: {e}", crate::build::plain(&meta_file)))?;
    same_engine(&meta, &meta_file)?;
    let has: Vec<&str> = meta["modules"]
        .as_array()
        .map(|a| a.iter().filter_map(Json::as_str).collect())
        .unwrap_or_default();
    // A page has no Steam: it runs on the null platform (phase 5 item 9).
    let want = modules
        .iter()
        .filter(|m| !(target == "web" && *m == "steam"));
    if let Some(m) = want.into_iter().find(|m| !has.contains(&m.as_str())) {
        return Err(format!(
            "the game uses the {m} module, and the runtime in {} was built without it (it has: {})",
            crate::build::plain(&dir),
            has.join(", ")
        ));
    }
    Ok(Some(Runtime { dir, meta }))
}

/// The runtimes under `root`, as `windows standard`, `web`.
pub fn installed(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if root.join("web").join(META).is_file() {
        out.push("web".to_string());
    }
    for t in ["windows", "linux", "macos"] {
        for v in ["standard", "steam"] {
            if root.join(t).join(v).join(META).is_file() {
                out.push(format!("{t} {v}"));
            }
        }
    }
    out
}

/// A runtime must come from the engine this CLI is: packs and plugin
/// libraries change between versions. A revision of `dev` (a build from a
/// checkout) matches any revision of its version.
fn same_engine(meta: &Json, file: &Path) -> Result<(), String> {
    let (v, r) = (
        argus_daemon::daemon::VERSION,
        argus_daemon::daemon::REVISION,
    );
    let rv = meta["version"].as_str().unwrap_or("?");
    let rr = meta["revision"].as_str().unwrap_or("?");
    if rv != v || (rr != r && rr != "dev" && r != "dev") {
        return Err(format!(
            "the runtime in {} is Argus {rv} ({rr}), and this argus is {v} ({r}): an install's runtimes come with it, so reinstall, or set ARGUS_RUNTIME to runtimes of this version",
            crate::build::plain(file.parent().unwrap_or(file))
        ));
    }
    Ok(())
}

/// Put a desktop runtime in `out` as the game's executable (named `game`),
/// with whatever it ships beside it (Steam's library). Returns the
/// executable.
pub fn place_desktop(
    rt: &Runtime,
    out: &Path,
    game: &str,
    target: &str,
) -> Result<PathBuf, String> {
    let ext = if target == "windows" { ".exe" } else { "" };
    let exe = out.join(format!("{game}{ext}"));
    for e in std::fs::read_dir(&rt.dir).map_err(|e| format!("{}: {e}", rt.dir.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        let name = e.file_name().to_string_lossy().into_owned();
        if name == META || !e.path().is_file() {
            continue;
        }
        let to = if name == format!("argus-game{ext}") {
            exe.clone()
        } else {
            out.join(&name)
        };
        std::fs::copy(e.path(), &to)
            .map_err(|err| format!("{} to {}: {err}", e.path().display(), to.display()))?;
    }
    if !exe.is_file() {
        return Err(format!(
            "the runtime in {} has no argus-game{ext}",
            crate::build::plain(&rt.dir)
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755));
    }
    Ok(exe)
}

/// Put the web runtime in `out`: `pkg/` (the module and its glue), the
/// page, and its script.
pub fn place_web(rt: &Runtime, out: &Path) -> Result<(), String> {
    let pkg = out.join("pkg");
    let _ = std::fs::remove_dir_all(&pkg);
    std::fs::create_dir_all(&pkg).map_err(|e| format!("{}: {e}", pkg.display()))?;
    let from = rt.dir.join("pkg");
    for e in std::fs::read_dir(&from).map_err(|e| format!("{}: {e}", from.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        if e.path().is_file() {
            std::fs::copy(e.path(), pkg.join(e.file_name()))
                .map_err(|err| format!("{}: {err}", e.path().display()))?;
        }
    }
    for f in ["index.html", "main.js"] {
        std::fs::copy(rt.dir.join(f), out.join(f))
            .map_err(|e| format!("{}: {e}", rt.dir.join(f).display()))?;
    }
    Ok(())
}
