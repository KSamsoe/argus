//! `argus docs` (§16, phase 2 item 8).
//!
//! `argus docs --for-agent` writes the project's ARGUS.md from the live
//! daemon: how to work with the project, its scenes, the component schemas
//! with their doc strings, the systems with their stages and write sets, and
//! the prefab catalog. It is regenerated on demand, so it never drifts from
//! the project, and it is what a coding agent's project instructions point
//! at. `argus docs` alone prints the engine's reference, the tools and every
//! method, in the same shape and from the same table.

use crate::tools::{self, Caller};
use crate::{project, Invocation};
use argus_daemon::docs;
use serde_json::{json, Value as Json};
use std::fmt::Write as _;
use std::io::Write;

/// `argus docs [--for-agent] [--out file]`.
pub fn command(inv: &Invocation, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if !inv.for_agent {
        // `argus docs <topic> [section=..] [find=..]`: what the docs tool
        // answers; `argus docs` alone, the whole reference.
        let arg = |k: &str| {
            inv.args
                .iter()
                .find_map(|a| a.strip_prefix(&format!("{k}=")).map(str::to_string))
        };
        let topic = inv
            .words
            .get(1)
            .cloned()
            .or_else(|| arg("topic"))
            // `argus docs list`: the topics, as the docs tool gives them.
            .filter(|t| t != "list");
        let listed = inv.words.get(1).is_some_and(|t| t == "list");
        let (section, find) = (arg("section"), arg("find"));
        if topic.is_none() && section.is_none() && find.is_none() && !listed {
            let _ = write!(out, "{}", reference());
            return crate::EXIT_OK;
        }
        let project = (topic.as_deref() == Some("project")).then(|| {
            project::connect(inv, err)
                .map_err(|e| e.to_string())
                .and_then(|mut c| for_agent(&mut c).map_err(|e| e["message"].to_string()))
        });
        return match crate::manual::answer(
            topic.as_deref(),
            section.as_deref(),
            find.as_deref(),
            project,
        ) {
            Ok(r) if inv.json => {
                let _ = writeln!(out, "{r}");
                crate::EXIT_OK
            }
            Ok(r) => {
                let _ = write!(out, "{}", docs_text(&r));
                crate::EXIT_OK
            }
            Err(e) => {
                let _ = writeln!(err, "argus docs: {e}");
                crate::EXIT_USAGE
            }
        };
    }
    let root = match project::find_project(inv.project.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(err, "argus: {e}");
            return crate::EXIT_USAGE;
        }
    };
    let mut client = match project::connect(inv, err) {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(err, "argus: {e}");
            return crate::EXIT_CONNECT;
        }
    };
    let md = match for_agent(&mut client) {
        Ok(m) => m,
        Err(e) => {
            let _ = write!(err, "{}", crate::render::error(&e));
            return crate::EXIT_RPC;
        }
    };
    let path = inv.out.clone().unwrap_or_else(|| root.join("ARGUS.md"));
    if let Err(e) = std::fs::write(&path, &md) {
        let _ = writeln!(err, "argus: {}: {e}", path.display());
        return crate::EXIT_RPC;
    }
    let _ = writeln!(
        out,
        "wrote {} ({} lines)",
        path.to_string_lossy().trim_start_matches(r"\\?\"),
        md.lines().count()
    );
    crate::EXIT_OK
}

/// A markdown table cell: no pipes or newlines.
fn cell(v: &Json) -> String {
    let s = match v {
        Json::Null => String::new(),
        Json::String(s) => s.clone(),
        Json::Array(a) => a
            .iter()
            .map(|x| x.as_str().map_or_else(|| x.to_string(), str::to_string))
            .collect::<Vec<_>>()
            .join(", "),
        other => other.to_string(),
    };
    s.replace('|', "\\|").replace(['\n', '\r'], " ")
}

fn rows(v: &Json) -> Vec<Json> {
    v.get("rows")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default()
}

/// Declared by the engine (a Rust crate), not by the game's scripts.
fn engine_source(src: &str) -> bool {
    src.starts_with("crates") || src.starts_with("argus-") || src.contains("argus-")
}

