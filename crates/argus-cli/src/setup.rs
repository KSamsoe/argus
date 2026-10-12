//! `argus mcp setup <client>` (distribution item 3): the Argus MCP server
//! put in an agent host's own config file, merged with what is there,
//! idempotent, with a backup of a file it changes.
//!
//! A project-scoped entry opens that project (`argus mcp --project <dir>`).
//! A user-scoped one opens none unless `--project` names one: the server
//! then starts on no project and offers them (`projects`, `open_project`,
//! `new_project`), so one entry serves every project. The command is this
//! program's own path, since an agent host may not start from a shell
//! with `argus` on its PATH.
//!
//! Where each client keeps its servers, from its own docs (checked
//! 2026-10-09; these move):
//!
//! | client | project file | user file | key |
//! |---|---|---|---|
//! | claude-code | `.mcp.json` | `~/.claude.json` (by `claude mcp add-json` when it is here) | `mcpServers` |
//! | claude-desktop | | `claude_desktop_config.json` (and the Store app's copy) | `mcpServers` |
//! | cursor | `.cursor/mcp.json` | `~/.cursor/mcp.json` | `mcpServers` |
//! | vscode | `.vscode/mcp.json` | the default profile's `mcp.json` | `servers` |
//! | codex | `.codex/config.toml` | `~/.codex/config.toml` | `[mcp_servers.argus]` |
//! | gemini | `.gemini/settings.json` | `~/.gemini/settings.json` | `mcpServers` |
//!
//! `$ARGUS_SETUP_HOME` stands in for the home folder (and the application
//! data folders under it), and then no client's own CLI is run: tests.

use crate::{jsonedit, Invocation};
use serde_json::{json, Value as Json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The server's name in every client.
const NAME: &str = "argus";

/// An agent host `argus mcp setup` knows.
pub struct Client {
    /// The name on the command line.
    pub name: &'static str,
    /// Its own name.
    pub title: &'static str,
    /// Whether it has a project file.
    pub project: bool,
    /// Where its docs say this (checked when the item was built).
    pub docs: &'static str,
}

/// The clients, in the order `argus mcp setup` lists them.
pub const CLIENTS: &[Client] = &[
    Client {
        name: "claude-code",
        title: "Claude Code",
        project: true,
        docs: "https://code.claude.com/docs/en/mcp",
    },
    Client {
        name: "claude-desktop",
        title: "Claude Desktop",
        project: false,
        docs: "https://modelcontextprotocol.io/docs/develop/connect-local-servers",
    },
    Client {
        name: "cursor",
        title: "Cursor",
        project: true,
        docs: "https://cursor.com/docs/context/mcp",
    },
    Client {
        name: "vscode",
        title: "VS Code",
        project: true,
        docs: "https://code.visualstudio.com/docs/copilot/customization/mcp-servers",
    },
    Client {
        name: "codex",
        title: "Codex CLI",
        project: true,
        docs: "https://developers.openai.com/codex/mcp",
    },
    Client {
        name: "gemini",
        title: "Gemini CLI",
        project: true,
        docs: "https://geminicli.com/docs/tools/mcp-server/",
    },
];

/// How a file holds its servers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    /// `mcpServers.argus = {command, args}` (Claude Code adds type).
    Servers { typed: bool },
    /// VS Code: `servers.argus = {type, command, args}`.
    VsCode,
    /// Gemini CLI: `mcpServers.argus = {command, args, timeout}`.
    Gemini,
    /// Codex CLI: a `[mcp_servers.argus]` table in TOML.
    Codex,
}

struct Target {
    file: PathBuf,
    shape: Shape,
}

struct Homes {
    home: PathBuf,
    /// Windows' roaming application data; a Mac's `~/Library/Application
    /// Support`; elsewhere `~/.config`.
    config: PathBuf,
    /// Windows' local application data.
    local: PathBuf,
    /// Whether these are a test's (no client CLI is run then).
    test: bool,
}

