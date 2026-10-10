//! `argus mcp` (§16, phase 2 item 8): the agent tools as a Model Context
//! Protocol server on stdin and stdout, newline-delimited JSON-RPC 2.0.
//!
//! An agent host starts it (`{"command": "argus", "args": ["mcp",
//! "--project", "<dir>"]}`); it finds or starts the project's daemon, the
//! same way every `argus` command does, and runs `world.summary` on connect
//! (§5), which goes into its instructions so the agent starts oriented. The
//! tools, their descriptions, and their input schemas come from
//! `argus_daemon::docs::TOOLS`, the table the CLI's help is made from; a
//! call's answer begins with the command line that reruns it from a shell.
//! Nothing but protocol messages is written to stdout; the daemon's startup
//! chatter goes to stderr.
//!
//! With no project (a user-level entry in an agent host, distribution item
//! 3), it starts on none: `projects` lists the ones this machine knows,
//! `open_project` points the tools at one, and `new_project` makes one from
//! a template. Those three work with a project open too, to switch.

use crate::client::Client;
use crate::{project, tools, Invocation};
use argus_daemon::docs::{self, opt, req, ToolDoc, STRING};
use serde_json::{json, Value as Json};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// The protocol versions this server speaks, newest first.
pub const VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// Tools that only read; an agent host may run them without asking.
const READ_ONLY: &[&str] = &[
    "summary", "query", "describe", "observe", "events", "projects",
];

/// The tools that choose the project the others work on (distribution
/// item 3). They run in this server, not the daemon.
pub const PROJECT_TOOLS: &[ToolDoc] = &[
    ToolDoc {
        name: "projects",
        doc: "The Argus projects on this machine: the one the tools are on now (open), the ones opened before, most recent first (recent), the templates a new one can start from, and with under, every project in that folder. From a shell: argus projects [under=<dir>].",
        params: &[opt("under", STRING, "a folder: the projects in it, a few levels down")],
        calls: &[],
    },
    ToolDoc {
        name: "open_project",
        doc: "Point the tools at a project: its daemon is found or started, and the answer is its world.summary. From a shell: argus --project <dir> <command>.",
        params: &[req("path", STRING, "the project's directory, the one with argus.toml (a path from projects)")],
        calls: &["world.summary"],
    },
    ToolDoc {
        name: "new_project",
        doc: "Make a project from a template, run its scenarios (they pass), write its ARGUS.md, AGENTS.md, and CLAUDE.md, and open it. Read its ARGUS.md next. From a shell: argus new <dir> --template <t>.",
        params: &[
            req("dir", STRING, "where: a new or empty directory"),
            opt("template", STRING, "one of the templates projects lists; default blank"),
            opt("name", STRING, "the game's name; default the directory's"),
        ],
        calls: &["test.run", "world.summary"],
    },
];

fn listed(t: &ToolDoc, description: String) -> Json {
    let mut o = json!({
        "name": t.name,
        "description": description,
        "inputSchema": docs::input_schema(t.params),
    });
    if READ_ONLY.contains(&t.name) {
        o["annotations"] = json!({ "readOnlyHint": true });
    }
    o
}

/// The tool list as `tools/list` gives it: the engine's, then the
/// project tools.
pub fn tool_list() -> Vec<Json> {
    docs::TOOLS
        .iter()
        .map(|t| listed(t, docs::tool_description(t)))
        .chain(PROJECT_TOOLS.iter().map(|t| listed(t, t.doc.to_string())))
        .collect()
}

/// What `projects` answers: the open one, the recent ones, the templates,
/// and those under a folder.
pub fn projects_report(open: Option<&Path>, under: Option<&str>) -> Json {
    let mut r = json!({
        "open": open.map(|p| crate::projects::describe(p, None)),
        "recent": crate::projects::recent(),
        "templates": crate::scaffold::root()
            .map(|t| crate::scaffold::list(&t))
            .unwrap_or_default()
            .into_iter()
            .map(|(name, about)| json!({ "name": name, "about": about }))
            .collect::<Vec<_>>(),
    });
    if let Some(d) = under {
        r["under"] = json!(crate::projects::under(Path::new(d)));
    }
    r
}