/// The project's ARGUS.md, from the daemon `c` talks to.
pub fn for_agent(c: &mut dyn Caller) -> Result<String, Json> {
    let status = c.call("session.status", json!({}))?;
    let summary = tools::run(c, "summary", &json!({}))?;
    let scenes = c.call("scene.list", json!({}))?;
    let components = c.call("world.describe", json!({ "limit": 1000 }))?;
    let systems = c.call("script.systems", json!({ "limit": 1000 }))?;
    let prefabs = c.call("world.prefabs", json!({ "limit": 1000 }))?;
    let defs = c.call("world.defs", json!({}))?;
    let project = status["project"].as_str().unwrap_or("this project");

    let mut o = String::new();
    let _ = writeln!(o, "# ARGUS.md: {project}\n");
    let _ = writeln!(
        o,
        "Generated by `argus docs --for-agent` from the live daemon (Argus {} {}; scene `{}`, seed {}, clock `{}`). \
Regenerate it after changing components, systems, prefabs, or scenes; do not edit it by hand.\n",
        cell(&status["engine"]["version"]),
        cell(&status["engine"]["revision"]),
        cell(&status["scene"]),
        cell(&status["seed"]),
        cell(&status["clock"])
    );

    o.push_str("## Working with this project\n\n");
    o.push_str(
        "The game runs in the Argus daemon, which holds its world. Read and change it through the Argus tools \
(an MCP server: `argus mcp --project <dir>`) or the same tools as shell commands, `argus <tool> key=value` \
(`--params '<json>'` for the same JSON an MCP call takes). Every protocol method is also a command, \
`argus <namespace> <method>`; `argus help` lists them all.\n\n",
    );
    o.push_str("| tool | what it does |\n|---|---|\n");
    for t in docs::TOOLS {
        let first = t
            .doc
            .split(". ")
            .next()
            .unwrap_or(t.doc)
            .trim_end_matches('.');
        let _ = writeln!(o, "| `{}` | {}. |", t.name, cell(&json!(first)));
    }
    o.push_str(
        "\nChanges to files: scripts are Luau in `components/`, `defs/`, and `systems/`; entities are TOML under \
`scenes/`; assets and their sidecars are in `assets/`. `reload` takes scripts and assets in while the game runs; an entity file changed on disk is taken in when the scene loads again (`scene load`). \
Change entities with `edit`, so the change feed records who and why and undo works.\n\n\
A hot path can be Rust instead: a crate in `plugins/<name>/` that `argus.toml`'s `plugins` lists, declaring its \
systems as Luau does (`System::new(name, stage).after(..).reads(&[..]).writes(&[..]).run(f)`, from the \
`argus-plugin` crate the engine writes to `.argus/plugins/sdk`) and its components as `component!` structs, \
field for field as the game declares them. `reload` builds and swaps it in; a compile error is an `errors` row \
with its file and line, and the last good build keeps running. The SDK is the plugin's API and documents itself: \
`cargo doc --open --manifest-path .argus/plugins/sdk/Cargo.toml` (it has no dependencies, so this works offline).\n\n\
The scripting API (what `components/`, `systems/`, and `defs/` declare and call), the scenario API (what `tests/` \
writes), and every file a project is made of are at the end of this file.\n\n",
    );

    o.push_str("## Scenes\n\n");
    let current = scenes["current"].as_str().unwrap_or("");
    for s in scenes["scenes"].as_array().cloned().unwrap_or_default() {
        let name = s.as_str().unwrap_or_default();
        let sum = if name == current {
            Ok(summary.clone())
        } else {
            c.call("world.summary", json!({ "scene": name, "source": "index" }))
        };
        let _ = writeln!(
            o,
            "### {name}{}\n",
            if name == current { " (loaded)" } else { "" }
        );
        match sum {
            Err(e) => {
                let _ = writeln!(o, "Not indexed: {}\n", cell(&e["message"]));
            }
            Ok(s) => {
                let _ = writeln!(
                    o,
                    "Clock `{}`, seed {}, {} entities ({} authored){}.\n",
                    cell(&s["clock"]),
                    cell(&s["seed"]),
                    cell(&s["entities"]),
                    cell(&s["authored"]),
                    {
                        // A scene without rooms lists one, unnamed: say none.
                        let rooms: Vec<&str> = s["rooms"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Json::as_str)
                            .filter(|r| !r.is_empty())
                            .collect();
                        if rooms.is_empty() {
                            String::new()
                        } else {
                            format!(", rooms {}", rooms.join(", "))
                        }
                    }
                );
                let named = s["named"].as_array().cloned().unwrap_or_default();
                if !named.is_empty() {
                    o.push_str("| entity | components |\n|---|---|\n");
                    for n in &named {
                        let _ = writeln!(o, "| `{}` | {} |", cell(&n[0]), cell(&n[1]));
                    }
                    if s["named_total"].as_u64().unwrap_or(0) > named.len() as u64 {
                        let _ = writeln!(
                            o,
                            "\n{} named entities in all; `query` lists the rest.",
                            cell(&s["named_total"])
                        );
                    }
                    o.push('\n');
                }
            }
        }
    }

    o.push_str("## Systems, in run order\n\n");
    o.push_str("| system | stage | reads | writes | what it does |\n|---|---|---|---|---|\n");
    for s in rows(&systems) {
        let off = if s["enabled"] == false { " (off)" } else { "" };
        let _ = writeln!(
            o,
            "| `{}`{off} | {} | {} | {} | {} |",
            cell(&s["name"]),
            cell(&s["stage"]),
            cell(&s["reads"]),
            cell(&s["writes"]),
            cell(&s["doc"])
        );
    }
    o.push('\n');

    let prefab_rows = rows(&prefabs);
    o.push_str("## Prefabs\n\n");
    if prefab_rows.is_empty() {
        o.push_str("None.\n\n");
    } else {
        o.push_str("| prefab | extends | components | tags | instances |\n|---|---|---|---|---|\n");
        for p in prefab_rows {
            let _ = writeln!(
                o,
                "| `{}` | {} | {} | {} | {} |",
                cell(&p["path"]),
                cell(&p["base"]),
                cell(&p["components"]),
                cell(&p["tags"]),
                cell(&p["instances"])
            );
        }
        o.push('\n');
    }

    let tables = defs["tables"].as_array().cloned().unwrap_or_default();
    if !tables.is_empty() {
        o.push_str("## Def tables\n\n");
        for t in tables {
            let _ = writeln!(o, "- {}", cell(&t));
        }
        o.push('\n');
    }

    let all = rows(&components);
    let (game, engine): (Vec<Json>, Vec<Json>) = all
        .into_iter()
        .partition(|c| !engine_source(c["source"].as_str().unwrap_or("")));
    for (title, list) in [
        ("## The game's components", game),
        ("## The engine's components", engine),
    ] {
        let _ = writeln!(o, "{title}\n");
        if list.is_empty() {
            o.push_str("None.\n\n");
        }
        for c in list {
            let _ = writeln!(o, "### {}\n", cell(&c["name"]));
            if !cell(&c["doc"]).is_empty() {
                let _ = writeln!(o, "{}\n", cell(&c["doc"]));
            }
            let fields = c["fields"].as_array().cloned().unwrap_or_default();
            if !fields.is_empty() {
                o.push_str("| field | type | default | range | tune | what it is |\n|---|---|---|---|---|---|\n");
                for f in fields {
                    let _ = writeln!(
                        o,
                        "| `{}` | {} | {} | {} | {} | {} |",
                        cell(&f["name"]),
                        cell(&f["type"]),
                        cell(&f["default"]),
                        cell(&f["range"]),
                        if f["tune"] == true { "tune" } else { "" },
                        cell(&f["doc"])
                    );
                }
                o.push('\n');
            }
            // The game's are declared in its own files, where an agent edits
            // them; the engine's source is not the agent's to read (found at
            // the phase 5 gate: ARGUS.md pointed at `crates\argus-...`).
            let from = if title.contains("engine") {
                "Part of the engine: its fields are above, and `argus docs` has the methods that read and change it.".to_string()
            } else {
                format!("Declared in `{}`.", cell(&c["source"]))
            };
            let _ = writeln!(
                o,
                "Written by {}; read by {}. {from}\n",
                or_none(&c["written_by"]),
                or_none(&c["read_by"]),
            );
        }
    }
    o.push_str("## Scripts: the Luau API\n\n");
    o.push_str(&docs::scripting(3));
    o.push_str("## Scenarios: the test API\n\n");
    o.push_str(&docs::scenarios(3));
    o.push_str("## Files: what a project is made of\n\n");
    o.push_str(&argus_daemon::formats::files(3));
    Ok(o)
}