fn homes() -> Result<Homes, String> {
    let env = |k: &str| {
        std::env::var(k)
            .ok()
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(h) = env("ARGUS_SETUP_HOME") {
        let (config, local) = if cfg!(windows) {
            (h.join("AppData/Roaming"), h.join("AppData/Local"))
        } else if cfg!(target_os = "macos") {
            (
                h.join("Library/Application Support"),
                h.join("Library/Application Support"),
            )
        } else {
            (h.join(".config"), h.join(".local/share"))
        };
        return Ok(Homes {
            home: h,
            config,
            local,
            test: true,
        });
    }
    let home = env(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .or_else(|| env("HOME"))
        .ok_or("no home folder (USERPROFILE or HOME)")?;
    let (config, local) = if cfg!(windows) {
        (
            env("APPDATA").unwrap_or_else(|| home.join("AppData/Roaming")),
            env("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData/Local")),
        )
    } else if cfg!(target_os = "macos") {
        // Claude Desktop and VS Code keep their settings here on a Mac,
        // XDG or not.
        let support = home.join("Library/Application Support");
        (support.clone(), support)
    } else {
        (
            env("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config")),
            home.join(".local/share"),
        )
    };
    Ok(Homes {
        home,
        config,
        local,
        test: false,
    })
}

/// Claude Desktop's config files: the documented one, and on Windows the
/// copy the Store (MSIX) app reads inside its package, for each package.
fn desktop_files(h: &Homes) -> Vec<PathBuf> {
    let mut out = vec![h.config.join("Claude").join("claude_desktop_config.json")];
    if cfg!(windows) {
        if let Ok(rd) = std::fs::read_dir(h.local.join("Packages")) {
            let mut pkgs: Vec<PathBuf> = rd
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("Claude_"))
                .map(|e| {
                    e.path()
                        .join("LocalCache/Roaming/Claude/claude_desktop_config.json")
                })
                .collect();
            pkgs.sort();
            out.extend(pkgs);
        }
    }
    out
}

fn targets(client: &str, user: bool, root: Option<&Path>, h: &Homes) -> Vec<Target> {
    let at = |p: PathBuf, shape| vec![Target { file: p, shape }];
    let r = |rel: &str| root.map(|r| r.join(rel)).unwrap_or_default();
    match (client, user) {
        ("claude-code", false) => at(r(".mcp.json"), Shape::Servers { typed: true }),
        ("claude-code", true) => at(h.home.join(".claude.json"), Shape::Servers { typed: true }),
        ("claude-desktop", _) => desktop_files(h)
            .into_iter()
            .map(|file| Target {
                file,
                shape: Shape::Servers { typed: false },
            })
            .collect(),
        ("cursor", false) => at(r(".cursor/mcp.json"), Shape::Servers { typed: true }),
        ("cursor", true) => at(
            h.home.join(".cursor/mcp.json"),
            Shape::Servers { typed: true },
        ),
        ("vscode", false) => at(r(".vscode/mcp.json"), Shape::VsCode),
        ("vscode", true) => at(h.config.join("Code/User/mcp.json"), Shape::VsCode),
        ("codex", false) => at(r(".codex/config.toml"), Shape::Codex),
        ("codex", true) => {
            let dir = std::env::var("CODEX_HOME")
                .ok()
                .filter(|d| !d.is_empty() && !h.test)
                .map(PathBuf::from)
                .unwrap_or_else(|| h.home.join(".codex"));
            at(dir.join("config.toml"), Shape::Codex)
        }
        ("gemini", false) => at(r(".gemini/settings.json"), Shape::Gemini),
        ("gemini", true) => at(h.home.join(".gemini/settings.json"), Shape::Gemini),
        _ => Vec::new(),
    }
}

/// Whether a client looks installed here: its folder is there.
fn found(client: &str, h: &Homes) -> bool {
    let p = match client {
        "claude-code" => h.home.join(".claude"),
        "claude-desktop" => {
            return desktop_files(h)
                .iter()
                .any(|f| f.parent().is_some_and(Path::is_dir))
        }
        "cursor" => h.home.join(".cursor"),
        "vscode" => h.config.join("Code"),
        "codex" => h.home.join(".codex"),
        "gemini" => h.home.join(".gemini"),
        _ => return false,
    };
    p.is_dir()
}

/// The entry's fields for a file's shape.
fn fields(shape: Shape, command: &str, args: &[String]) -> Vec<(&'static str, Json)> {
    let mut f = Vec::new();
    if matches!(shape, Shape::Servers { typed: true } | Shape::VsCode) {
        f.push(("type", json!("stdio")));
    }
    f.push(("command", json!(command)));
    f.push(("args", json!(args)));
    if shape == Shape::Gemini {
        // Opening a project starts its daemon, which can take a while the
        // first time (the import cache).
        f.push(("timeout", json!(120_000)));
    }
    f
}