/// The projects in a few lines, for the instructions: the most recent
/// ones and the templates (`projects` has the rest).
fn projects_brief() -> String {
    const SHOWN: usize = 8;
    let r = projects_report(None, None);
    let recent = r["recent"].as_array().cloned().unwrap_or_default();
    let mut s = if recent.is_empty() {
        "No projects opened yet.".to_string()
    } else {
        let mut s = String::from("Recent projects (open_project with the path):");
        for p in recent.iter().take(SHOWN) {
            s.push_str(&format!(
                "\n- {}: {}",
                p["name"].as_str().unwrap_or("?"),
                p["path"].as_str().unwrap_or("?")
            ));
        }
        if recent.len() > SHOWN {
            s.push_str(&format!(
                "\n- and {} more (projects lists them)",
                recent.len() - SHOWN
            ));
        }
        s
    };
    let templates: Vec<&str> = r["templates"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["name"].as_str())
        .collect();
    if !templates.is_empty() {
        s.push_str(&format!(
            "\nTemplates for new_project: {}.",
            templates.join(", ")
        ));
    }
    s
}

/// What the agent is told when no project is open.
fn no_project() -> String {
    "No project is open: list them with projects, open one with open_project, or make one with new_project.".to_string()
}

/// What the server tells the agent on connect, with the world's summary.
fn instructions(summary: Result<Json, String>) -> String {
    let mut s = String::from(
        "Argus is a protocol-first 2D game engine. These tools drive one project's live game world, held by the engine's daemon. \
Read it with summary, query, describe, observe (the semantic frame, or a picture), and events (what happened and why). \
Change the game's authored state with edit (one transaction with a note, undoable) or offer a change for the human to feel with propose. \
Advance time with run, go back to any frame with seek, run the project's scenarios with test, and take script and asset changes from disk with reload. \
marks is the queue of moments the human flagged while playing, each with a note; open one to reproduce it. \
Every tool is also a shell command, argus <tool> key=value, and the full protocol is argus <namespace> <method>. \
The project's ARGUS.md (argus docs --for-agent) lists its components, systems, prefabs, and scenes.\n\n",
    );
    match summary {
        Ok(v) => {
            s.push_str("The world now (world.summary):\n");
            s.push_str(&v.to_string());
        }
        Err(e) if e.starts_with(&no_project()) => s.push_str(&e),
        Err(e) => {
            s.push_str("The project's daemon could not be reached yet: ");
            s.push_str(&e);
            s.push_str(". Tools will try again.");
        }
    }
    s
}

