//! §16 `argus mcp` (phase 2 item 8), end to end: the binary speaks MCP on
//! stdio, runs world.summary on connect, lists the dozen tools with the
//! descriptions and input schemas of the one schema, and every tool works;
//! a tool's answer is the one its shell form gives.

#[path = "../../argus-daemon/tests/common/mod.rs"]
mod common;

use argus_daemon::docs;
use serde_json::{json, Value as Json};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    lines: mpsc::Receiver<String>,
    next: u64,
    root: std::path::PathBuf,
}

impl Mcp {
    fn start(root: &Path) -> Mcp {
        let mut child = Command::new(env!("CARGO_BIN_EXE_argus"))
            .arg("--project")
            .arg(root)
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout: ChildStdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for l in BufReader::new(stdout).lines() {
                match l {
                    Ok(l) => {
                        if tx.send(l).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Mcp {
            child,
            stdin,
            lines,
            next: 1,
            root: root.to_path_buf(),
        }
    }

    fn send(&mut self, msg: Json) {
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// A request and its answer (one line each; nothing else on stdout).
    fn request(&mut self, method: &str, params: Json) -> Json {
        let id = self.next;
        self.next += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let line = self
            .lines
            .recv_timeout(Duration::from_secs(120))
            .expect("the server answers");
        let v: Json =
            serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON ({e}): {line}"));
        assert_eq!(v["id"], id, "{v}");
        v
    }

    /// A tool call: (isError, the rerun line, the answer's JSON, content).
    fn tool(&mut self, name: &str, args: Json) -> (bool, String, Json, Vec<Json>) {
        let r = self.request("tools/call", json!({ "name": name, "arguments": args }));
        let result = &r["result"];
        let content = result["content"].as_array().cloned().unwrap_or_default();
        let text = content[0]["text"].as_str().unwrap_or_default().to_string();
        let (first, rest) = text.split_once('\n').unwrap_or((&text, ""));
        let json: Json = serde_json::from_str(rest).unwrap_or(Json::Null);
        (result["isError"] == true, first.to_string(), json, content)
    }

    /// The shell form of a tool call, against the same daemon.
    fn shell(&self, name: &str, args: &Json) -> Json {
        let o = Command::new(env!("CARGO_BIN_EXE_argus"))
            .arg("--project")
            .arg(&self.root)
            .args([name, "--params", &args.to_string(), "--json"])
            .output()
            .unwrap();
        serde_json::from_slice(&o.stdout).unwrap_or_else(|_| {
            panic!(
                "argus {name}: {} {}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            )
        })
    }
}

struct StopOnDrop(std::path::PathBuf);
impl Drop for StopOnDrop {
    fn drop(&mut self) {
        let _ = Command::new(env!("CARGO_BIN_EXE_argus"))
            .arg("--project")
            .arg(&self.0)
            .args(["daemon", "stop"])
            .output();
    }
}

#[test]
fn argus_mcp_serves_the_tools_of_the_one_schema() {
    let t = common::temp("mcp");
    common::project(t.path());
    common::write(
        t.path(),
        "defs/timeline.toml",
        "[\"nudge\"]
length = 1.0

[\"nudge.hurt\"]
timeline = \"timeline:nudge\"
at = 0.25
kind = \"event\"
effects = \"actor:Health.cur = 4\"
",
    );
    let root = t.path().canonicalize().unwrap();
    let _stop = StopOnDrop(root.clone());
    let mut m = Mcp::start(&root);

    // Connect: the version is agreed and the world's summary comes along.
    let init = m.request(
        "initialize",
        json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } }),
    );
    let r = &init["result"];
    assert_eq!(r["protocolVersion"], "2025-06-18");
    assert_eq!(r["serverInfo"]["name"], "argus");
    assert!(r["capabilities"]["tools"].is_object());
    let inst = r["instructions"].as_str().unwrap();
    assert!(
        inst.contains("world.summary") && inst.contains("\"arena\""),
        "{inst}"
    );
    m.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    let old = m.request("initialize", json!({ "protocolVersion": "1999-01-01" }));
    assert_eq!(
        old["result"]["protocolVersion"], "2025-06-18",
        "an unknown version gets the newest"
    );

    // The list is the schema's.
    let list = m.request("tools/list", json!({}));
    let tools = list["result"]["tools"].as_array().unwrap();
    // The engine's tools, then the ones that choose the project
    // (distribution item 3).
    let project_tools = argus_cli::mcp::PROJECT_TOOLS;
    assert_eq!(tools.len(), docs::TOOLS.len() + project_tools.len());
    for (t, d) in tools.iter().zip(docs::TOOLS) {
        assert_eq!(t["name"], d.name);
        assert_eq!(t["description"], docs::tool_description(d));
        assert_eq!(t["inputSchema"], docs::input_schema(d.params));
    }
    for (t, d) in tools[docs::TOOLS.len()..].iter().zip(project_tools) {
        assert_eq!(t["name"], d.name);
        assert_eq!(t["description"], d.doc);
        assert_eq!(t["inputSchema"], docs::input_schema(d.params));
    }

    // Reading tools answer as their shell forms do.
    for (name, args) in [
        ("summary", json!({})),
        ("query", json!({ "entity": "@player" })),
        (
            "query",
            json!({ "select": ["id", "Health.cur"], "where": { "has": ["Health"] } }),
        ),
        ("describe", json!({ "component": "Health" })),
        ("describe", json!({ "systems": true })),
        ("observe", json!({})),
        ("marks", json!({})),
        ("query", json!({ "tuning": true })),
    ] {
        let (is_err, rerun, got, _) = m.tool(name, args.clone());
        assert!(!is_err, "{name} {args}: {got}");
        assert_eq!(rerun, format!("argus {name} --params '{args}'"));
        // The same answer, but for how long it took.
        let timeless = |mut v: Json| {
            if let Some(o) = v.as_object_mut() {
                o.remove("ms");
            }
            v
        };
        assert_eq!(
            timeless(got),
            timeless(m.shell(name, &args)),
            "{name} {args}: the MCP answer is the shell's"
        );
    }
    let (_, _, sem, _) = m.tool("observe", json!({}));
    assert_eq!(sem["format"], "semantic");

    // Change, run, look back.
    let (is_err, _, got, _) = m.tool(
        "edit",
        json!({ "ops": [{ "op": "set", "entity": "@player", "path": "Health.cur", "value": 7 }], "note": "from the MCP test" }),
    );
    assert!(!is_err, "{got}");
    let (_, _, p, _) = m.tool("query", json!({ "entity": "@player" }));
    assert_eq!(p["components"]["Health"]["cur"], 7);
    let (is_err, _, got, _) = m.tool(
        "edit",
        json!({ "ops": [
            { "op": "set", "entity": "@player", "path": "Health.cur", "value": 9 },
            { "op": "set", "entity": "@nobody", "path": "Health.cur", "value": 1 }
        ] }),
    );
    assert!(is_err && got["nothing_kept"] == true, "{got}");
    assert!(got["message"].as_str().unwrap().contains("nobody"), "{got}");
    let (_, _, p, _) = m.tool("query", json!({ "entity": "@player" }));
    assert_eq!(
        p["components"]["Health"]["cur"], 7,
        "a failed op keeps none"
    );
    let (is_err, _, got, _) = m.tool("edit", json!({ "undo": true }));
    assert!(!is_err, "{got}");
    let (_, _, p, _) = m.tool("query", json!({ "entity": "@player" }));
    assert_ne!(p["components"]["Health"]["cur"], 7, "undone");

    let (is_err, _, got, _) = m.tool("run", json!({ "step": 30 }));
    assert!(!is_err, "{got}");
    assert_eq!(got["status"]["tick"], 30);
    assert_eq!(got["did"], json!(["sim.step"]));
    let (is_err, _, got, _) = m.tool("seek", json!({ "frame": 10 }));
    assert!(!is_err, "{got}");
    assert_eq!(got["tick"], 10);
    // A timeline played and watched in one call, as the editor's Play.
    let (is_err, _, got, _) = m.tool(
        "run",
        json!({ "timeline": { "name": "nudge", "actor": "@player" }, "step": 30 }),
    );
    assert!(!is_err, "{got}");
    assert_eq!(got["did"], json!(["timeline.play", "sim.step"]));
    let (_, _, p, _) = m.tool("query", json!({ "entity": "@player" }));
    assert_eq!(
        p["components"]["Health"]["cur"], 4,
        "its cue fired on the actor"
    );
    let (is_err, _, got, _) = m.tool("events", json!({ "limit": 5 }));
    assert!(!is_err && got["rows"].is_array(), "{got}");
    let (is_err, _, got, _) = m.tool("reload", json!({}));
    assert!(!is_err && got["errors"].is_object(), "{got}");

    // A sound made from a kind and a seed: written as an edit, imported,
    // and described; not over one that is there; undone like any edit.
    let (is_err, _, got, _) = m.tool(
        "generate",
        json!({ "name": "sfx/zap", "sound": "laser", "seed": 2 }),
    );
    assert!(!is_err, "{got}");
    let on_disk = std::fs::read_to_string(m.root.join("assets/sfx/zap.toml")).unwrap();
    assert_eq!(json!(on_disk.replace("\r\n", "\n")), got["text"]);
    assert!(
        on_disk.contains("argus generate name=sfx/zap sound=laser seed=2"),
        "{on_disk}"
    );
    let heard = got["heard"]["text"].as_str().unwrap();
    assert!(
        heard.contains("a note") && heard.contains("(lower)"),
        "{heard}"
    );
    assert_eq!(got["edit"]["author"], "agent:claude");
    let (is_err, _, got, _) = m.tool("generate", json!({ "name": "sfx/zap", "sound": "jump" }));
    assert!(
        is_err && got["message"].as_str().unwrap().contains("there already"),
        "{got}"
    );
    let (is_err, _, got, _) = m.tool("edit", json!({ "undo": true }));
    assert!(!is_err, "{got}");
    assert!(!m.root.join("assets/sfx/zap.toml").exists(), "undone");
    // A sprite: an art script, imported, and its picture in the answer.
    let (is_err, _, got, content) = m.tool(
        "generate",
        json!({ "name": "sprites/blob", "sprite": "creature", "seed": 3 }),
    );
    assert!(!is_err, "{got}");
    assert!(m.root.join("assets/sprites/blob.art.luau").exists());
    assert_eq!(got["drawn"]["clips"], json!(["idle", "walk"]));
    assert_eq!(got["imported"], json!(["sprites/blob"]), "{got}");
    assert!(
        content
            .iter()
            .any(|c| c["type"] == "image" && c["mimeType"] == "image/png"),
        "the picture is in the answer"
    );
    // A name no scenario has runs nothing, which is an error, not a pass
    // (issue 29).
    let (is_err, _, got, _) = m.tool("test", json!({ "name": "no scenario is called this" }));
    assert!(is_err, "{got}");
    assert!(got.to_string().contains("nothing ran"), "{got}");

    // A proposal waits for the human.
    let (is_err, _, got, _) = m.tool(
        "propose",
        json!({ "ops": [{ "op": "set", "entity": "@player", "path": "Health.cur", "value": 3 }], "note": "try a fragile hero" }),
    );
    assert!(!is_err, "{got}");
    let (_, _, waiting, _) = m.tool("propose", json!({ "list": true }));
    assert_eq!(
        waiting["rows"].as_array().map(Vec::len),
        Some(1),
        "{waiting}"
    );

    // A picture comes back as one.
    let (is_err, _, got, content) = m.tool(
        "observe",
        json!({ "format": "png", "width": 160, "height": 90 }),
    );
    if is_err {
        eprintln!("no picture on this machine: {got}");
    } else {
        let img = content
            .iter()
            .find(|c| c["type"] == "image")
            .expect("an image");
        assert_eq!(img["mimeType"], "image/png");
        assert!(
            img["data"].as_str().unwrap().starts_with("iVBORw0KGgo"),
            "a PNG"
        );
    }

    // Mistakes are answers, not crashes.
    let (is_err, _, got, _) = m.tool("query", json!({ "bogus": 1 }));
    assert!(
        is_err && got["message"].as_str().unwrap().contains("takes no"),
        "{got}"
    );
    let (is_err, _, got, _) = m.tool("frobnicate", json!({}));
    assert!(
        is_err && got["message"].as_str().unwrap().contains("no tool"),
        "{got}"
    );
    let e = m.request("resources/list", json!({}));
    assert_eq!(e["error"]["code"], -32601);
    assert_eq!(m.request("ping", json!({}))["result"], json!({}));

    // The server ends with its input.
    drop(m.stdin);
    let status = m.child.wait().unwrap();
    assert!(status.success());
}

#[test]
fn argus_docs_writes_argus_md_from_the_live_daemon_and_the_reference_from_the_schema() {
    let t = common::temp("argusmd");
    common::project(t.path());
    let root = t.path().canonicalize().unwrap();
    let _stop = StopOnDrop(root.clone());
    let o = Command::new(env!("CARGO_BIN_EXE_argus"))
        .arg("--project")
        .arg(&root)
        .args(["docs", "--for-agent"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let md = std::fs::read_to_string(root.join("ARGUS.md")).unwrap();
    for part in [
        "## Working with this project",
        "## Scenes",
        "### arena (loaded)",
        "## Systems, in run order",
        "## Prefabs",
        "## The game's components",
        "## The engine's components",
    ] {
        assert!(md.contains(part), "ARGUS.md lacks {part:?}");
    }
    // The prefab catalog, with its instance; the game's own components with
    // their docs and who writes them; the systems with their write sets.
    assert!(md.contains("| `enemy/bat` |"), "{md}");
    let game = &md[md.find("## The game's components").unwrap()
        ..md.find("## The engine's components").unwrap()];
    assert!(
        game.contains("### Health") && game.contains("### Enemy") && game.contains("Hostile"),
        "{game}"
    );
    assert!(!game.contains("### Transform"), "Transform is the engine's");
    assert!(
        md.contains("| `sim.damage` |"),
        "the game's system is listed"
    );
    for t in docs::TOOLS {
        assert!(md.contains(&format!("| `{}` |", t.name)), "tool {}", t.name);
    }
    // Regenerated on demand: the same project gives the same file.
    let again = Command::new(env!("CARGO_BIN_EXE_argus"))
        .arg("--project")
        .arg(&root)
        .args(["docs", "--for-agent", "--out"])
        .arg(root.join("again.md"))
        .output()
        .unwrap();
    assert!(again.status.success());
    assert_eq!(std::fs::read_to_string(root.join("again.md")).unwrap(), md);

    // `argus docs`: the engine's reference, every tool and method.
    let r = Command::new(env!("CARGO_BIN_EXE_argus"))
        .arg("docs")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&r.stdout);
    for t in docs::TOOLS {
        let head = format!("### {}\n", t.name);
        assert!(text.contains(&head), "reference lacks tool {}", t.name);
    }
    for m in docs::METHODS {
        let head = format!("### {}\n", m.name);
        assert!(text.contains(&head), "reference lacks {}", m.name);
    }
}