/// A TOML string: a literal one ('...') unless the text has a quote.
fn toml_str(s: &str) -> String {
    if s.contains('\'') || s.contains('\n') {
        Json::from(s).to_string()
    } else {
        format!("'{s}'")
    }
}

/// Codex's table for the entry.
fn codex_table(command: &str, args: &[String]) -> String {
    let args: Vec<String> = args.iter().map(|a| toml_str(a)).collect();
    format!(
        "[mcp_servers.{NAME}]\ncommand = {}\nargs = [{}]\n# Opening a project starts its daemon, which can take a while the first time.\nstartup_timeout_sec = 120\n",
        toml_str(command),
        args.join(", ")
    )
}

/// `text` with the `[mcp_servers.argus]` table replaced by `table`, or
/// added at the end; the rest of the file as it was.
fn codex_set(text: &str, table: &str) -> String {
    let header = format!("[mcp_servers.{NAME}]");
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let Some(start) = lines.iter().position(|l| l.trim() == header) else {
        let mut out = text.to_string();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(table);
        return out;
    };
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map_or(lines.len(), |n| start + 1 + n);
    // Keep the blank lines between this table and the next.
    let mut keep = end;
    while keep > start + 1 && lines[keep - 1].trim().is_empty() {
        keep -= 1;
    }
    let mut out: String = lines[..start].concat();
    out.push_str(table);
    out.push_str(&lines[keep..].concat());
    out
}

/// The `[mcp_servers.argus]` table's text in `text`, if it is there.
fn codex_get(text: &str) -> Option<String> {
    let header = format!("[mcp_servers.{NAME}]");
    let start = text
        .split_inclusive('\n')
        .position(|l| l.trim() == header)?;
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map_or(lines.len(), |n| start + 1 + n);
    Some(lines[start..end].concat().trim_end().to_string())
}

/// Put the entry in one file. The file's report: added, updated, or
/// unchanged, and its backup.
fn write(t: &Target, command: &str, args: &[String]) -> Result<Json, String> {
    let file = crate::build::plain(&t.file);
    let old = match std::fs::read_to_string(&t.file) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("{file}: {e}")),
    };
    let text = old.clone().unwrap_or_default();
    let (had, new) = match t.shape {
        Shape::Codex => {
            let table = codex_table(command, args);
            let had = codex_get(&text);
            if had.as_deref() == Some(table.trim_end()) {
                return Ok(json!({ "file": file, "status": "unchanged" }));
            }
            (had.is_some(), codex_set(&text, &table))
        }
        shape => {
            let key = if shape == Shape::VsCode {
                "servers"
            } else {
                "mcpServers"
            };
            let f = fields(shape, command, args);
            let want = Json::Object(f.iter().map(|(k, v)| (k.to_string(), v.clone())).collect());
            let had = jsonedit::get(&text, key, NAME).map_err(|e| {
                format!("{file} is not a JSON object this can edit ({e}); add the entry by hand: \"{NAME}\": {want}")
            })?;
            if had.as_ref() == Some(&want) {
                return Ok(json!({ "file": file, "status": "unchanged" }));
            }
            let new = jsonedit::set(&text, key, NAME, &f).map_err(|e| format!("{file}: {e}"))?;
            // A file that was JSON stays JSON, with the entry in it.
            if serde_json::from_str::<Json>(&text).is_ok() || text.trim().is_empty() {
                let v: Json = serde_json::from_str(&new).map_err(|e| {
                    format!("{file}: the edit would not be JSON ({e}); nothing written")
                })?;
                if v[key][NAME] != want {
                    return Err(format!("{file}: the edit did not take; nothing written"));
                }
            }
            (had.is_some(), new)
        }
    };
    let mut report = json!({ "file": file, "status": if had { "updated" } else { "added" } });
    if let Some(old) = &old {
        let backup = t.file.with_file_name(format!(
            "{}.argus-backup",
            t.file.file_name().unwrap_or_default().to_string_lossy()
        ));
        std::fs::write(&backup, old).map_err(|e| format!("{}: {e}", backup.display()))?;
        report["backup"] = json!(crate::build::plain(&backup));
    }
    if let Some(d) = t.file.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    std::fs::write(&t.file, new).map_err(|e| format!("{file}: {e}"))?;
    Ok(report)
}

