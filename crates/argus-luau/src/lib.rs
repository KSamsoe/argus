//! Argus Luau binding — phase 0, piece 3 of the Argus Engine build plan.
//!
//! Game logic is Luau (§7). This crate hosts a sandboxed Luau VM inside the
//! daemon, lets scripts declare components, def schemas, and systems, binds
//! those systems into the scheduler with the same declared-access contract
//! Rust systems have, enforces an instruction budget and a memory limit,
//! reloads changed files without losing world state, and reports every
//! compile or declaration error as structured `file:line` data instead of a
//! crash.
//!
//! `SPEC-TRACE.md` next to this crate maps the spec to code and tests.

#![deny(missing_docs)]
#![warn(clippy::all)]

pub mod api;
mod bind;
mod decl;
mod migrate;

pub use bind::script_codes as codes;
pub use decl::{ComponentDecl, DefDecl, MigrateDecl, Step, SystemDecl};

use argus_ecs::{layers, Access, ArgusError, Layers, Result, World};
use argus_sim::{ClockBinding, Scheduler, Stage, SystemDesc, SystemFn};
use bind::{Link, QueryHandle};
use decl::Pending;
use mlua::{Function, Lua, VmState};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Limits applied to every script (§2 "sandboxed inside the daemon with
/// instruction budgets").
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptConfig {
    /// VM interrupts (roughly: calls plus loop iterations) one system may
    /// use per run before it is stopped and disabled.
    pub interrupt_budget: u64,
    /// Bytes the Luau heap may hold.
    pub memory_limit: usize,
    /// Sim seconds in one of the game's days (`[time] day` in argus.toml),
    /// which `q.day`, `q.day_time`, and `q.day_length` count in.
    pub day_seconds: f64,
}

impl Default for ScriptConfig {
    fn default() -> Self {
        ScriptConfig {
            interrupt_budget: 5_000_000,
            memory_limit: 256 << 20,
            day_seconds: 86_400.0,
        }
    }
}

/// The game's day length, where the query handle reads it.
pub(crate) struct DayLength(pub f64);

/// Path queries a script can make (phase 3 item 2, §12: "Delve's monsters
/// and any game with NPCs use the same path queries"): the navigation the
/// player's assembly has, handed to the host ([`ScriptHost::set_paths`]),
/// so this crate needs no part of it.
pub trait Paths {
    /// The waypoints from one point to another (each cell's centre after
    /// the start, the goal itself last) on `grid`, or the grid `from` is
    /// on, and what the way costs in steps; none when there is no way.
    fn path(
        &self,
        world: &argus_ecs::World,
        from: [f32; 2],
        to: [f32; 2],
        grid: argus_ecs::Entity,
    ) -> Result<Option<(Vec<[f32; 2]>, f32)>>;
    /// Whether a point is on an open cell of `grid` (or the grid it is on).
    fn walkable(
        &self,
        world: &argus_ecs::World,
        at: [f32; 2],
        grid: argus_ecs::Entity,
    ) -> Result<bool>;
    /// Steps from a point to a `NavField` entity's cell; none where the
    /// field does not reach.
    fn distance(
        &self,
        world: &argus_ecs::World,
        field: argus_ecs::Entity,
        at: [f32; 2],
    ) -> Result<Option<f32>>;
    /// The centre of the next cell down a `NavField` from a point; none at
    /// the field's cell or where it does not reach.
    fn downhill(
        &self,
        world: &argus_ecs::World,
        field: argus_ecs::Entity,
        at: [f32; 2],
    ) -> Result<Option<[f32; 2]>>;
}

/// The paths a host was given, where the query handle reads them.
pub(crate) struct PathsData(pub Rc<dyn Paths>);

/// A project's data tables as scripts read them (§4 "Data tables", phase 3
/// item 3: "Outpost is mostly defs"): `q:def("item:bread")` and
/// `q:defs("recipes")`. The player's assembly hands the host its project's
/// tables ([`ScriptHost::set_defs`]), so this crate reads no files for it.
pub trait DefRows {
    /// One row by reference, `prefix:key`: its fields by name, in schema
    /// order; none when no table has that prefix or no row that key.
    fn row(&self, r: &str) -> Option<Vec<(String, argus_ecs::Value)>>;
    /// A table's reference prefix and its rows in file order, each its key
    /// and fields; none when there is no such table.
    #[allow(clippy::type_complexity)]
    fn table(&self, name: &str)
        -> Option<(String, Vec<(String, Vec<(String, argus_ecs::Value)>)>)>;
}

/// The game's player-facing strings as scripts read them (§12, phase 3 item
/// 4: "all player-facing strings go through a string table keyed by id"):
/// `q:text("hud.day", {day = 3})` and `q:languages()`. The player's
/// assembly hands the host its string tables ([`ScriptHost::set_texts`]);
/// the language is the world's (`UiSettings.language`).
pub trait Texts {
    /// A key's string in the world's language (falling back to the base
    /// language's), or `None` where no table has it.
    fn text(&self, world: &argus_ecs::World, key: &str) -> Option<String>;
    /// The languages there are tables for, the base first.
    fn languages(&self) -> Vec<String>;
}

/// The string tables a host was given.
pub(crate) struct TextsData(pub Rc<dyn Texts>);

/// A game's maps as scripts read and write them (phase 5 item 4):
/// `q:tiles(map)`, `map` a `Tilemap` entity. A runtime map's cells are the
/// run's and a script writes them (Delve's floors from the seed); any
/// map's a script may read. The player's assembly hands the host the
/// session's map store ([`ScriptHost::set_tiles`]). Cells are by column
/// and row from the map's top-left, 0 first; tile 0 is empty.
pub trait Tiles {
    /// The width and height in cells, and the layers' names, of the map a
    /// `Tilemap` entity places.
    fn size(
        &self,
        world: &argus_ecs::World,
        map: argus_ecs::Entity,
    ) -> Result<(u32, u32, Vec<String>)>;
    /// One cell of a layer.
    fn get(
        &self,
        world: &argus_ecs::World,
        map: argus_ecs::Entity,
        layer: &str,
        x: u32,
        y: u32,
    ) -> Result<u32>;
    /// Where the map's cell (0, 0)'s top-left corner is in the world, and
    /// the world units a cell is.
    fn grid(&self, world: &argus_ecs::World, map: argus_ecs::Entity) -> Result<([f32; 2], f32)>;
    /// A rect of a layer's cells (x, y, w, h), row-major: only a runtime
    /// map's.
    fn set(
        &self,
        world: &argus_ecs::World,
        map: argus_ecs::Entity,
        layer: &str,
        rect: [u32; 4],
        cells: &[u32],
    ) -> Result<()>;
}

/// The maps a host was given.
pub(crate) struct TilesData(pub Rc<dyn Tiles>);

/// The tables a host was given, and the frozen Lua tables made from them
/// (a row is made once, not every time a system asks).
pub(crate) struct DefsData {
    pub rows: Rc<dyn DefRows>,
    pub made: std::cell::RefCell<std::collections::HashMap<String, mlua::RegistryKey>>,
}

/// Where in the pipeline a script error happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorPhase {
    /// The file did not parse.
    Syntax,
    /// The file ran but raised while loading (before or outside a declaration).
    Load,
    /// A declaration was malformed or conflicted with the world.
    Declaration,
}

