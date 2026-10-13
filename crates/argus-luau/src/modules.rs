//! Shared code for game scripts (GitHub issue 24): `require("rules")` in a
//! system or component file evaluates `lib/rules.luau` once and hands every
//! file that asks the same value, frozen. A module is Luau that returns a
//! value, usually a table of functions and constants; it runs in the same
//! sandbox as the systems, and its calls count against the calling
//! system's budget. A mod's files look in the mod's own `lib/` first, then
//! the game's; the game's files only in the game's, so a mod adds modules
//! but cannot replace one (Kevin, 2026-10-13).
//!
//! The host records which modules each file required, directly or through
//! another module, so a reload that finds a module changed reloads every
//! file that uses it.

use argus_ecs::Layers;
use mlua::{Lua, RegistryKey, Value};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

/// The modules of a project, as the VM has them.
#[derive(Default)]
pub(crate) struct Modules {
    /// Where to look: the game and its mods, once a project is loaded.
    pub layers: Option<Layers>,
    /// Each module evaluated, by its path from the game's root: its value
    /// and the hash of the bytes it was made from.
    pub loaded: BTreeMap<String, (RegistryKey, u64)>,
    /// What each file (a declaration file or a module) required, by path.
    pub uses: BTreeMap<String, BTreeSet<String>>,
    /// Modules being evaluated now, outermost first: a cycle's chain.
    loading: Vec<String>,
    /// A module that did not evaluate, by path, with what it said: only
    /// the one whose own code failed, not the modules requiring it.
    pub errors: BTreeMap<String, String>,
    /// The failure a module that did not evaluate passed up, so the files
    /// above it name it once instead of nesting it at each level.
    root: Option<String>,
}

impl Modules {
    /// Where `name` is for a file at `caller` (its path from the game's
    /// root): its path from the game's root and on disk.
    fn resolve(&self, caller: &str, name: &str) -> Result<(String, PathBuf), String> {
        let Some(layers) = &self.layers else {
            return Err(format!(
                "require({name:?}): a module is a file of a project's lib/ folder, and these scripts were loaded without a project"
            ));
        };
        let file = format!("lib/{name}.luau");
        let mut tried = Vec::new();
        // A mod's own lib/ first, for its own files.
        if let Some(id) = layers.owner(caller) {
            if let Some(l) = layers.each().into_iter().find(|l| l.id == Some(id)) {
                let path = l.root.join(&file);
                let shown = l.show(&file);
                if argus_os::fs::is_file(&path) {
                    return Ok((shown, path));
                }
                tried.push(shown);
            }
        }
        let path = layers.game.join(&file);
        if argus_os::fs::is_file(&path) {
            return Ok((file, path));
        }
        tried.push(file);
        let there = module_names(&layers.game);
        Err(format!(
            "require({name:?}): there is no {}; the game's modules are {}",
            tried.join(" or "),
            if there.is_empty() {
                "none (a module is a .luau file in lib/, which returns its value)".to_string()
            } else {
                there.join(", ")
            }
        ))
    }

    /// Every module `file` uses, directly or through another module.
    pub fn closure(&self, file: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut todo: Vec<&str> = vec![file];
        while let Some(f) = todo.pop() {
            for m in self.uses.get(f).into_iter().flatten() {
                if out.insert(m.clone()) {
                    todo.push(m);
                }
            }
        }
        out
    }
}

/// The modules of a project's `lib/`, by the names `require` takes.
fn module_names(root: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    let lib = root.join("lib");
    fn walk(dir: &std::path::Path, lib: &std::path::Path, out: &mut Vec<String>) {
        let Ok(rd) = argus_os::fs::read_dir(dir) else {
            return;
        };
        for e in rd {
            if e.is_dir {
                walk(&e.path, lib, out);
            } else if let Ok(rel) = e.path.strip_prefix(lib) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                if let Some(n) = rel.strip_suffix(".luau") {
                    out.push(n.to_string());
                }
            }
        }
    }
    walk(&lib, &lib, &mut out);
    out.sort();
    out
}

/// The modules' files under each layer's `lib/`, by their paths from the
/// game's root, with where they are on disk.
pub(crate) fn scan(layers: &Layers) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for l in layers.each() {
        for n in module_names(l.root) {
            let rel = format!("lib/{n}.luau");
            out.push((l.show(&rel), l.root.join(&rel)));
        }
    }
    out
}