/// Claude Code's user scope through its own CLI, which keeps
/// `~/.claude.json` (a file it rewrites while it runs). None when there is
/// no `claude` to run.
fn claude_cli(command: &str, args: &[String], h: &Homes) -> Option<Result<Json, String>> {
    if h.test {
        return None;
    }
    let file = h.home.join(".claude.json");
    let want = Json::Object(
        fields(Shape::Servers { typed: true }, command, args)
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    );
    let have = std::fs::read_to_string(&file)
        .ok()
        .and_then(|t| jsonedit::get(&t, "mcpServers", NAME).ok().flatten());
    let ok = Command::new("claude").arg("--version").output().ok()?;
    if !ok.status.success() {
        return None;
    }
    let plain = crate::build::plain(&file);
    if have.as_ref() == Some(&want) {
        return Some(Ok(
            json!({ "file": plain, "status": "unchanged", "by": "claude mcp" }),
        ));
    }
    if have.is_some() {
        let _ = Command::new("claude")
            .args(["mcp", "remove", "--scope", "user", NAME])
            .output();
    }
    let o = Command::new("claude")
        .args(["mcp", "add-json", "--scope", "user", NAME])
        .arg(want.to_string())
        .output();
    Some(match o {
        Ok(o) if o.status.success() => Ok(json!({
            "file": plain,
            "status": if have.is_some() { "updated" } else { "added" },
            "by": "claude mcp add-json --scope user",
        })),
        Ok(o) => Err(format!(
            "claude mcp add-json: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(e) => Err(format!("claude mcp add-json: {e}")),
    })
}

/// What to do next in each client.
fn next(client: &str, user: bool) -> &'static str {
    match (client, user) {
        ("claude-code", false) => "Start claude in the project; it asks once to use the project's .mcp.json servers.",
        ("claude-code", true) => "Every claude session has it; with no project open, ask the agent to list the projects.",
        ("claude-desktop", _) => "Quit Claude Desktop (from the tray too) and start it again; the argus tools are in the chat's tools menu.",
        ("cursor", _) => "Cursor lists it under Settings, MCP; turn it on there if it asks.",
        ("vscode", _) => "In VS Code, trust the folder, then start argus from the MCP servers view or the chat's tools menu.",
        ("codex", false) => "Codex reads a project's .codex/config.toml only when the project is trusted.",
        ("codex", true) => "Every codex session has it; with no project open, ask the agent to list the projects.",
        ("gemini", false) => "Gemini CLI connects a project's servers only in a trusted folder (/permissions).",
        _ => "Every gemini session has it; with no project open, ask the agent to list the projects.",
    }
}

/// `argus mcp setup [client] [--scope project|user] [--project dir]`.
pub fn command(
    inv: &Invocation,
    words: &[String],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    match setup(inv, words) {
        Ok(r) => {
            if inv.json {
                let _ = writeln!(out, "{r}");
            } else {
                let _ = write!(out, "{}", summary(&r));
            }
            crate::EXIT_OK
        }
        Err(e) => {
            let _ = writeln!(err, "argus mcp setup: {e}");
            crate::EXIT_USAGE
        }
    }
}

fn summary(r: &Json) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    if let Some(list) = r["clients"].as_array() {
        let _ = writeln!(
            s,
            "usage: argus mcp setup <client> [--scope project|user] [--project dir]\n\nclients:"
        );
        for c in list {
            let _ = writeln!(
                s,
                "  {:<15} {:<15} {}",
                c["name"].as_str().unwrap_or_default(),
                c["title"].as_str().unwrap_or_default(),
                if c["found"] == true { "found here" } else { "" }
            );
        }
        return s;
    }
    let _ = writeln!(
        s,
        "{} ({} scope): argus mcp {}",
        r["title"].as_str().unwrap_or_default(),
        r["scope"].as_str().unwrap_or_default(),
        r["args"]
            .as_array()
            .map(|a| a
                .iter()
                .skip(1)
                .filter_map(Json::as_str)
                .collect::<Vec<_>>()
                .join(" "))
            .unwrap_or_default()
    );
    for f in r["files"].as_array().into_iter().flatten() {
        let _ = writeln!(
            s,
            "  {:<9} {}{}",
            f["status"].as_str().unwrap_or_default(),
            f["file"].as_str().unwrap_or_default(),
            f["backup"]
                .as_str()
                .map(|b| format!("  (was: {b})"))
                .unwrap_or_default()
        );
    }
    let _ = writeln!(s, "{}", r["next"].as_str().unwrap_or_default());
    s
}