/// Standard base64 with padding (a picture in a tool result).
pub fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 {
            A[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            A[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

struct Server<'a> {
    /// The command line, with `project` set to the project open.
    inv: Invocation,
    root: Option<PathBuf>,
    client: Option<Client>,
    err: &'a mut dyn Write,
}

impl Server<'_> {
    /// The daemon, found or started.
    fn client(&mut self) -> Result<&mut Client, String> {
        if self.root.is_none() && self.inv.socket.is_none() {
            return Err(no_project());
        }
        if self.client.is_none() {
            self.client = Some(project::connect_as(
                &self.inv,
                self.err,
                Some("agent:claude"),
            )?);
        }
        Ok(self.client.as_mut().expect("connected"))
    }

    /// Point the tools at a project; its summary.
    fn open(&mut self, path: &str) -> Result<Json, Json> {
        let root =
            project::find_project(Some(Path::new(path))).map_err(|e| json!({ "message": e }))?;
        let root = root.canonicalize().unwrap_or(root);
        self.client = None;
        self.root = Some(root.clone());
        self.inv.project = Some(root.clone());
        self.inv.socket = None;
        let c = self.client().map_err(|e| json!({ "message": e }))?;
        let summary = tools::run(c, "summary", &json!({}))?;
        Ok(json!({
            "open": crate::projects::describe(&root, None),
            "summary": summary,
            "next": "Read the project's ARGUS.md (in its directory) before changing it.",
        }))
    }

    /// The tools that choose the project.
    fn project_tool(&mut self, name: &str, args: &Json) -> Result<Json, Json> {
        let s = |k: &str| args.get(k).and_then(Json::as_str);
        match name {
            "projects" => Ok(projects_report(self.root.as_deref(), s("under"))),
            "open_project" => {
                let path =
                    s("path").ok_or_else(|| json!({ "message": "open_project needs path" }))?;
                self.open(path)
            }
            _ => {
                let dir = s("dir").ok_or_else(|| json!({ "message": "new_project needs dir" }))?;
                let made =
                    crate::scaffold::make(Path::new(dir), s("template"), s("name"), true, self.err)
                        .map_err(|e| json!({ "message": e }))?;
                let mut opened = self.open(made["dir"].as_str().unwrap_or(dir))?;
                opened["made"] = made;
                Ok(opened)
            }
        }
    }

    fn initialize(&mut self, params: &Json) -> Json {
        let asked = params.get("protocolVersion").and_then(Json::as_str);
        let version = asked
            .filter(|v| VERSIONS.contains(v))
            .unwrap_or(VERSIONS[0]);
        let summary = if self.root.is_none() && self.inv.socket.is_none() {
            Err(format!("{}\n\n{}", no_project(), projects_brief()))
        } else {
            self.client()
                .and_then(|c| tools::run(c, "summary", &json!({})).map_err(|e| e.to_string()))
        };
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "argus", "version": env!("CARGO_PKG_VERSION") },
            "instructions": instructions(summary),
        })
    }

    fn call(&mut self, params: &Json) -> Json {
        let name = params.get("name").and_then(Json::as_str).unwrap_or("");
        let args = params.get("arguments").cloned().unwrap_or(json!({}));
        let rerun = format!("argus {name} --params '{args}'");
        let res = if PROJECT_TOOLS.iter().any(|t| t.name == name) {
            self.project_tool(name, &args)
        } else {
            match self.client() {
                Ok(c) => tools::run(c, name, &args),
                Err(e) => Err(json!({ "message": e })),
            }
        };
        match res {
            Ok(r) => {
                let mut content = vec![
                    json!({ "type": "text", "text": format!("{rerun}\n{}", serde_json::to_string_pretty(&r).unwrap_or_default()) }),
                ];
                if let Some(img) = self.picture(name, &r) {
                    content.push(img);
                }
                json!({ "content": content, "isError": false })
            }
            Err(e) => {
                // A lost daemon is found again on the next call.
                if e["message"]
                    .as_str()
                    .is_some_and(|m| m.starts_with("connection lost"))
                {
                    self.client = None;
                }
                json!({
                    "content": [{ "type": "text", "text": format!("{rerun}\n{}", serde_json::to_string_pretty(&e).unwrap_or_default()) }],
                    "isError": true,
                })
            }
        }
    }

    /// `observe` with a PNG: the picture itself, for an agent that can see.
    fn picture(&mut self, tool: &str, r: &Json) -> Option<Json> {
        if tool != "observe" || r.get("format").and_then(Json::as_str) == Some("semantic") {
            return None;
        }
        let path = PathBuf::from(r.get("path")?.as_str()?);
        let path = if path.is_absolute() {
            path
        } else {
            self.root.as_ref()?.join(path)
        };
        let bytes = std::fs::read(path).ok()?;
        Some(json!({ "type": "image", "data": base64(&bytes), "mimeType": "image/png" }))
    }
}

/// `argus mcp`: serve until stdin closes.
pub fn serve(inv: &Invocation, err: &mut dyn Write) -> i32 {
    let root = project::find_project(inv.project.as_deref()).ok();
    let mut inv = inv.clone();
    if let Some(r) = &root {
        inv.project = Some(r.clone());
    }
    let mut server = Server {
        inv,
        root,
        client: None,
        err,
    };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Json>(&line) {
            Err(e) => Some(
                json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("not JSON: {e}") } }),
            ),
            Ok(msg) => {
                let method = msg.get("method").and_then(Json::as_str).unwrap_or("");
                let params = msg.get("params").cloned().unwrap_or(json!({}));
                // Notifications (initialized, cancelled) get no answer.
                msg.get("id").cloned().map(|id| {
                    let result = match method {
                        "initialize" => Ok(server.initialize(&params)),
                        "ping" => Ok(json!({})),
                        "tools/list" => Ok(json!({ "tools": tool_list() })),
                        "tools/call" => Ok(server.call(&params)),
                        other => Err(
                            json!({ "code": -32601, "message": format!("no method {other:?}") }),
                        ),
                    };
                    match result {
                        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
                        Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
                    }
                })
            }
        };
        if let Some(r) = reply {
            if writeln!(stdout, "{r}")
                .and_then(|_| stdout.flush())
                .is_err()
            {
                break;
            }
        }
    }
    crate::EXIT_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn every_tool_has_a_schema_and_the_read_only_ones_say_so() {
        let list = tool_list();
        assert_eq!(list.len(), docs::TOOLS.len() + PROJECT_TOOLS.len());
        for t in &list {
            assert_eq!(t["inputSchema"]["type"], "object", "{t}");
            assert!(t["description"]
                .as_str()
                .unwrap()
                .contains("From a shell: argus"));
        }
        let ro: Vec<&str> = list
            .iter()
            .filter(|t| t["annotations"]["readOnlyHint"] == true)
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(ro, READ_ONLY);
    }
}
