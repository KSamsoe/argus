//! An MCP session leaves no daemon it started (Todd's Mac test, issue 2:
//! `argus mcp` in a game's folder left the game's daemon running after the
//! host closed the pipe). A daemon that was running before the session,
//! or that another client is connected to when it ends, stays.

use argus_cli::client::Client;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A temp directory removed on drop, used as its path.
struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        // A daemon a failed test left would hold the folder.
        let _ = argus(&self.0.join("game"), &["daemon", "stop"]);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for Temp {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

/// A new game from the blank template, in its own temp folder.
fn game(name: &str) -> Temp {
    let d = std::env::temp_dir().join(format!("argus-mcp-daemon-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let t = Temp(d);
    let o = Command::new(env!("CARGO_BIN_EXE_argus"))
        .arg("new")
        .arg(t.join("game"))
        .args(["--no-git", "--json"])
        .env(
            "ARGUS_TEMPLATES",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../templates"),
        )
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    t
}

fn argus(project: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_argus"))
        .arg("--project")
        .arg(project)
        .args(args)
        .env_remove("ARGUS_PROJECT")
        .output()
        .unwrap()
}

/// Whether a daemon answers for the project (none is started to ask).
fn running(project: &Path) -> bool {
    argus(project, &["--no-start", "session", "status"])
        .status
        .success()
}

/// An MCP session on the project that asks for the summary (which finds or
/// starts its daemon), then closes the pipe as a host does, and has ended.
fn session(project: &Path) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_argus"))
        .arg("mcp")
        .arg("--project")
        .arg(project)
        .env_remove("ARGUS_PROJECT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    for msg in [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"summary","arguments":{}}}"#,
    ] {
        writeln!(stdin, "{msg}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        assert!(line.contains("\"result\""), "{line}");
    }
    drop(stdin);
    let deadline = Instant::now() + Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "the session did not end when its pipe closed"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_session_stops_the_daemon_it_started() {
    let t = game("started");
    let p = t.join("game");
    assert!(!running(&p));
    session(&p);
    assert!(
        !running(&p),
        "the daemon the session started is still running"
    );
}

#[test]
fn a_session_leaves_a_daemon_that_was_running() {
    let t = game("before");
    let p = t.join("game");
    assert!(argus(&p, &["daemon", "start"]).status.success());
    session(&p);
    assert!(running(&p), "the session stopped a daemon it did not start");
    assert!(argus(&p, &["daemon", "stop"]).status.success());
}

#[test]
fn a_session_leaves_a_daemon_another_client_is_connected_to() {
    let t = game("shared");
    let p = t.join("game");
    // The session starts the daemon; then another client connects (the
    // editor, say) and is still there when the session ends.
    let mut child = Command::new(env!("CARGO_BIN_EXE_argus"))
        .arg("mcp")
        .arg("--project")
        .arg(&p)
        .env_remove("ARGUS_PROJECT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    for msg in [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"summary","arguments":{}}}"#,
    ] {
        writeln!(stdin, "{msg}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
    }
    let info = argus_cli::project::daemon_info(&p.canonicalize().unwrap()).expect("daemon.json");
    let mut other = Client::connect(info["socket"].as_str().unwrap()).unwrap();
    let r = other.call("session.status", serde_json::json!({})).unwrap();
    assert_eq!(r["result"]["clients"], 2, "{r}");
    drop(stdin);
    let deadline = Instant::now() + Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "the session did not end");
        std::thread::sleep(Duration::from_millis(20));
    }
    let r = other.call("session.status", serde_json::json!({})).unwrap();
    assert_eq!(
        r["result"]["clients"], 1,
        "the other client is still served: {r}"
    );
    drop(other);
    assert!(argus(&p, &["daemon", "stop"]).status.success());
}
