//! `argus new` (distribution item 2): a working project from a template,
//! ready for a person or an agent to start in.
//!
//! The template is copied, named, and run: its scenarios must pass before
//! `argus new` says done. Then the project's ARGUS.md is written from its
//! own daemon (run in this process, so none is left running), with an
//! `AGENTS.md` and a `CLAUDE.md` that point at it and at `argus docs`, so
//! any agent knows where to read first.
//!
//! Templates are directories under the install's `templates/` (beside
//! `bin/`, or `$ARGUS_TEMPLATES`), else the engine checkout's. Each has a
//! `template.toml` saying what it is (`about = "..."`), which is not copied.

use crate::tools::Caller;
use crate::Invocation;
use serde_json::{json, Value as Json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What each template says about itself.
const ABOUT: &str = "template.toml";

/// The template `argus new` uses when none is named.
pub const DEFAULT: &str = "blank";

/// Where the templates are: `$ARGUS_TEMPLATES`, else `templates/` beside
/// the directory this program is in (an install), else the checkout's.
pub fn root() -> Result<PathBuf, String> {
    if let Ok(d) = std::env::var("ARGUS_TEMPLATES") {
        if !d.is_empty() {
            return Ok(PathBuf::from(d));
        }
    }
    if let Some(d) = crate::exe()
        .ok()
        .and_then(|e| Some(e.parent()?.parent()?.join("templates")))
        .filter(|d| d.is_dir())
    {
        return Ok(d);
    }
    let d = crate::build::engine_dir()?.join("templates");
    if d.is_dir() {
        Ok(d)
    } else {
        Err(format!(
            "no templates: neither an install's templates/ nor {}",
            crate::build::plain(&d)
        ))
    }
}

/// The templates there, by name, with what each is.
pub fn list(root: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(root) else {
        return out;
    };
    for e in rd.flatten() {
        let about = e.path().join(ABOUT);
        let Ok(text) = std::fs::read_to_string(&about) else {
            continue;
        };
        let what = text
            .lines()
            .find_map(|l| l.trim().strip_prefix("about = "))
            .map(|v| v.trim().trim_matches('"').to_string())
            .unwrap_or_default();
        out.push((e.file_name().to_string_lossy().into_owned(), what));
    }
    out.sort();
    out
}

/// A daemon in this process, as the tools call one.
struct Local(argus_daemon::Daemon);

impl Caller for Local {
    fn call(&mut self, method: &str, params: Json) -> Result<Json, Json> {
        self.0
            .call(0, method, &params)
            .map_err(|e| json!({ "code": e.code, "message": e.message, "hint": e.hint }))
    }
}

/// `argus new <dir> [--template t] [--name n] [--no-git]`.
pub fn new_command(
    inv: &Invocation,
    words: &[String],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    match new_project(inv, words, err) {
        Ok(report) => {
            if inv.json {
                let _ = writeln!(out, "{report}");
            } else {
                let _ = write!(out, "{}", summary(&report));
            }
            crate::EXIT_OK
        }
        Err(e) => {
            let _ = writeln!(err, "argus new: {e}");
            crate::EXIT_USAGE
        }
    }
}

fn summary(r: &Json) -> String {
    let dir = r["dir"].as_str().unwrap_or_default();
    format!(
        "made {} from the {} template in {dir}: {} of {} scenario runs pass\n\n  cd {dir}\n  argus test                    the scenarios in tests/\n  argus editor                  the editor window\n  argus mcp setup <client>      connect an agent (claude-code, cursor, vscode, codex, gemini, claude-desktop)\n\nAn agent starts with AGENTS.md (CLAUDE.md for Claude), which points at ARGUS.md.\n",
        r["name"].as_str().unwrap_or_default(),
        r["template"].as_str().unwrap_or_default(),
        r["tests"]["passed"],
        r["tests"]["total"],
    )
}

fn new_project(inv: &Invocation, words: &[String], err: &mut dyn Write) -> Result<Json, String> {
    let [dir] = words else {
        let names = list(&root()?);
        return Err(format!(
            "usage: argus new <dir> [--template {}] [--name name] [--no-git]",
            names
                .iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>()
                .join("|")
        ));
    };
    make(
        Path::new(dir),
        inv.template.as_deref(),
        inv.name.as_deref(),
        !inv.no_git,
        err,
    )
}

/// Make a project in `dir` from `template` (default blank), named `name`
/// (default the directory's), a git repository when `git` and git is
/// here; its scenarios must pass. The report says what was made.
pub fn make(
    dir: &Path,
    template: Option<&str>,
    name: Option<&str>,
    git: bool,
    err: &mut dyn Write,
) -> Result<Json, String> {
    let templates = root()?;
    let names = list(&templates);
    let template = template.unwrap_or(DEFAULT);
    if !names.iter().any(|(n, _)| n == template) {
        return Err(format!(
            "no template {template:?}; there are: {}",
            names
                .iter()
                .map(|(n, w)| format!("{n} ({w})"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let dir = dir.to_path_buf();
    if dir.exists()
        && std::fs::read_dir(&dir)
            .map(|mut d| d.next().is_some())
            .unwrap_or(true)
    {
        return Err(format!(
            "{} is there already and not empty; a new project goes in a new or empty directory",
            dir.display()
        ));
    }
    let name = match name {
        Some(n) => n.to_string(),
        None => dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| format!("{} names no directory", dir.display()))?,
    };
    if name.trim().is_empty() || name.contains('"') || name.contains('\n') {
        return Err(format!("{name:?} cannot be a game's name"));
    }

    let mut files = Vec::new();
    copy(&templates.join(template), &dir, Path::new(""), &mut files)?;
    rename(&dir.join("argus.toml"), &name)?;
    let dir = dir.canonicalize().unwrap_or(dir);
    let git = git && git_init(&dir);

    // The template's scenarios, from its own daemon: a new project starts
    // from one that works.
    let mut d = Local(argus_daemon::Daemon::new());
    let opened = d
        .call(
            "session.open",
            json!({ "project": crate::build::plain(&dir) }),
        )
        .map_err(|e| format!("the new project does not open: {}", e["message"]))?;
    if opened["script_errors"].as_u64().unwrap_or(0) > 0 {
        return Err(format!(
            "the {template} template has script errors: {opened}"
        ));
    }
    let tests = d
        .call("test.run", json!({ "replays": false }))
        .map_err(|e| format!("its scenarios do not run: {}", e["message"]))?;
    if tests["ok"] != json!(true) {
        let _ = write!(err, "{}", crate::render::test_report(&tests));
        return Err(format!(
            "the {template} template's scenarios do not pass in {}",
            crate::build::plain(&dir)
        ));
    }

    // Where an agent reads first.
    let md =
        crate::agentdoc::for_agent(&mut d).map_err(|e| format!("ARGUS.md: {}", e["message"]))?;
    std::fs::write(dir.join("ARGUS.md"), md).map_err(|e| format!("ARGUS.md: {e}"))?;
    std::fs::write(dir.join("AGENTS.md"), agents_md(&name))
        .map_err(|e| format!("AGENTS.md: {e}"))?;
    std::fs::write(dir.join("CLAUDE.md"), claude_md()).map_err(|e| format!("CLAUDE.md: {e}"))?;
    let _ = d.call("session.close", json!({}));
    files.extend(["ARGUS.md", "AGENTS.md", "CLAUDE.md"].map(String::from));
    crate::projects::remember(&dir);

    Ok(json!({
        "dir": crate::build::plain(&dir),
        "name": name,
        "template": template,
        "files": files,
        "git": git,
        "tests": runs(&tests),
    }))
}

/// A test.run report's runs, passed and in all.
fn runs(report: &Json) -> Json {
    let seeds: Vec<&Json> = report["scenarios"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|s| s["seeds"].as_array().into_iter().flatten())
        .collect();
    let passed = seeds.iter().filter(|s| s["ok"] == json!(true)).count();
    json!({ "passed": passed, "total": seeds.len() })
}

/// Copy a template, all but its `template.toml`.
fn copy(from: &Path, to: &Path, rel: &Path, files: &mut Vec<String>) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for e in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        let name = e.file_name();
        if rel.as_os_str().is_empty() && name == ABOUT {
            continue;
        }
        let r = rel.join(&name);
        if e.path().is_dir() {
            copy(&e.path(), &to.join(&name), &r, files)?;
        } else {
            std::fs::copy(e.path(), to.join(&name))
                .map_err(|err| format!("{}: {err}", e.path().display()))?;
            files.push(r.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

/// Name the game: `argus.toml`'s `name` line.
fn rename(toml: &Path, name: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(toml).map_err(|e| format!("{}: {e}", toml.display()))?;
    let mut done = false;
    let lines: Vec<String> = text
        .lines()
        .map(|l| {
            if !done && l.trim_start().starts_with("name =") {
                done = true;
                format!("name = \"{name}\"")
            } else {
                l.to_string()
            }
        })
        .collect();
    if !done {
        return Err(format!("{} has no name line", toml.display()));
    }
    std::fs::write(toml, lines.join("\n") + "\n").map_err(|e| e.to_string())
}

/// A repository for the project, when git is here and the project is not
/// inside one already: a build's code revision is its commit.
fn git_init(dir: &Path) -> bool {
    if !argus_daemon::daemon::git_usable() {
        return false;
    }
    let inside = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if inside {
        return false;
    }
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["init", "-q"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// What any agent reads first in a new project.
pub fn agents_md(name: &str) -> String {
    format!(
        "# Working on {name}\n\n\
This is a game made with Argus. The game runs in the Argus daemon, which holds its world: read and change it \
through the Argus tools (the MCP server `argus mcp`) or the `argus` command line, rather than guessing from the files.\n\n\
- Read ARGUS.md first. It is generated from the live game: the components, systems, prefabs, and scenes, \
with the scripting (Luau) and scenario APIs. Regenerate it with `argus docs --for-agent` after changing \
components, systems, prefabs, or scenes.\n\
- The manual says how to do things, the reference lists everything: `argus docs list` lists both (`argus docs agents` \
first, then the chapter for the task: scripting, scenes, gameplay, presentation, testing, ...), or the `docs` tool \
over MCP (`docs topic=agents`). `argus docs find=<word>` searches them.\n\
- `components/` declares the game's components and `systems/` its systems, in Luau, with the code they share \
in modules in `lib/` (`require(\"rules\")`); `scenes/<scene>/` holds one TOML file per entity; `input.toml` the \
actions and their bindings.\n\
- `argus test` runs the scenarios in `tests/`. A change is done when they pass; add a scenario for each \
feature, so it stays working.\n\
- `argus build` makes a release (the executable and its packs, or `--target web` for the page), and checks \
that it plays as the project does.\n"
    )
}

/// Claude Code reads CLAUDE.md; it imports AGENTS.md, so there is one text.
pub fn claude_md() -> String {
    "@AGENTS.md\n".to_string()
}
