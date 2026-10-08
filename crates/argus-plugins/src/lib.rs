//! A project's Rust plugins (phase 5 item 6): built with cargo, loaded as
//! dynamic libraries, checked against the game, and run in the scheduler
//! beside the Luau systems (§7).
//!
//! A plugin is a crate in `plugins/<name>/` that `argus.toml` names in
//! `plugins`. [`load_all`] builds each one ([`build`]: cargo's own
//! messages, so a compile error is a `file:line` and the compiler's text),
//! loads a copy of the library (so cargo can build the next one while this
//! one runs), checks its component layouts and system declarations against
//! the world, and installs its systems. [`reload`] builds again and, when
//! the library changed, snapshots, swaps the old library for the new one,
//! restores, and continues (§7). The new library is loaded and checked
//! before the old one goes, so a bad build keeps the old one running.
//!
//! The boundary is [`argus_plugin::abi`]: plain C structs and functions.
//! Every call a plugin makes comes through [`host`], which checks it
//! against the system's declaration as the engine's own contexts do. A
//! plugin's events and its spawns and despawns land when its run ends, so
//! a plugin system behaves the same alone and in a parallel batch.

#![warn(missing_docs)]

mod build;
mod host;
mod install;
mod load;
pub mod sdk;

pub use build::{build, find_cargo, library_path, Build, Diagnostic};
pub use install::{check, install, remove};
pub use load::{load, LayoutSpec, Plugin, SystemSpec};

use argus_ecs::{ArgusError, World};
use argus_sim::Scheduler;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Error codes, the 61xx block.
pub mod codes {
    /// A plugin did not compile.
    pub const BUILD: u32 = 6101;
    /// A plugin's library did not load: not built, not a plugin, another ABI.
    pub const LOAD: u32 = 6102;
    /// A plugin reads a component laid out otherwise than the game's.
    pub const LAYOUT: u32 = 6103;
    /// A plugin's system declaration is wrong: an unknown stage or
    /// component, or a name another system has.
    pub const DECLARATION: u32 = 6104;
    /// Plugins need the desktop: the web page cannot load a library.
    pub const NOT_HERE: u32 = 6105;
    /// No cargo was found to build a plugin with.
    pub const NO_CARGO: u32 = 6106;
    /// A plugin system returned an error or panicked; it is switched off.
    pub const FAULT: u32 = 6107;
}

/// What went wrong with a plugin, as a `script.errors` row has it.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginError {
    /// The plugin.
    pub plugin: String,
    /// `compile`, `load`, or `install`.
    pub phase: &'static str,
    /// The project-relative file: `plugins/swarm-hot/src/lib.rs`.
    pub file: String,
    /// Its line, when there is one.
    pub line: Option<u32>,
    /// The message (the compiler's, for a compile error).
    pub message: String,
    /// A 61xx code.
    pub code: u32,
}

impl PluginError {
    /// `file:line`, or the file.
    pub fn at(&self) -> String {
        match self.line {
            Some(l) => format!("{}:{l}", self.file),
            None => self.file.clone(),
        }
    }

    /// As an engine error.
    pub fn to_error(&self) -> ArgusError {
        ArgusError::new(self.code, self.message.clone()).at(Some(self.at()))
    }
}

/// Where plugin libraries come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// Built from `plugins/<name>/` with cargo (the daemon, tests).
    Build {
        /// Optimised; a hot path is why a plugin exists.
        release: bool,
    },
    /// Already built, as `<dir>/<name>.<dll extension>` (a release build).
    Prebuilt(PathBuf),
}

impl Default for Source {
    fn default() -> Self {
        Source::Build { release: true }
    }
}

/// A world's plugins.
#[derive(Clone, Debug, Default)]
pub struct Plugins {
    /// Loaded and installed, in `argus.toml`'s order.
    pub loaded: Vec<Arc<Plugin>>,
    /// What went wrong, plugin by plugin.
    pub errors: Vec<PluginError>,
    /// Each build's report, when built.
    pub builds: Vec<Build>,
}

impl Plugins {
    /// The plugin a system comes from.
    pub fn of_system(&self, system: &str) -> Option<&str> {
        self.loaded
            .iter()
            .find(|p| p.systems.iter().any(|s| s.name == system))
            .map(|p| p.name.as_str())
    }
}