/// One entry of `script.errors` (§7): file, line, message. Never a crash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptError {
    /// Project-relative path.
    pub file: String,
    /// Line, when the message carried one.
    pub line: Option<u32>,
    /// The compiler's or the loader's message.
    pub message: String,
    /// What was happening.
    pub phase: ErrorPhase,
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Some(l) => write!(f, "{}:{l}: {}", self.file, self.message),
            None => write!(f, "{}: {}", self.file, self.message),
        }
    }
}

/// A registered script component, as `argus docs` lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentInfo {
    /// Component name.
    pub name: String,
    /// Declaring file.
    pub file: String,
    /// Schema version.
    pub version: u32,
    /// Whether a `migrate` function was declared.
    pub has_migrate: bool,
}

/// What a reload did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReloadReport {
    /// Files re-evaluated (new or changed).
    pub reloaded: Vec<String>,
    /// Files that disappeared; their systems were removed.
    pub removed: Vec<String>,
    /// Errors now outstanding across all files.
    pub errors: usize,
}

#[derive(Default)]
struct FileRecord {
    hash: u64,
    systems: Vec<String>,
}

/// The Luau VM plus everything loaded into it.
pub struct ScriptHost {
    lua: Lua,
    config: ScriptConfig,
    root: Option<PathBuf>,
    /// The game and its mods (phase 3 item 8): a mod's files are read
    /// after the game's, under `mods/<id>/`, and what they declare is
    /// checked against its namespace.
    layers: Option<Layers>,
    files: BTreeMap<String, FileRecord>,
    components: Vec<ComponentInfo>,
    migrations: BTreeMap<String, MigrateDecl>,
    defs: Vec<DefDecl>,
    errors: BTreeMap<String, Vec<ScriptError>>,
    /// The run order's error after the last reload, when the systems as
    /// declared cannot be ordered (an `after` naming no system): kept apart
    /// from the files' errors, since the fix may be in another file.
    order: Option<ScriptError>,
    budget: Rc<Cell<i64>>,
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Find an `ArgusError` anywhere in an mlua error's cause chain: a Rust
/// callback error that crossed Lua frames arrives wrapped in `CallbackError`.
fn find_argus(e: &mlua::Error) -> Option<&ArgusError> {
    match e {
        mlua::Error::CallbackError { cause, .. } => find_argus(cause),
        mlua::Error::WithContext { cause, .. } => find_argus(cause),
        other => other.downcast_ref::<ArgusError>(),
    }
}

fn to_argus(e: mlua::Error) -> ArgusError {
    if let Some(a) = find_argus(&e) {
        let mut a = a.clone();
        let here = parse_location(&e.to_string()).map(|(f, l)| format!("{f}:{l}"));
        match &a.r#ref {
            None => a.r#ref = here,
            // A script schema's line is where its range is declared, which
            // is the thing to read; an engine component's is Rust source
            // nobody edits, so the script's own line says more (found
            // building Outpost, phase 3 item 3).
            Some(at) if !at.contains(".luau:") && here.is_some() => {
                let decl = at.clone();
                a.hint = if a.hint.is_empty() {
                    format!("The field is declared at {decl}.")
                } else {
                    format!("{} The field is declared at {decl}.", a.hint)
                };
                a.r#ref = here;
            }
            Some(_) => {}
        }
        return a;
    }
    let text = e.to_string();
    let first = text.lines().next().unwrap_or("").to_string();
    let code = if first.contains("instruction budget") {
        codes::BUDGET_EXCEEDED
    } else if matches!(e, mlua::Error::SyntaxError { .. }) {
        codes::SYNTAX
    } else {
        codes::SCRIPT_RUNTIME
    };
    let message = strip_prefix(&first);
    let r#ref = parse_location(&text).map(|(f, l)| format!("{f}:{l}"));
    let hint = if code == codes::BUDGET_EXCEEDED {
        "The system ran past its instruction budget; look for an unbounded loop, or raise ScriptConfig.interrupt_budget."
    } else {
        ""
    };
    ArgusError::new(code, message).hint(hint).at(r#ref)
}

/// Turn `runtime error: [string "=x.luau"]:3: msg` into `x.luau:3: msg`.
fn strip_prefix(s: &str) -> String {
    let s = s
        .trim_start_matches("runtime error: ")
        .trim_start_matches("syntax error: ")
        .trim_start_matches("callback error: ");
    let s = s.replace("[string \"=", "").replace("[string \"", "");
    s.replace("\"]:", ":")
}

/// Find `<file>.luau:<line>` anywhere in a message.
fn parse_location(msg: &str) -> Option<(String, u32)> {
    let idx = msg.find(".luau:")?;
    let after = &msg[idx + 6..];
    let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    let line: u32 = digits.parse().ok()?;
    let before = &msg[..idx + 5];
    let start = before
        .rfind(['"', '[', '(', ' ', '\n', '\t'])
        .map(|i| i + 1)
        .unwrap_or(0);
    let mut file = before[start..].to_string();
    if let Some(f) = file.strip_prefix('=') {
        file = f.to_string();
    }
    Some((file, line))
}

impl ScriptHost {
    /// Give scripts path queries (`q:path`, `q:walkable`, `q:distance`,
    /// `q:downhill`; phase 3 item 2).
    pub fn set_paths(&self, paths: Rc<dyn Paths>) {
        self.lua.set_app_data(PathsData(paths));
    }