fn setup(inv: &Invocation, words: &[String]) -> Result<Json, String> {
    let h = homes()?;
    let Some(name) = words.first() else {
        return Ok(json!({
            "clients": CLIENTS.iter().map(|c| json!({
                "name": c.name, "title": c.title, "project": c.project, "found": found(c.name, &h), "docs": c.docs,
            })).collect::<Vec<_>>(),
        }));
    };
    let client = CLIENTS.iter().find(|c| c.name == name).ok_or_else(|| {
        format!(
            "no client {name:?}; there are: {}",
            CLIENTS
                .iter()
                .map(|c| c.name)
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    if words.len() > 1 {
        return Err(format!("one client at a time; got {}", words.join(" ")));
    }
    let root = crate::project::find_project(inv.project.as_deref())
        .ok()
        .map(|r| r.canonicalize().unwrap_or(r));
    let user = match inv.scope.as_deref() {
        Some("user") => true,
        Some("project") if !client.project => {
            return Err(format!(
                "{} has no project file; its scope is user",
                client.title
            ))
        }
        Some("project") => false,
        Some(s) => return Err(format!("no scope {s:?}: project or user")),
        None => !client.project || root.is_none(),
    };
    if !user && root.is_none() {
        return Err(
            "a project scope needs a project: run this in one, or give --project <dir>, or --scope user"
                .to_string(),
        );
    }
    // A user entry opens a project only when one is named.
    let pin = if user {
        inv.project.as_ref().and(root.as_ref())
    } else {
        root.as_ref()
    };
    let command = std::env::current_exe()
        .map(|e| crate::build::plain(&e.canonicalize().unwrap_or(e)))
        .map_err(|e| format!("this program's path: {e}"))?;
    let mut args = vec!["mcp".to_string()];
    if let Some(r) = pin {
        args.push("--project".to_string());
        args.push(crate::build::plain(r));
    }
    let mut files = Vec::new();
    match (client.name, user) {
        ("claude-code", true) => match claude_cli(&command, &args, &h) {
            Some(r) => files.push(r?),
            None => {
                for t in targets(client.name, true, None, &h) {
                    files.push(write(&t, &command, &args)?);
                }
            }
        },
        _ => {
            for t in targets(client.name, user, root.as_deref(), &h) {
                files.push(write(&t, &command, &args)?);
            }
        }
    }
    // Gemini reads GEMINI.md, which can import the AGENTS.md argus new wrote.
    if client.name == "gemini" && !user {
        let r = root.as_ref().expect("a project scope has a root");
        if r.join("AGENTS.md").is_file() && !r.join("GEMINI.md").exists() {
            let g = r.join("GEMINI.md");
            std::fs::write(&g, "@AGENTS.md\n").map_err(|e| format!("GEMINI.md: {e}"))?;
            files.push(json!({ "file": crate::build::plain(&g), "status": "added" }));
        }
    }
    Ok(json!({
        "client": client.name,
        "title": client.title,
        "scope": if user { "user" } else { "project" },
        "command": command,
        "args": args,
        "files": files,
        "next": next(client.name, user),
        "docs": client.docs,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_codex_table_is_replaced_in_place_and_the_rest_kept() {
        let text = "model = 'o4'\n\n[mcp_servers.argus]\ncommand = 'old'\n\n[mcp_servers.other]\ncommand = 'x'\n";
        let t = codex_table("argus", &["mcp".to_string()]);
        let new = codex_set(text, &t);
        assert!(
            new.starts_with("model = 'o4'\n\n[mcp_servers.argus]\ncommand = 'argus'\n"),
            "{new}"
        );
        assert!(
            new.ends_with("\n\n[mcp_servers.other]\ncommand = 'x'\n"),
            "{new}"
        );
        assert_eq!(codex_get(&new).unwrap(), t.trim_end());
        assert_eq!(codex_set(&new, &t), new, "idempotent");
        let added = codex_set("model = 'o4'", &t);
        assert_eq!(added, format!("model = 'o4'\n\n{t}"));
    }

    #[test]
    fn a_windows_path_is_a_literal_toml_string() {
        assert_eq!(toml_str("C:/a b"), "'C:/a b'");
        assert_eq!(toml_str("it's"), "\"it's\"");
    }
}