fn or_none(v: &Json) -> String {
    let s = cell(v);
    if s.is_empty() {
        "nothing".to_string()
    } else {
        s
    }
}

/// The engine's reference: the tools, then every method, from the same
/// table the protocol, the CLI, and the MCP server use.
pub fn reference() -> String {
    let mut o = String::from(
        "# Argus: the tools and the protocol\n\nGenerated by `argus docs` from the engine's one schema (`argus_daemon::docs`). \
The tools are what `argus mcp` serves and `argus <tool>` runs; every method is a protocol call and `argus <namespace> <method>`.\n\n## Tools\n\n",
    );
    o.push_str(&tools_at(3));
    // A game's own code: its scripts and its scenarios (found at the phase
    // 5 gate: neither had a reference, only the games' files to copy from).
    o.push_str("## Scripts: the Luau API\n\n");
    o.push_str(&docs::scripting(3));
    o.push_str("## Scenarios: the test API\n\n");
    o.push_str(&docs::scenarios(3));
    o.push_str("## Files: what a project is made of\n\n");
    o.push_str(&argus_daemon::formats::files(3));
    o.push_str("## The engine's components\n\n");
    o.push_str(&argus_daemon::formats::components(3));
    o.push_str("## Methods\n\n");
    o.push_str(&methods_at(3));
    o
}

