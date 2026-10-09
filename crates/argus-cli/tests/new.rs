//! Distribution item 2: `argus new` makes a working project from a
//! template, its scenarios passing, with ARGUS.md, AGENTS.md, and
//! CLAUDE.md for an agent to start from.

use serde_json::Value as Json;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("argus-new-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn new(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_argus"))
        .arg("new")
        .args(args)
        .env(
            "ARGUS_TEMPLATES",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../templates"),
        )
        .output()
        .unwrap()
}

#[test]
fn a_new_project_from_the_blank_template_passes_its_scenarios_and_points_an_agent_at_argus_md() {
    let dir = temp("blank").join("lantern");
    let o = new(&[dir.to_str().unwrap(), "--json"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let r: Json = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(r["name"], "lantern", "{r}");
    assert_eq!(r["template"], "blank", "{r}");
    let total = r["tests"]["total"].as_u64().unwrap();
    assert!(total >= 1, "{r}");
    assert_eq!(r["tests"]["passed"].as_u64().unwrap(), total, "{r}");

    let toml = std::fs::read_to_string(dir.join("argus.toml")).unwrap();
    assert!(toml.starts_with("name = \"lantern\"\n"), "{toml}");
    assert!(
        !dir.join("template.toml").exists(),
        "template.toml stays with the template"
    );
    let md = std::fs::read_to_string(dir.join("ARGUS.md")).unwrap();
    assert!(md.starts_with("# ARGUS.md: lantern"), "{}", &md[..200]);
    assert!(md.contains("Player"), "ARGUS.md has the game's components");
    let agents = std::fs::read_to_string(dir.join("AGENTS.md")).unwrap();
    assert!(
        agents.contains("ARGUS.md") && agents.contains("argus test"),
        "{agents}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap(),
        "@AGENTS.md\n"
    );
    // A repository, when git is here: a build's code revision is its commit.
    let git = Command::new("git").arg("--version").output().is_ok();
    assert_eq!(r["git"], git, "{r}");
    assert_eq!(dir.join(".git").is_dir(), git);
}

#[test]
fn a_new_project_goes_in_a_new_or_empty_directory() {
    let dir = temp("full");
    std::fs::write(dir.join("notes.txt"), "mine").unwrap();
    let o = new(&[dir.to_str().unwrap(), "--no-git"]);
    assert!(!o.status.success());
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(e.contains("not empty"), "{e}");
    assert_eq!(
        std::fs::read_to_string(dir.join("notes.txt")).unwrap(),
        "mine"
    );
}

#[test]
fn an_unknown_template_is_refused_with_the_ones_there_are() {
    let dir = temp("unknown").join("x");
    let o = new(&[dir.to_str().unwrap(), "--template", "mmo", "--no-git"]);
    assert!(!o.status.success());
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(
        e.contains("no template \"mmo\"") && e.contains("blank ("),
        "{e}"
    );
    assert!(!dir.exists(), "nothing is made");
}

#[test]
fn the_name_can_differ_from_the_directory() {
    let dir = temp("named").join("game");
    let o = new(&[
        dir.to_str().unwrap(),
        "--name",
        "Night Lantern",
        "--no-git",
        "--json",
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let toml = std::fs::read_to_string(dir.join("argus.toml")).unwrap();
    assert!(toml.starts_with("name = \"Night Lantern\"\n"), "{toml}");
    assert!(!dir.join(".git").exists());
}

#[test]
fn every_template_makes_a_project_whose_scenarios_pass() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../templates");
    let mut names: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .filter(|e| e.path().join("template.toml").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["arena", "blank", "colony", "platformer", "roguelike"],
        "the templates argus new offers"
    );
    let base = temp("every");
    for t in &names {
        let dir = base.join(t);
        let o = new(&[dir.to_str().unwrap(), "--template", t, "--no-git", "--json"]);
        assert!(
            o.status.success(),
            "{t}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        let r: Json = serde_json::from_slice(&o.stdout).unwrap();
        let total = r["tests"]["total"].as_u64().unwrap();
        assert!(total >= 1, "{t} has a scenario: {r}");
        assert_eq!(r["tests"]["passed"].as_u64().unwrap(), total, "{t}: {r}");
        // A start is small: no template carries a game's worth of assets.
        let bytes: u64 = walk(&root.join(t));
        assert!(bytes < 300 * 1024, "{t} is {bytes} bytes");
    }
}

fn walk(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| {
            if e.path().is_dir() {
                walk(&e.path())
            } else {
                e.metadata().map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}
