//! Issue 30: a system file that does not load (past Luau's 200 top-level
//! locals) is named, first, with its own line, in script.errors, the
//! summary, and every answer that runs the game; a system that runs after
//! one of its systems runs unordered and says why, instead of the game
//! refusing to start with an error that blames it.

mod common;

use common::*;
use serde_json::json;
use std::path::Path;

fn project(root: &Path) {
    write(
        root,
        "argus.toml",
        "name = \"locals\"\nclock = \"continuous@60\"\ndefault_scene = \"main\"\nplugins = []\n",
    );
    let mut big: Vec<String> = (0..210)
        .map(|n| format!("local function f{n}(x) return x + {n} end"))
        .collect();
    big.push(
        "return system \"sim.big\" {\n  stage = \"sim\",\n  run = function(q) local _ = f1(1) end,\n}\n"
            .to_string(),
    );
    write(root, "systems/big.luau", &big.join("\n"));
    write(
        root,
        "systems/after.luau",
        "return system \"sim.after\" {\n  stage = \"sim\",\n  after = { \"sim.big\" },\n  run = function(q) end,\n}\n",
    );
    write(
        root,
        "scenes/main/scene.toml",
        "clock = \"continuous@60\"\nseed = 1\n",
    );
}

#[test]
fn a_file_that_does_not_load_is_named_first_and_its_dependents_run() {
    let t = temp("load-errors");
    project(t.path());
    // Opens: the session says how many scripts went wrong.
    let mut d = argus_daemon::Daemon::new();
    let r = ok(
        &mut d,
        "session.open",
        json!({ "project": t.path().to_string_lossy() }),
    );
    assert_eq!(r["script_errors"], 2, "{r}");

    // The game starts and runs, without sim.big, saying so.
    let s = ok(&mut d, "sim.step", json!({ "n": 1 }));
    assert_eq!(s["script_errors"], 2, "{s}");
    let errs = ok(&mut d, "script.errors", json!({}));
    let rows = errs["rows"].as_array().unwrap();
    assert_eq!(rows[0]["file"], "systems/big.luau", "{errs}");
    assert_eq!(rows[0]["line"], 201);
    assert!(
        rows[0]["message"]
            .as_str()
            .unwrap()
            .contains("Out of local registers"),
        "{errs}"
    );
    let order = rows[1]["message"].as_str().unwrap();
    assert!(
        order.contains("sim.after runs after sim.big")
            && order.contains("systems/big.luau:201 did not load"),
        "{order}"
    );
    let sum = ok(&mut d, "world.summary", json!({}));
    assert_eq!(sum["script_errors"]["total"], 2);
    assert!(sum["script_errors"]["first"][0]
        .as_str()
        .unwrap()
        .starts_with("systems/big.luau:201:"));
    assert!(sum["systems"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n == "sim.after"));
}