/// The tools, each a section at heading `level`.
fn tools_at(level: usize) -> String {
    let h = "#".repeat(level);
    let mut o = String::new();
    for t in docs::TOOLS {
        let _ = writeln!(o, "{h} {}\n\n{}\n", t.name, docs::tool_description(t));
        if !t.params.is_empty() {
            params_table(&mut o, t.params);
        }
    }
    // The ones the MCP server answers itself: choosing a project, the docs.
    for t in crate::mcp::PROJECT_TOOLS {
        let _ = writeln!(o, "{h} {}\n\n{}\n", t.name, t.doc);
        if !t.params.is_empty() {
            params_table(&mut o, t.params);
        }
    }
    o
}

/// Every protocol method, each a section at heading `level`.
fn methods_at(level: usize) -> String {
    let h = "#".repeat(level);
    let mut o = String::new();
    for m in docs::METHODS {
        let _ = writeln!(o, "{h} {}\n\n{}\n", m.name, m.doc);
        if !m.params.is_empty() {
            params_table(&mut o, m.params);
        }
        let _ = writeln!(o, "Returns {}\n", m.returns);
    }
    o
}

/// A docs answer as text for a shell.
fn docs_text(r: &Json) -> String {
    if let Some(t) = r["text"].as_str() {
        return t.to_string();
    }
    if let Some(found) = r["found"].as_array() {
        let mut o = format!("{} sections mention {}:\n", found.len(), cell(&r["find"]));
        for f in found {
            let _ = writeln!(
                o,
                "  {:<24} {}  |  {}",
                cell(&f["topic"]),
                cell(&f["section"]),
                cell(&f["line"])
            );
        }
        return o;
    }
    if let Some(s) = r["sections"].as_array() {
        let mut o = format!(
            "{} is long; its sections (argus docs {} section=\"...\"):\n",
            cell(&r["topic"]),
            cell(&r["topic"])
        );
        for h in s {
            let _ = writeln!(o, "  {}", cell(h));
        }
        return o;
    }
    let mut o = String::from("the manual (argus docs <chapter>):\n");
    for c in r["manual"].as_array().into_iter().flatten() {
        let _ = writeln!(o, "  {:<24} {}", cell(&c["topic"]), cell(&c["title"]));
    }
    o.push_str("the reference (argus docs <part>):\n");
    for p in r["reference"].as_array().into_iter().flatten() {
        let _ = writeln!(o, "  {:<24} {}", cell(&p["topic"]), cell(&p["lists"]));
    }
    o.push_str("  project                  this project's ARGUS.md, from its daemon now\nfind=<word> searches all of them.\n");
    o
}

/// The tools part of the reference, on its own (the docs tool's `tools`).
pub fn tools_reference() -> String {
    format!(
        "# The agent tools\n\nWhat `argus mcp` serves and `argus <tool>` runs, from the engine's one schema.\n\n{}",
        tools_at(2)
    )
}

/// The methods part of the reference, on its own (the docs tool's `methods`).
pub fn methods_reference() -> String {
    format!(
        "# The protocol's methods\n\nEvery one is also a command: `argus <namespace> <method>`.\n\n{}",
        methods_at(2)
    )
}

fn params_table(o: &mut String, params: &[docs::Param]) {
    o.push_str("| param | type | required | what it is |\n|---|---|---|---|\n");
    for p in params {
        let _ = writeln!(
            o,
            "| `{}` | {} | {} | {} |",
            p.name,
            p.ty,
            if p.required { "yes" } else { "" },
            cell(&json!(p.doc))
        );
    }
    o.push('\n');
}