/// A file's leading comment: its `--` lines before the code, as one
/// paragraph.
pub(crate) fn leading_comment(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in text.lines() {
        let Some(c) = line.trim().strip_prefix("--") else {
            break;
        };
        if c.starts_with('[') {
            break;
        }
        let c = c.trim_start_matches('-').trim();
        if !c.is_empty() {
            out.push(c);
        }
    }
    out.join(" ")
}

/// A module's name: path parts of letters, digits, `_`, `-`, and `.`.
fn check_name(name: &str) -> mlua::Result<()> {
    let ok = !name.is_empty()
        && name.split('/').all(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c))
        });
    if ok {
        Ok(())
    } else {
        Err(mlua::Error::runtime(format!(
            "require({name:?}): a module's name is its path in lib/ without .luau, such as \"rules\" or \"rules/checks\""
        )))
    }
}

/// The file that called: its chunk's name, the path from the game's root.
fn caller(lua: &Lua) -> (String, i32) {
    lua.inspect_stack(1)
        .map(|d| {
            let file = d
                .source()
                .source
                .map(|s| s.trim_start_matches('=').to_string())
                .unwrap_or_default();
            (file, d.curr_line())
        })
        .unwrap_or_default()
}

/// The first line of what a failed evaluation said, as `file:line: msg`.
pub(crate) fn said(e: &mlua::Error) -> String {
    crate::strip_prefix(e.to_string().lines().next().unwrap_or(""))
}

/// Freeze a module's value, and every table in it, so one system cannot
/// change what another sees.
fn freeze(v: &Value, seen: &mut HashSet<*const std::ffi::c_void>) {
    let Value::Table(t) = v else {
        return;
    };
    if !seen.insert(t.to_pointer()) {
        return;
    }
    for (k, v) in t.clone().pairs::<Value, Value>().flatten() {
        freeze(&k, seen);
        freeze(&v, seen);
    }
    t.set_readonly(true);
}

/// The global `require`.
pub(crate) fn install(lua: &Lua, m: Rc<RefCell<Modules>>) -> mlua::Result<()> {
    let require = lua.create_function(move |lua, name: String| {
        check_name(&name)?;
        let (from, line) = caller(lua);
        // Each refusal starts with the line of the require, so the error
        // points there, in the file that asked.
        let at = if line > 0 {
            format!("{from}:{line}: ")
        } else {
            String::new()
        };
        let fail = |msg: String| mlua::Error::runtime(format!("{at}{msg}"));
        let (rel, path) = m.borrow().resolve(&from, &name).map_err(fail)?;
        m.borrow_mut()
            .uses
            .entry(from)
            .or_default()
            .insert(rel.clone());
        if let Some((key, _)) = m.borrow().loaded.get(&rel) {
            return lua.registry_value::<Value>(key);
        }
        if let Some(i) = m.borrow().loading.iter().position(|r| *r == rel) {
            let mut steps: Vec<String> = m.borrow().loading[i..].to_vec();
            steps.push(rel.clone());
            let chain = steps.join(", which requires ");
            return Err(fail(format!(
                "require({name:?}): modules cannot require each other in a circle: {chain}"
            )));
        }
        let bytes = argus_os::fs::read(&path)
            .map_err(|e| fail(format!("require({name:?}): {rel}: {e}")))?;
        {
            let mut mm = m.borrow_mut();
            mm.loading.push(rel.clone());
            mm.uses.remove(&rel);
            mm.root = None;
        }
        let made = (|| -> mlua::Result<Value> {
            let func = lua
                .load(&bytes[..])
                .set_name(format!("={rel}"))
                .into_function()?;
            let v: Value = func.call(())?;
            if let Some(env) = func.environment() {
                env.set_readonly(true);
            }
            freeze(&v, &mut HashSet::new());
            Ok(v)
        })();
        m.borrow_mut().loading.pop();
        match made {
            Ok(v) => {
                let key = lua.create_registry_value(v.clone())?;
                let mut mm = m.borrow_mut();
                mm.loaded.insert(rel.clone(), (key, crate::fnv(&bytes)));
                mm.errors.remove(&rel);
                Ok(v)
            }
            Err(e) => {
                let text = said(&e);
                let mut mm = m.borrow_mut();
                // A module it required failed: that one's failure is the
                // news, recorded where it happened, and passed up as is.
                let root = match &mm.root {
                    Some(r) if text.contains(r.as_str()) => r.clone(),
                    _ => {
                        mm.errors.insert(rel.clone(), text.clone());
                        mm.root = Some(text.clone());
                        text
                    }
                };
                Err(fail(format!(
                    "require({name:?}): {rel} did not load: {root}"
                )))
            }
        }
    })?;
    lua.globals().set("require", require)
}