    /// Give scripts the game's maps (`q:tiles`; phase 5 item 4).
    pub fn set_tiles(&self, tiles: Rc<dyn Tiles>) {
        self.lua.set_app_data(TilesData(tiles));
    }

    /// Give scripts the game's string tables (`q:text`, `q:languages`;
    /// phase 3 item 4).
    pub fn set_texts(&self, texts: Rc<dyn Texts>) {
        self.lua.set_app_data(TextsData(texts));
    }

    /// Give scripts the project's data tables (`q:def`, `q:defs`; phase 3
    /// item 3). Called again when the tables are read again, which forgets
    /// the rows made from the old ones.
    pub fn set_defs(&self, rows: Rc<dyn DefRows>) {
        self.lua.set_app_data(DefsData {
            rows,
            made: Default::default(),
        });
    }

    /// Create the VM: declaration globals installed, the ones that would
    /// make a run differ from its replay taken out, memory limit and
    /// interrupt budget set, then sandboxed so globals are read-only (§7).
    pub fn new(config: ScriptConfig) -> Result<ScriptHost> {
        let lua = Lua::new();
        lua.set_app_data(Pending::default());
        lua.set_app_data(DayLength(config.day_seconds));
        decl::install_globals(&lua).map_err(to_argus)?;
        deterministic_globals(&lua).map_err(to_argus)?;
        lua.set_memory_limit(config.memory_limit)
            .map_err(to_argus)?;
        let budget = Rc::new(Cell::new(i64::MAX));
        {
            let budget = budget.clone();
            lua.set_interrupt(move |_| {
                let b = budget.get() - 1;
                budget.set(b);
                if b < 0 {
                    Err(mlua::Error::runtime("instruction budget exceeded"))
                } else {
                    Ok(VmState::Continue)
                }
            });
        }
        lua.sandbox(true).map_err(to_argus)?;
        Ok(ScriptHost {
            lua,
            config,
            root: None,
            layers: None,
            files: BTreeMap::new(),
            components: Vec::new(),
            migrations: BTreeMap::new(),
            defs: Vec::new(),
            errors: BTreeMap::new(),
            order: None,
            budget,
        })
    }

