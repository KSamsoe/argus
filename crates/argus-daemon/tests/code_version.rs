//! The code version a run, a mark, and a build carry (issue 7): uncommitted
//! changes are named by a hash of what the changed files hold, so two
//! builds with different changes say different things, and a change to a
//! file no run reads is no change.

mod common;

use argus_daemon::daemon::code_version;
use std::path::Path;
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

#[test]
fn uncommitted_changes_are_named_by_what_the_game_files_hold() {
    let t = common::temp("code-version");
    let root = t.path();
    let put = |rel: &str, text: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    put(
        "argus.toml",
        "name = \"versions\"\nclock = \"continuous@60\"\n\n[build]\nexclude = [\"m0\"]\n",
    );
    put("systems/a.luau", "-- a\n");
    put("systems/b.luau", "-- b\n");
    put("README.md", "the game\n");
    put(".gitignore", ".argus/\n");
    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "first"]);
    let rev = git(root, &["rev-parse", "--short", "HEAD"]);
    assert_eq!(code_version(root), format!("git:{rev}"), "committed");

    put("systems/a.luau", "-- a, changed\n");
    let one = code_version(root);
    let dirty = format!("git:{rev}+dirty.");
    assert!(one.starts_with(&dirty), "{one}");
    assert_eq!(one.len(), dirty.len() + 8, "eight hex digits: {one}");
    assert_eq!(code_version(root), one, "the same changes say the same");

    // Files no run reads: the daemon's own folder, a build, the notes, and
    // what [build] exclude names.
    put(".argus/marks/1/mark.json", "{}");
    put("dist/web/index.html", "<p>");
    put("README.md", "the game, now with notes\n");
    put("m0/notes.md", "the first try");
    assert_eq!(code_version(root), one, "none of these is the game");

    // A different change, a new file, a staged one, a deleted one: each is
    // another version.
    put("systems/a.luau", "-- a, changed again\n");
    let two = code_version(root);
    assert_ne!(two, one, "another change to the same file");
    put("systems/c.luau", "-- c\n");
    let three = code_version(root);
    assert_ne!(three, two, "a new file");
    git(root, &["add", "systems/c.luau"]);
    assert_eq!(code_version(root), three, "staged or not, the same files");
    std::fs::remove_file(root.join("systems/b.luau")).unwrap();
    let four = code_version(root);
    assert_ne!(four, three, "a deleted file");

    // Back as committed: clean again.
    git(root, &["reset", "-q", "--hard"]);
    std::fs::remove_dir_all(root.join("m0")).unwrap();
    std::fs::remove_dir_all(root.join("dist")).unwrap();
    assert_eq!(code_version(root), format!("git:{rev}"));
}