/// Build (or find), load, check, and install every plugin `names` lists.
/// A plugin that fails is reported and left out; the rest still load.
pub fn load_all(
    root: &Path,
    names: &[String],
    source: &Source,
    world: &mut World,
    sched: &mut Scheduler,
) -> Plugins {
    let mut out = Plugins::default();
    for name in names {
        let dll = match source {
            Source::Build { release } => {
                let b = build(root, name, *release);
                let errs = b.errors(name);
                // A build that fails keeps the last good library, if there
                // is one, as a reload does: the game runs as it last built.
                let dll = b.dll.clone().or_else(|| {
                    let last = library_path(root, name, *release);
                    last.is_file().then_some(last)
                });
                out.errors.extend(errs);
                out.builds.push(b);
                match dll {
                    Some(d) => d,
                    None => {
                        sched.tolerate_missing(true);
                        continue;
                    }
                }
            }
            Source::Prebuilt(dir) => {
                dir.join(format!("{name}.{}", std::env::consts::DLL_EXTENSION))
            }
        };
        let copy = matches!(source, Source::Build { .. });
        let p = match load(root, name, &dll, copy) {
            Ok(p) => p,
            Err(e) => {
                out.errors.push(e);
                sched.tolerate_missing(true);
                continue;
            }
        };
        let errs = install(world, sched, &p, None);
        if !errs.is_empty() {
            sched.tolerate_missing(true);
        }
        out.errors.extend(errs);
        out.loaded.push(p);
    }
    out
}

/// What [`reload`] did for one plugin.
#[derive(Clone, Debug, PartialEq)]
pub struct Reloaded {
    /// The plugin.
    pub plugin: String,
    /// Whether a new library went in.
    pub swapped: bool,
    /// The build's milliseconds (cargo's whole run).
    pub build_ms: u64,
    /// From the build's end to the world running on the new library:
    /// load, check, snapshot, swap, restore (§17: "Rust plugin reload <
    /// 2 s after compile").
    pub reload_ms: f64,
}

/// Build every plugin again and swap in each one whose library changed
/// (§7: "the daemon snapshots, unloads the old dylib, loads the new one,
/// restores, and continues"). A plugin that does not build, load, or
/// check keeps its old library and reports why. Errors replace the old.
pub fn reload(
    root: &Path,
    names: &[String],
    plugins: &mut Plugins,
    world: &mut World,
    sched: &mut Scheduler,
) -> Vec<Reloaded> {
    let mut report = Vec::new();
    plugins.errors.clear();
    plugins.builds.clear();
    for name in names {
        let b = build(root, name, true);
        let build_ms = b.ms;
        let errs = b.errors(name);
        let dll = b.dll.clone();
        plugins.builds.push(b);
        let old = plugins.loaded.iter().position(|p| &p.name == name);
        let mut done = Reloaded {
            plugin: name.clone(),
            swapped: false,
            build_ms,
            reload_ms: 0.0,
        };
        let Some(dll) = dll.filter(|_| errs.is_empty()) else {
            plugins.errors.extend(errs);
            report.push(done);
            continue;
        };
        if let Some(i) = old {
            if plugins.loaded[i].is_build(&dll) {
                // The same library: still checked, since the scripts just
                // reloaded may have changed a component it reads.
                let p = &plugins.loaded[i];
                plugins.errors.extend(check(world, sched, p, Some(p)));
                report.push(done);
                continue;
            }
        }
        let t = std::time::Instant::now();
        let new = match load(root, name, &dll, true) {
            Ok(p) => p,
            Err(e) => {
                plugins.errors.push(e);
                report.push(done);
                continue;
            }
        };
        let problems = check(world, sched, &new, old.map(|i| &*plugins.loaded[i]));
        if !problems.is_empty() {
            plugins.errors.extend(problems);
            report.push(done);
            continue;
        }
        let snap = world.snapshot();
        let state = sched.state();
        let prev = old.map(|i| plugins.loaded.remove(i));
        let errs = install(world, sched, &new, prev.as_deref());
        // The old library goes when the last system holding it does: the
        // swap above replaced each one.
        drop(prev);
        world.restore(&snap);
        sched.restore_state(&state);
        plugins.errors.extend(errs);
        let at = old
            .unwrap_or(plugins.loaded.len())
            .min(plugins.loaded.len());
        plugins.loaded.insert(at, new);
        done.swapped = true;
        done.reload_ms = t.elapsed().as_secs_f64() * 1e3;
        report.push(done);
    }
    report
}