    /// The configuration.
    pub fn config(&self) -> &ScriptConfig {
        &self.config
    }

    /// Load a project: `components/`, then `defs/`, then `systems/`, each
    /// recursively, files in sorted order. Returns true if no file errored.
    pub fn load_project(
        &mut self,
        world: &mut World,
        sched: &mut Scheduler,
        root: &Path,
    ) -> Result<bool> {
        self.load_layers(world, sched, &Layers::game(root))
    }

    /// [`load_project`](Self::load_project) for the game and its mods
    /// (phase 3 item 8, §15): the game's files, then each mod's in load
    /// order, a mod's named from the game's root (`mods/<id>/systems/..`).
    /// A mod runs in the same sandbox with the same instruction budget;
    /// what it declares must be in its namespace (`<id>.name`,
    /// `<id>_Name`), and it cannot redeclare the game's systems,
    /// components, or def tables.
    pub fn load_layers(
        &mut self,
        world: &mut World,
        sched: &mut Scheduler,
        layers: &Layers,
    ) -> Result<bool> {
        self.root = Some(layers.game.clone());
        self.layers = Some(layers.clone());
        let mut ok = true;
        for (shown, path) in scan_layers(layers) {
            // Source, or bytecode where a release pack compiled it (§15).
            let src = argus_os::fs::read(&path)
                .map_err(|e| ArgusError::new(codes::SYNTAX, format!("cannot read {shown}: {e}")))?;
            ok &= self.load_bytes(world, sched, &shown, &src);
        }
        Ok(ok)
    }

    /// The game and mods the scripts were loaded from.
    pub fn layers(&self) -> Option<&Layers> {
        self.layers.as_ref()
    }

