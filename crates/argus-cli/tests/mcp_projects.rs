//! Distribution item 3: `argus mcp` with no project (a user-level entry in
//! an agent host) starts on none, and its project tools list the projects,
//! make one from a template, and open it, after which the engine's tools
//! work on it.

use serde_json::{json, Value as Json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    lines: mpsc::Receiver<String>,
    next: u64,
}

impl Mcp {
    fn start(cwd: &Path, home: &Path) -> Mcp {
        let mut child = Command::new(env!("CARGO_BIN_EXE_argus"))
            .arg("mcp")
            .current_dir(cwd)
            .env("ARGUS_HOME", home)
            .env(
                "ARGUS_TEMPLATES",
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../templates"),
            )
            .env_remove("ARGUS_PROJECT")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for l in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(l).is_err() {
                    break;
                }
            }
        });
        Mcp {
            child,
            stdin,
            lines,
            next: 1,
        }
    }

    fn request(&mut self, method: &str, params: Json) -> Json {
        let id = self.next;
        self.next += 1;
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
        let line = self
            .lines
            .recv_timeout(Duration::from_secs(300))
            .expect("an answer");
        let v: Json = serde_json::from_str(&line).unwrap();
        assert_eq!(v["id"], id, "{v}");
        v["result"].clone()
    }

    /// A tool's answer: its JSON (after the command line that reruns it),
    /// and whether it is an error.
    fn tool(&mut self, name: &str, args: Json) -> (Json, bool) {
        let r = self.request("tools/call", json!({ "name": name, "arguments": args }));
        let text = r["content"][0]["text"].as_str().unwrap().to_string();
        let body = text.split_once('\n').map_or("", |(_, b)| b);
        (
            serde_json::from_str(body).unwrap_or(json!(body)),
            r["isError"] == true,
        )
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("argus-mcp-projects-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn with_no_project_the_server_offers_them_makes_one_and_opens_it() {
    let base = temp("free");
    let home = base.join("home");
    let mut m = Mcp::start(&base, &home);
    let init = m.request(
        "initialize",
        json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } }),
    );
    let said = init["instructions"].as_str().unwrap();
    assert!(said.contains("No project is open"), "{said}");

    let tools = m.request("tools/list", json!({}));
    let names: Vec<&str> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for t in ["projects", "open_project", "new_project", "summary"] {
        assert!(names.contains(&t), "{t} in {names:?}");
    }

    // An engine tool says what to do first.
    let (e, is_error) = m.tool("summary", json!({}));
    assert!(is_error, "{e}");
    assert!(e.to_string().contains("open_project"), "{e}");

    let (p, is_error) = m.tool("projects", json!({}));
    assert!(!is_error, "{p}");
    assert!(p["open"].is_null(), "{p}");
    assert!(
        p["templates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "blank"),
        "{p}"
    );

    let dir = base.join("lantern");
    let (made, is_error) = m.tool(
        "new_project",
        json!({ "dir": dir.to_string_lossy(), "template": "blank" }),
    );
    assert!(!is_error, "{made}");
    assert_eq!(made["open"]["name"], "lantern", "{made}");
    assert_eq!(
        made["made"]["tests"]["passed"], made["made"]["tests"]["total"],
        "{made}"
    );
    assert!(dir.join("ARGUS.md").is_file());

    // The engine's tools are on it now.
    let (q, is_error) = m.tool("query", json!({ "entity": "@player" }));
    assert!(!is_error, "{q}");
    assert!(q.to_string().contains("Player"), "{q}");
    let (p, _) = m.tool("projects", json!({ "under": base.to_string_lossy() }));
    assert_eq!(p["open"]["name"], "lantern", "{p}");
    assert_eq!(p["under"].as_array().unwrap().len(), 1, "{p}");

    drop(m);
    let _ = Command::new(env!("CARGO_BIN_EXE_argus"))
        .arg("--project")
        .arg(&dir)
        .args(["daemon", "stop"])
        .output();
}
