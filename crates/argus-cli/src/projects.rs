//! The projects this machine knows (distribution item 3), so that one
//! user-level `argus mcp` entry in an agent host serves every project: it
//! lists them, opens one, or makes one.
//!
//! A project is remembered when the CLI connects to its daemon (any
//! command, the editor, the MCP server) or `argus new` makes it, in
//! `projects.json` in Argus's own data folder (`$ARGUS_HOME`, else the
//! platform's: `%APPDATA%\Argus`, `~/.local/share/Argus`). A project in
//! the temp folder is not remembered.

use serde_json::{json, Value as Json};
use std::path::{Path, PathBuf};

/// The most projects remembered.
const KEEP: usize = 30;

/// How deep `under` looks for projects in a folder.
const DEPTH: usize = 3;

/// Argus's own data folder.
pub fn home() -> Option<PathBuf> {
    match std::env::var("ARGUS_HOME") {
        Ok(d) if !d.is_empty() => Some(PathBuf::from(d)),
        _ => argus_player::save::save_dir("Argus"),
    }
}

fn file() -> Option<PathBuf> {
    home().map(|h| h.join("projects.json"))
}

fn read() -> Vec<Json> {
    file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| serde_json::from_str::<Json>(&t).ok())
        .and_then(|v| v["recent"].as_array().cloned())
        .unwrap_or_default()
}

fn key(p: &Path) -> String {
    crate::build::plain(p)
}

/// Remember a project as the most recent. Nothing is written when it is
/// already first, and a failure to write is no one's problem.
pub fn remember(root: &Path) {
    let Some(f) = file() else { return };
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if scratch(&root) {
        return;
    }
    let path = key(&root);
    let mut list = read();
    if list.first().and_then(|p| p["path"].as_str()) == Some(path.as_str()) {
        return;
    }
    list.retain(|p| p["path"].as_str() != Some(path.as_str()));
    list.insert(
        0,
        json!({ "path": path, "opened": argus_os::time::unix_secs() }),
    );
    list.truncate(KEEP);
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(
        &f,
        serde_json::to_string_pretty(&json!({ "recent": list })).unwrap_or_default(),
    );
}

/// One project as the tools list it: its path, its game's name, when it
/// was last opened.
pub fn describe(root: &Path, opened: Option<&Json>) -> Json {
    let name = argus_scene::ProjectConfig::load(root)
        .map(|c| c.name)
        .unwrap_or_default();
    let mut o = json!({ "path": key(root), "name": name });
    if let Some(t) = opened.filter(|t| !t.is_null()) {
        o["opened"] = t.clone();
    }
    o
}

/// The remembered projects that are still there, most recent first.
pub fn recent() -> Vec<Json> {
    read()
        .iter()
        .filter_map(|p| {
            let path = PathBuf::from(p["path"].as_str()?);
            (!scratch(&path) && path.join("argus.toml").is_file())
                .then(|| describe(&path, p.get("opened")))
        })
        .collect()
}

/// A project not to come back to: in the temp folder (a test's, a
/// trial's), or in a cargo build's `target/tmp` (a test's scratch, which
/// filled Kevin's list on the first day).
pub fn scratch(root: &Path) -> bool {
    // Compared as plain text, without Windows' verbatim prefix and case
    // blind, since a stored path has neither and a canonical one both.
    let flat = |p: &Path| key(p).replace('\\', "/").to_lowercase();
    let at = flat(root);
    let temp = std::env::temp_dir();
    let temps = [flat(&temp), flat(&temp.canonicalize().unwrap_or_default())];
    if temps.iter().any(|t| {
        !t.is_empty() && (at == *t || at.starts_with(&format!("{}/", t.trim_end_matches('/'))))
    }) {
        return true;
    }
    let parts: Vec<String> = root
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect();
    parts.windows(2).any(|w| w[0] == "target" && w[1] == "tmp")
}

/// Every project in a folder, a few levels down.
pub fn under(dir: &Path) -> Vec<Json> {
    let mut out = Vec::new();
    walk(dir, 0, &mut out);
    out.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    out
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<Json>) {
    if dir.join("argus.toml").is_file() {
        out.push(describe(dir, None));
        return;
    }
    if depth >= DEPTH {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || matches!(name.as_ref(), "target" | "node_modules" | "dist") {
            continue;
        }
        if e.path().is_dir() {
            walk(&e.path(), depth + 1, out);
        }
    }
}