    /// Re-scan the project: re-evaluate new and changed files, drop the
    /// systems of deleted files. World state is untouched; systems continue
    /// from the current frame (§7). Returns what changed.
    pub fn reload(&mut self, world: &mut World, sched: &mut Scheduler) -> Result<ReloadReport> {
        let Some(root) = self.root.clone() else {
            return Ok(ReloadReport {
                errors: self.errors().len(),
                ..Default::default()
            });
        };
        let layers = self.layers.clone().unwrap_or_else(|| Layers::game(&root));
        let mut report = ReloadReport::default();
        let found = scan_layers(&layers);
        let present: Vec<String> = found.iter().map(|(s, _)| s.clone()).collect();
        for (rel, path) in &found {
            let src = argus_os::fs::read(path)
                .map_err(|e| ArgusError::new(codes::SYNTAX, format!("cannot read {rel}: {e}")))?;
            let changed = self
                .files
                .get(rel)
                .map(|r| r.hash != fnv(&src))
                .unwrap_or(true);
            if changed {
                self.load_bytes(world, sched, rel, &src);
                report.reloaded.push(rel.clone());
            }
        }
        // A file that failed because of another (systems reading a
        // component whose file did not load) is tried again once anything
        // changed, so fixing the cause clears what followed from it (found
        // at the phase 5 gate: sixteen systems' errors stayed after their
        // component's file was fixed, until a scene load).
        if !report.reloaded.is_empty() {
            for (rel, path) in &found {
                if report.reloaded.contains(rel) || !self.errors.contains_key(rel) {
                    continue;
                }
                let src = argus_os::fs::read(path).map_err(|e| {
                    ArgusError::new(codes::SYNTAX, format!("cannot read {rel}: {e}"))
                })?;
                self.load_bytes(world, sched, rel, &src);
                report.reloaded.push(rel.clone());
            }
        }
        let gone: Vec<String> = self
            .files
            .keys()
            .filter(|k| !present.contains(k))
            .cloned()
            .collect();
        for rel in gone {
            self.forget_file(sched, &rel);
            report.removed.push(rel);
        }
        report.errors = self.errors().len();
        Ok(report)
    }

    fn forget_file(&mut self, sched: &mut Scheduler, rel: &str) {
        if let Some(rec) = self.files.remove(rel) {
            for s in rec.systems {
                let _ = sched.remove_system(&s);
            }
        }
        self.errors.remove(rel);
        self.components.retain(|c| c.file != rel);
        self.defs.retain(|d| d.file != rel);
    }

    /// Evaluate one file's source under its project-relative name. Errors
    /// are recorded, not returned; the result says whether there were none.
    pub fn load_source(
        &mut self,
        world: &mut World,
        sched: &mut Scheduler,
        rel: &str,
        src: &str,
    ) -> bool {
        self.load_bytes(world, sched, rel, src.as_bytes())
    }

