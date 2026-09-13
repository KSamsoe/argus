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

mod bind;
mod decl;

pub use bind::script_codes as codes;
pub use decl::{ComponentDecl, DefDecl, SystemDecl};

use argus_ecs::{Access, ArgusError, Result, World};
use argus_sim::{ClockBinding, Scheduler, Stage, SystemDesc, SystemFn};
use bind::{Link, QueryHandle};
use decl::Pending;
use mlua::{Function, Lua, RegistryKey, VmState};
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
}

impl Default for ScriptConfig {
    fn default() -> Self {
        ScriptConfig {
            interrupt_budget: 5_000_000,
            memory_limit: 256 << 20,
        }
    }
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
    files: BTreeMap<String, FileRecord>,
    components: Vec<ComponentInfo>,
    migrations: BTreeMap<String, RegistryKey>,
    defs: Vec<DefDecl>,
    errors: BTreeMap<String, Vec<ScriptError>>,
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
        if a.r#ref.is_none() {
            a.r#ref = parse_location(&e.to_string()).map(|(f, l)| format!("{f}:{l}"));
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
    /// Create the VM: declaration globals installed, memory limit and
    /// interrupt budget set, then sandboxed so globals are read-only (§7).
    pub fn new(config: ScriptConfig) -> Result<ScriptHost> {
        let lua = Lua::new();
        lua.set_app_data(Pending::default());
        decl::install_globals(&lua).map_err(to_argus)?;
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
            files: BTreeMap::new(),
            components: Vec::new(),
            migrations: BTreeMap::new(),
            defs: Vec::new(),
            errors: BTreeMap::new(),
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
        self.root = Some(root.to_path_buf());
        let mut ok = true;
        for rel in scan(root) {
            let src = std::fs::read_to_string(root.join(&rel))
                .map_err(|e| ArgusError::new(codes::SYNTAX, format!("cannot read {rel}: {e}")))?;
            ok &= self.load_source(world, sched, &rel, &src);
        }
        Ok(ok)
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
        let mut report = ReloadReport::default();
        let present = scan(&root);
        for rel in &present {
            let src = std::fs::read_to_string(root.join(rel))
                .map_err(|e| ArgusError::new(codes::SYNTAX, format!("cannot read {rel}: {e}")))?;
            let changed = self
                .files
                .get(rel)
                .map(|r| r.hash != fnv(src.as_bytes()))
                .unwrap_or(true);
            if changed {
                self.load_source(world, sched, rel, &src);
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
                        hash: fnv(src.as_bytes()),
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

        for c in pending.0 {
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
        for d in pending.1 {
            self.defs.retain(|x| x.schema.name != d.schema.name);
            self.defs.push(d);
        }
        for s in pending.2 {
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
                hash: fnv(src.as_bytes()),
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

    /// Every outstanding error, by file (`script.errors`).
    pub fn errors(&self) -> Vec<&ScriptError> {
        self.errors.values().flatten().collect()
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

/// Project-relative `.luau` files in load order.
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
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("luau") {
            out.push(p);
        }
    }
}
