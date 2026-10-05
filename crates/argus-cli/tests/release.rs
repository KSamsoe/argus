//! Section 17: "Release build, incremental < 30 s" (phase 5 item 0). A
//! release build of Hollow, then one script changed and committed, then the
//! build again over the first: the packs are content-addressed, so only
//! what changed ships, and cargo relinks only the player for its new code
//! version. Ignored: the first build compiles the ship profile when it is
//! cold. CI runs it on the reference laptop, where it is warm.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

const BOUND: Duration = Duration::from_secs(30);

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let name = e.file_name();
        if name == ".argus" || name == "dist" {
            continue;
        }
        if e.path().is_dir() {
            copy(&e.path(), &to.join(&name));
        } else {
            std::fs::copy(e.path(), to.join(&name)).unwrap();
        }
    }
}

fn git(root: &Path, args: &[&str]) {
    let o = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["-c", "user.name=argus", "-c", "user.email=argus@invalid"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
}

fn build(root: &Path, out: &Path, since: Option<&Path>) -> (serde_json::Value, Duration) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_argus"));
    c.arg("--project")
        .arg(root)
        .args(["--json", "build", "--release", "--out"])
        .arg(out);
    if let Some(s) = since {
        c.arg("--since").arg(s);
    }
    let t = Instant::now();
    let o = c.output().unwrap();
    let took = t.elapsed();
    assert!(
        o.status.success(),
        "argus build: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    (serde_json::from_slice(&o.stdout).unwrap(), took)
}

#[test]
#[ignore = "a release build; CI runs it (cargo test --release -- --ignored)"]
fn a_release_build_after_one_script_changed_takes_under_30_s() {
    let dir = std::env::temp_dir().join(format!("argus-incremental-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.join("hollow");
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hollow"),
        &root,
    );
    std::fs::write(root.join(".gitignore"), ".argus/\n").unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "first"]);
    let (first, _) = build(&root, &dir.join("a"), None);

    // One script changed, as a developer commits it: its lines move, so its
    // compiled blob is new, and the code version the player carries is too.
    let script = root.join("systems/signs.luau");
    let was = std::fs::read_to_string(&script).unwrap();
    std::fs::write(&script, format!("local _ = 1\n{was}")).unwrap();
    git(&root, &["commit", "-q", "-am", "second"]);
    let (second, took) = build(&root, &dir.join("b"), Some(&dir.join("a")));

    assert_ne!(first["code"], second["code"], "the code version moved");
    assert!(
        second["check"]["same"] == true,
        "the build plays as the project does: {}",
        second["check"]
    );
    let new = second["packs"]["blobs_new"].as_u64().unwrap();
    assert!(
        (1..=4).contains(&new),
        "only what changed ships: {new} new blobs of {}",
        second["packs"]["files"]
    );
    assert!(
        took < BOUND,
        "the incremental release build took {took:?} (section 17: under {BOUND:?})"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