    /// [`load_source`](Self::load_source) for source or for [`compile`]d
    /// bytecode, which Luau tells apart by its first byte.
    pub fn load_bytes(
        &mut self,
        world: &mut World,
        sched: &mut Scheduler,
        rel: &str,
        src: &[u8],
    ) -> bool {
        self.errors.remove(rel);
        let previous_systems = self
            .files
            .get(rel)
            .map(|r| r.systems.clone())
            .unwrap_or_default();
        self.components.retain(|c| c.file != rel);
        self.defs.retain(|d| d.file != rel);
        {
            let mut p = self.lua.app_data_mut::<Pending>().expect("pending");
            p.current_file = rel.to_string();
            p.components.clear();
            p.systems.clear();
            p.defs.clear();
        }
        self.budget.set(self.config.interrupt_budget as i64);

        let chunk = self.lua.load(src).set_name(format!("={rel}"));
        let func = match chunk.into_function() {
            Ok(f) => f,
            Err(e) => {
                self.record(rel, ErrorPhase::Syntax, e);
                self.files.insert(
                    rel.to_string(),
                    FileRecord {
                        hash: fnv(src),
                        systems: Vec::new(),
                    },
                );
                return false;
            }
        };
        if let Err(e) = func.call::<()>(()) {
            let phase = if find_argus(&e).is_some() {
                ErrorPhase::Declaration
            } else {
                ErrorPhase::Load
            };
            self.record(rel, phase, e);
        }
        // Freeze the chunk's environment: module load may define globals,
        // but nothing may live there afterwards (§7 "freezes script globals
        // after module load"). A run function that writes a global fails.
        if let Some(env) = func.environment() {
            env.set_readonly(true);
        }

        let pending = {
            let mut p = self.lua.app_data_mut::<Pending>().expect("pending");
            (
                std::mem::take(&mut p.components),
                std::mem::take(&mut p.defs),
                std::mem::take(&mut p.systems),
            )
        };
        let mut ok = !self.errors.contains_key(rel);
        let mut declared_systems = Vec::new();
        // A mod's file declares only in its namespace (phase 3 item 8).
        let owner = self
            .layers
            .as_ref()
            .and_then(|l| l.owner(rel))
            .map(str::to_string);
        let (mut components, mut defs, mut systems) = pending;
        if let Some(id) = &owner {
            let mut refused = Vec::new();
            components.retain(|c| {
                let n = &c.schema.name;
                if layers::owns(id, n) {
                    return true;
                }
                refused.push(if world.component_id(n).is_some() {
                    layers::not_changeable(id, "component", n, rel)
                } else {
                    layers::outside(id, "component", n, rel)
                });
                false
            });
            defs.retain(|d| {
                let n = &d.schema.name;
                if layers::owns(id, n) {
                    return true;
                }
                let theirs = self.defs.iter().any(|x| &x.schema.name == n);
                refused.push(if theirs {
                    layers::not_changeable(id, "def table", n, rel)
                } else {
                    layers::outside(id, "def table", n, rel)
                });
                false
            });
            systems.retain(|s| {
                if layers::owns(id, &s.name) {
                    return true;
                }
                refused.push(if sched.is_enabled(&s.name).is_ok() {
                    layers::not_changeable(id, "system", &s.name, s.source.clone())
                } else {
                    layers::outside(id, "system", &s.name, s.source.clone())
                });
                false
            });
            for e in refused {
                ok = false;
                self.push_error(rel, ErrorPhase::Declaration, e);
            }
        }

        for c in components {
            let name = c.schema.name.clone();
            let version = c.schema.version;
            match world.register_schema(c.schema) {
                Ok(_) => {
                    self.components.push(ComponentInfo {
                        name: name.clone(),
                        file: rel.to_string(),
                        version,
                        has_migrate: c.migrate.is_some(),
                    });
                    match c.migrate {
                        Some(k) => {
                            self.migrations.insert(name, k);
                        }
                        None => {
                            self.migrations.remove(&name);
                        }
                    }
                }
                Err(e) => {
                    ok = false;
                    self.push_error(rel, ErrorPhase::Declaration, e);
                }
            }
        }
        for d in defs {
            self.defs.retain(|x| x.schema.name != d.schema.name);
            self.defs.push(d);
        }
        for s in systems {
            match self.bind_system(world, &s) {
                Ok((desc, run)) => match sched.replace_system(desc, run) {
                    Ok(()) => declared_systems.push(s.name.clone()),
                    Err(e) => {
                        ok = false;
                        self.push_error(rel, ErrorPhase::Declaration, e);
                    }
                },
                Err(e) => {
                    ok = false;
                    self.push_error(rel, ErrorPhase::Declaration, e);
                }
            }
        }
        // Systems this file used to declare but no longer does.
        for old in previous_systems {
            if !declared_systems.contains(&old) {
                let _ = sched.remove_system(&old);
            }
        }
        self.files.insert(
            rel.to_string(),
            FileRecord {
                hash: fnv(src),
                systems: declared_systems,
            },
        );
        ok
    }

