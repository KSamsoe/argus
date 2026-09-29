//! P1 and §11: the editor reaches the world only through the protocol. Its
//! dependency tree holds the protocol client and the UI, and no engine
//! crate: nothing that could hold a world, run a system, or read a scene
//! file behind the daemon's back.

use std::process::Command;

#[test]
fn the_editor_depends_on_the_protocol_client_and_no_engine_crate() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let out = Command::new(env!("CARGO"))
        .args([
            "tree",
            "-p",
            "argus-editor",
            "-e",
            "normal",
            "--prefix",
            "none",
            "--format",
            "{p}",
            "--offline",
        ])
        .current_dir(root)
        .output()
        .expect("cargo tree runs");
    assert!(
        out.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let mut ours: Vec<&str> = text
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter(|n| n.starts_with("argus"))
        .collect();
    ours.sort();
    ours.dedup();
    assert_eq!(
        ours,
        vec!["argus-client", "argus-editor"],
        "the editor's own crates; anything else is a private channel to the engine"
    );
    assert!(
        text.lines().any(|l| l.starts_with("egui ")),
        "the UI is there: {text}"
    );
}
