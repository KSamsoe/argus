//! `argus daemon start --web` (phase 5 item 10): an address the web client
//! may not listen on is refused before a daemon starts, and a daemon that
//! cannot listen (its port taken) is reported at once with its reason, not
//! after the 30 s wait for a daemon that will not come.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

fn project(tag: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("cli-web-{tag}"));
    let _ = std::fs::remove_dir_all(&root);
    for (rel, text) in [
        (
            "argus.toml",
            "name = \"webby\"\nclock = \"continuous@60\"\ndefault_scene = \"s\"\nplugins = []\n",
        ),
        (
            "scenes/s/scene.toml",
            "clock = \"continuous@60\"\nseed = 1\n",
        ),
        (
            "scenes/s/camera.toml",
            "[Transform]\n\n[Camera]\nheight = 10.0\n",
        ),
    ] {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    root
}

fn start(root: &Path, web: &str) -> (i32, String, Duration) {
    let t = Instant::now();
    let o = Command::new(env!("CARGO_BIN_EXE_argus"))
        .arg("--project")
        .arg(root)
        .args(["daemon", "start", "--web", web])
        .output()
        .unwrap();
    let _ = Command::new(env!("CARGO_BIN_EXE_argus"))
        .arg("--project")
        .arg(root)
        .args(["daemon", "stop"])
        .output();
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stderr).into_owned(),
        t.elapsed(),
    )
}

#[test]
fn an_address_off_this_machine_and_its_tailnet_is_refused_before_a_daemon_starts() {
    let root = project("lan");
    let (code, said, took) = start(&root, "192.168.1.50");
    assert_ne!(code, 0);
    assert!(said.contains("not this machine or its tailnet"), "{said}");
    assert!(!said.contains("starting a daemon"), "{said}");
    assert!(took < Duration::from_secs(5), "{took:?}");
}

#[test]
fn a_daemon_that_cannot_listen_is_reported_at_once_with_its_reason() {
    let root = project("taken");
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    let (code, said, took) = start(&root, &format!(":{port}"));
    assert_ne!(code, 0);
    assert!(said.contains("the daemon stopped"), "{said}");
    assert!(
        said.contains(&format!("cannot listen on 127.0.0.1:{port}")),
        "{said}"
    );
    assert!(
        took < Duration::from_secs(20),
        "not the 30 s wait: {took:?}"
    );
}