    fn bind_system(&self, world: &World, s: &SystemDecl) -> Result<(SystemDesc, SystemFn)> {
        let src = Some(s.source.clone());
        let stage = Stage::parse(&s.stage).ok_or_else(|| {
            ArgusError::new(
                codes::BAD_DECLARATION,
                format!("system {}: unknown stage {:?}", s.name, s.stage),
            )
            .hint("One of input, sim, late, hierarchy, present.")
            .at(src.clone())
        })?;
        let clock = match s.clock.as_deref() {
            None | Some("driving") => ClockBinding::Driving,
            Some("continuous") => ClockBinding::Continuous,
            Some("discrete") => ClockBinding::Discrete,
            Some(other) => {
                return Err(ArgusError::new(
                    codes::BAD_DECLARATION,
                    format!("system {}: unknown clock {other:?}", s.name),
                )
                .hint("One of driving, continuous, discrete.")
                .at(src))
            }
        };
        let reads: Vec<&str> = s.reads.iter().map(String::as_str).collect();
        let writes: Vec<&str> = s.writes.iter().map(String::as_str).collect();
        let access = Access::from_names(world.registry(), &reads, &writes).map_err(|e| {
            ArgusError::new(e.code, format!("system {}: {}", s.name, e.message))
                .hint("Declare the component in components/ before the system that uses it.")
                .at(src.clone())
        })?;
        let mut desc = SystemDesc::new(&s.name, stage)
            .access(access)
            .clock(clock)
            .source(s.source.clone())
            .doc(&s.doc);
        for a in &s.after {
            desc = desc.after(a);
        }
        if let Some(r) = &s.rng {
            desc = desc.rng(r);
        }
        if s.on_load {
            desc = desc.on_load();
        }
        if s.unfrozen {
            desc = desc.unfrozen();
        }
        let run: Function = self.lua.registry_value(&s.run).map_err(to_argus)?;
        let lua = self.lua.clone();
        let budget = self.budget.clone();
        let limit = self.config.interrupt_budget as i64;
        let f: SystemFn = Box::new(move |ctx| {
            // The 'static is a lie the proxies never get to exploit: the
            // link is nulled before this closure returns.
            let raw: *mut argus_sim::SystemCtx<'_> = ctx;
            let link: Link = Rc::new(Cell::new(raw.cast::<argus_sim::SystemCtx<'static>>()));
            budget.set(limit);
            let q = lua
                .create_userdata(QueryHandle { link: link.clone() })
                .map_err(to_argus)?;
            let r = run.call::<()>(q);
            link.set(std::ptr::null_mut());
            r.map_err(to_argus)
        });
        Ok((desc, f))
    }

    fn record(&mut self, rel: &str, phase: ErrorPhase, e: mlua::Error) {
        let a = to_argus(e);
        self.push_error(rel, phase, a);
    }

    fn push_error(&mut self, rel: &str, phase: ErrorPhase, a: ArgusError) {
        let (file, line) = a
            .r#ref
            .as_deref()
            .and_then(|r| parse_location(&format!("{r}:")).or_else(|| parse_location(r)))
            .map(|(f, l)| (f, Some(l)))
            .unwrap_or((rel.to_string(), None));
        let mut message = a.message.clone();
        if let Some(l) = line {
            let prefix = format!("{file}:{l}: ");
            if let Some(rest) = message.strip_prefix(&prefix) {
                message = rest.to_string();
            }
        }
        if !a.hint.is_empty() {
            message.push(' ');
            message.push_str(&a.hint);
        }
        let file = if file.is_empty() {
            rel.to_string()
        } else {
            file
        };
        self.errors
            .entry(rel.to_string())
            .or_default()
            .push(ScriptError {
                file,
                line,
                message,
                phase,
            });
    }

    /// Every outstanding error, by file (`script.errors`), and the run
    /// order's.
    pub fn errors(&self) -> Vec<&ScriptError> {
        self.errors
            .values()
            .flatten()
            .chain(self.order.iter())
            .collect()
    }

    /// What ordering the systems after a reload said: an error stays in
    /// `script.errors` until a reload orders them (found at the phase 5
    /// gate: a missing `after` target was in the reload's answer only).
    pub fn set_order_error(&mut self, e: Option<&ArgusError>) {
        self.order = e.map(|e| {
            let at = e.r#ref.clone().unwrap_or_default();
            let (file, line) = match at.rsplit_once(':') {
                Some((f, l)) if l.parse::<u32>().is_ok() => (f.to_string(), l.parse().ok()),
                _ => (at, None),
            };
            ScriptError {
                file,
                line,
                message: e.message.clone(),
                phase: ErrorPhase::Declaration,
            }
        });
    }

    /// Components declared from script.
    pub fn components(&self) -> &[ComponentInfo] {
        &self.components
    }

    /// Def table schemas declared from script.
    pub fn defs(&self) -> &[DefDecl] {
        &self.defs
    }

    /// Whether a component declared a `migrate` function.
    pub fn has_migration(&self, component: &str) -> bool {
        self.migrations.contains_key(component)
    }

    /// Systems declared by a file.
    pub fn systems_in(&self, rel: &str) -> &[String] {
        self.files
            .get(rel)
            .map(|r| r.systems.as_slice())
            .unwrap_or(&[])
    }

    /// Files known to the host.
    pub fn files(&self) -> Vec<&str> {
        self.files.keys().map(String::as_str).collect()
    }

    /// Bytes the Luau heap currently holds.
    pub fn memory_used(&self) -> usize {
        self.lua.used_memory()
    }
}

/// Compile a script to Luau bytecode, as a release pack ships it (§15:
/// "Luau precompiled to bytecode"). The options are the ones loading source
/// uses (optimisation 1, line numbers kept), so a precompiled script runs
/// exactly as its source does and its errors still name lines. A syntax
/// error is the same `script.errors` row a load would give.
pub fn compile(rel: &str, src: &str) -> std::result::Result<Vec<u8>, ScriptError> {
    mlua::Compiler::new().compile(src).map_err(|e| {
        let text = match &e {
            mlua::Error::SyntaxError { message, .. } => message.clone(),
            other => other.to_string(),
        };
        // Luau says `[string "..."]:3: message` or `:3: message`.
        let tail = text.rsplit_once("]:").map_or(text.as_str(), |(_, t)| t);
        let tail = tail.trim_start_matches(':');
        let (line, message) = match tail.split_once(':') {
            Some((n, m)) if n.trim().parse::<u32>().is_ok() => {
                (n.trim().parse().ok(), m.trim().to_string())
            }
            _ => (None, text.trim().to_string()),
        };
        ScriptError {
            file: rel.to_string(),
            line,
            message,
            phase: ErrorPhase::Syntax,
        }
    })
}

/// Luau's own `math.<name>` over `inputs` (one argument, or two for
/// `atan2` and `pow`, from `second`), in a fresh VM: what a script gets on
/// this platform, from the C library Luau was built against. The
/// cross-platform determinism spike's probe (phase 5 item 11).
pub fn math_outputs(name: &str, inputs: &[f64], second: &[f64]) -> Result<Vec<f64>> {
    let lua = Lua::new();
    let f: Function = lua
        .globals()
        .get::<mlua::Table>("math")
        .and_then(|m| m.get(name))
        .map_err(to_argus)?;
    inputs
        .iter()
        .enumerate()
        .map(|(i, x)| match second.get(i) {
            Some(y) => f.call::<f64>((*x, *y)),
            None => f.call::<f64>(*x),
        })
        .collect::<mlua::Result<Vec<f64>>>()
        .map_err(to_argus)
}

/// Project-relative `.luau` files in load order.
/// Every layer's script files, the game's first: each one's name from the
/// game's root (`systems/x.luau`, `mods/<id>/systems/y.luau`) and its path.
fn scan_layers(layers: &Layers) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for l in layers.each() {
        for rel in scan(l.root) {
            let path = l.root.join(&rel);
            out.push((l.show(&rel), path));
        }
    }
    out
}

fn scan(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for dir in ["components", "defs", "systems"] {
        let mut files = Vec::new();
        walk(&root.join(dir), &mut files);
        files.sort();
        for f in files {
            if let Ok(rel) = f.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = argus_os::fs::read_dir(dir) else {
        return;
    };
    for entry in rd {
        let p = entry.path;
        if entry.is_dir {
            walk(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("luau") {
            out.push(p);
        }
    }
}

/// Take out what would make a run differ from its replay (§7: scripts run
/// with no clock and no device; randomness is the seeded `q:rng()`). Lua's
/// standard library came with `os` (the time, the date, a clock) and an
/// unseeded `math.random`: a script could read them, and a seek or a
/// replay then gave another result. Found writing the manual; no example
/// used them. `math.random` and `math.randomseed` now say what to use.
fn deterministic_globals(lua: &Lua) -> mlua::Result<()> {
    lua.load(
        r#"
        os = nil
        local function no_random()
          error("math.random would make a run differ from its replay: give the system a stream (rng = \"name\") and use q:rng():next(), q:rng():range(lo, hi), or q:rng():chance(p)", 2)
        end
        math.random = no_random
        math.randomseed = no_random
        "#,
    )
    .set_name("=argus")
    .exec()
}
