//! Phase 5 item 4 (§3): "Changing a prefab is a transaction like any other;
//! the index lists every instance, and the change feed reports how many
//! entities the change touched." A prefab's field changed through the
//! protocol: its file written with its comments kept, every instance given
//! the new value at once (one that overrides the field keeps its own), a
//! running game keeping the rest of each instance's state, the feed naming
//! every instance touched, undo putting the file back byte for byte.

mod common;

use common::*;
use serde_json::{json, Value as Json};

fn health(d: &mut argus_daemon::Daemon, id: &str) -> Json {
    ok(d, "world.get", json!({ "entity": id }))["components"]["Health"].clone()
}

#[test]
fn a_prefab_changed_through_the_protocol_reaches_every_instance() {
    let t = temp("prefab-edit");
    project(t.path());
    write(
        t.path(),
        "prefabs/enemy/bat.toml",
        "# Bats: a weak flyer.\ntags = [\"Enemy\"]\n\n[Health]\ncur = 2 # starts full\nmax = 2\n",
    );
    // A second bat whose own file overrides the maximum.
    write(
        t.path(),
        "scenes/arena/bat.big.toml",
        "prefab = \"enemy/bat\"\n\n[Transform]\npos = [6.0, 0.0]\n\n[Health]\nmax = 9\n",
    );
    let mut d = open(t.path());
    let file = t.path().join("prefabs/enemy/bat.toml");
    let before = std::fs::read_to_string(&file).unwrap();

    // The game runs a tick: every enemy loses a hit point.
    ok(&mut d, "sim.step", json!({ "n": 1 }));
    assert_eq!(health(&mut d, "@bat")["cur"], 1);

    // The prefab's maximum, changed.
    let row = commit(
        &mut d,
        "agent:claude",
        "tougher bats",
        &[(
            "edit.prefab",
            json!({ "prefab": "enemy/bat", "path": "Health.max", "value": 5 }),
        )],
    );
    let touched: Vec<&str> = row["touched"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Json::as_str)
        .collect();
    assert!(
        touched.contains(&"@bat") && touched.contains(&"@bat.big"),
        "every instance in the feed: {row}"
    );
    assert!(touched.contains(&"prefab:enemy/bat"), "{row}");
    assert_eq!(row["files"], json!(["prefabs/enemy/bat.toml"]));
    assert_eq!(
        row["ops"][0],
        json!(["prefab", "enemy/bat", "Health.max", 5])
    );
    // The bat has the new maximum and keeps its wound; the big bat keeps
    // its own maximum.
    assert_eq!(health(&mut d, "@bat"), json!({ "cur": 1, "max": 5 }));
    assert_eq!(health(&mut d, "@bat.big")["max"], 9);
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("max = 5"), "{text}");
    assert!(
        text.contains("# Bats: a weak flyer.") && text.contains("cur = 2 # starts full"),
        "comments kept: {text}"
    );
    // The index lists both instances.
    let inst = ok(&mut d, "world.instances", json!({ "prefab": "enemy/bat" }));
    assert_eq!(inst["total"], 2, "{inst}");
    // world.get's provenance names the prefab for the bat's maximum.
    let g = ok(
        &mut d,
        "world.get",
        json!({ "entity": "@bat", "resolved": true }),
    );
    assert!(g.to_string().contains("enemy/bat"), "{g}");

    // Undo: the file as it was, byte for byte, and the old maximum.
    ok(&mut d, "edit.undo", json!({ "author": "agent:claude" }));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
    assert_eq!(health(&mut d, "@bat")["max"], 2);

    // Taken out: the default shows (the fixture's Health.max is 10).
    commit(
        &mut d,
        "agent:claude",
        "",
        &[(
            "edit.prefab",
            json!({ "prefab": "enemy/bat", "path": "Health.max", "remove": true }),
        )],
    );
    assert_eq!(health(&mut d, "@bat")["max"], 10);
    assert!(!std::fs::read_to_string(&file).unwrap().contains("max ="));
    ok(&mut d, "edit.undo", json!({ "author": "agent:claude" }));

    // Refused when queued: a prefab there is not, a field there is not.
    let tx = ok(&mut d, "edit.begin", json!({ "author": "agent:claude" }))["tx"].clone();
    let e = err(
        &mut d,
        "edit.prefab",
        json!({ "tx": tx, "prefab": "enemy/owl", "path": "Health.max", "value": 1 }),
    );
    assert!(e.message.contains("enemy/owl"), "{}", e.message);
    let e = err(
        &mut d,
        "edit.prefab",
        json!({ "tx": tx, "prefab": "enemy/bat", "path": "Health.wings", "value": 1 }),
    );
    assert!(e.message.contains("wings"), "{}", e.message);
    ok(&mut d, "edit.abort", json!({ "tx": tx }));

    // A person's lock on the prefab refuses the agent.
    ok(
        &mut d,
        "edit.lock",
        json!({ "author": "human", "prefabs": ["enemy/bat"], "note": "tuning by hand" }),
    );
    let tx = ok(&mut d, "edit.begin", json!({ "author": "agent:claude" }))["tx"].clone();
    ok(
        &mut d,
        "edit.prefab",
        json!({ "tx": tx, "prefab": "enemy/bat", "path": "Health.max", "value": 7 }),
    );
    let e = err(&mut d, "edit.commit", json!({ "tx": tx }));
    assert!(e.message.contains("tuning by hand"), "{}", e.message);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
}
