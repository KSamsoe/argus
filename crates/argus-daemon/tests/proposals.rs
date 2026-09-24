//! Proposals and A/B by feel (§10, phase 2 item 4) through the protocol: a
//! transaction proposed instead of committed, its variants tried as worlds
//! that take the same input at the same tick, one picked (an ordinary
//! commit that undo takes back) or all turned down.

mod common;

use argus_daemon::Daemon;
use common::*;
use serde_json::{json, Value as Json};
use std::path::Path;

/// The fixture, walking at a tune field's speed.
fn project_with_speed(root: &Path) {
    project(root);
    write(
        root,
        "components/speed.luau",
        "component \"Speed\" { doc = \"Units a tick\", v = field.f32 { default = 2, tune = true } }\n",
    );
    write(
        root,
        "systems/move.luau",
        r#"system "sim.move" {
  stage = "sim",
  reads = { "PlayerSlot", "Actions", "Speed" },
  writes = { "Transform" },
  doc = "Walks the player by its move action at its speed.",
  run = function(q)
    for e, t, a, s in q:each("Transform", "Actions", "Speed") do
      local m = a.move
      local p = t.pos
      t.pos = { x = p.x + m.x * s.v, y = p.y + m.y * s.v }
    end
  end,
}
"#,
    );
    let player = read(root, "scenes/arena/player.toml") + "\n[Speed]\nv = 2.0\n";
    write(root, "scenes/arena/player.toml", &player);
}

/// Propose `Speed.v = v` for the player, into `into` or a new proposal.
fn propose(d: &mut Daemon, v: f64, note: &str, into: Option<u64>) -> Json {
    let tx = ok(d, "edit.begin", json!({ "author": "agent", "note": note }))["tx"].clone();
    ok(
        d,
        "edit.set",
        json!({ "tx": tx, "entity": "@player", "path": "Speed.v", "value": v }),
    );
    let mut p = json!({ "tx": tx });
    if let Some(id) = into {
        p["proposal"] = json!(id);
    } else {
        p["note"] = json!("walk faster");
    }
    ok(d, "edit.propose", p)
}

fn x(d: &mut Daemon) -> f64 {
    ok(d, "world.get", json!({ "entity": "@player" }))["components"]["Transform"]["pos"][0]
        .as_f64()
        .unwrap()
}

fn speed(d: &mut Daemon) -> f64 {
    ok(d, "world.get", json!({ "entity": "@player" }))["components"]["Speed"]["v"]
        .as_f64()
        .unwrap()
}

#[test]
fn a_proposal_changes_nothing_until_it_is_picked() {
    let t = temp("proposals-make");
    project_with_speed(t.path());
    let mut d = open(t.path());
    let before = read(t.path(), "scenes/arena/player.toml");
    // A lock held before: checking a proposal on a fork must not lose it.
    ok(
        &mut d,
        "edit.lock",
        json!({ "author": "human", "note": "mine", "paths": ["Health"] }),
    );
    let b = propose(&mut d, 3.0, "speed 3", None);
    assert_eq!(
        (b["proposal"].clone(), b["variant"].clone()),
        (json!(1), json!("B"))
    );
    let c = propose(&mut d, 4.0, "speed 4", Some(1));
    assert_eq!(c["variant"], "C");
    assert_eq!(speed(&mut d), 2.0, "the world is as it was");
    assert_eq!(
        read(t.path(), "scenes/arena/player.toml"),
        before,
        "and so are the files"
    );
    assert!(t.path().join(".argus/proposals/1.json").is_file());
    let feed = ok(&mut d, "edit.feed", json!({}));
    let rows = feed["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{feed}");
    assert_eq!(rows[0]["kind"], json!({ "proposed": 1 }));
    assert_eq!(rows[1]["ops"][0], json!(["set", "@player", "Speed.v", 4.0]));
    assert_eq!(
        ok(&mut d, "edit.locks", json!({}))["rows"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "the lock stands"
    );
    let list = ok(&mut d, "proposal.list", json!({}));
    assert_eq!(list["pending"], 1);
    assert_eq!(list["rows"][0]["variants"][1]["note"], "speed 4");

    // One that does not apply is refused, and nothing is kept.
    let tx = ok(
        &mut d,
        "edit.begin",
        json!({ "author": "agent", "note": "bad" }),
    )["tx"]
        .clone();
    ok(
        &mut d,
        "edit.set",
        json!({ "tx": tx, "entity": "@player", "path": "Speed.nope", "value": 1 }),
    );
    let e = err(&mut d, "edit.propose", json!({ "tx": tx }));
    assert_eq!(e.code, argus_daemon::codes::BAD_PROPOSAL);
    assert!(e.message.contains("does not apply"), "{}", e.message);
    assert_eq!(
        ok(&mut d, "proposal.list", json!({ "all": true }))["rows"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        ok(&mut d, "sim.fork", json!({}))["worlds"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "no fork left over"
    );
}

#[test]
fn a_trial_runs_every_variant_on_the_same_input_and_the_pick_is_the_commit() {
    let t = temp("proposals-trial");
    project_with_speed(t.path());
    // Proposed in one daemon, tried in another: proposals are files.
    {
        let mut d = open(t.path());
        propose(&mut d, 3.0, "speed 3", None);
        propose(&mut d, 4.0, "speed 4", Some(1));
    }
    let mut d = open(t.path());
    ok(&mut d, "sim.step", json!({ "n": 10 }));
    let tried = ok(&mut d, "proposal.try", json!({ "id": 1 }));
    assert_eq!(tried["trial"]["showing"], "A");
    assert_eq!(tried["trial"]["worlds"].as_array().unwrap().len(), 3);

    // Play in A: walk right for 20 ticks.
    ok(
        &mut d,
        "input.inject",
        json!({ "action": "move", "value": [1, 0], "frames": 20 }),
    );
    ok(&mut d, "sim.step", json!({ "n": 30 }));
    assert_eq!((x(&mut d), speed(&mut d)), (40.0, 2.0));
    // B caught up to the same tick with the same input, at its own speed.
    ok(&mut d, "proposal.show", json!({ "variant": "B" }));
    assert_eq!(ok(&mut d, "sim.status", json!({}))["tick"], 40);
    assert_eq!((x(&mut d), speed(&mut d)), (60.0, 3.0));
    // Play on in B: the other worlds take the same input.
    ok(
        &mut d,
        "input.inject",
        json!({ "action": "move", "value": [1, 0], "frames": 10 }),
    );
    ok(&mut d, "sim.step", json!({ "n": 10 }));
    assert_eq!(x(&mut d), 90.0);
    ok(&mut d, "proposal.show", json!({ "variant": "C" }));
    assert_eq!((x(&mut d), speed(&mut d)), (120.0, 4.0));
    ok(&mut d, "proposal.show", json!({ "variant": "A" }));
    assert_eq!(x(&mut d), 60.0);
    assert_eq!(ok(&mut d, "sim.status", json!({}))["tick"], 50);

    // The pick is the commit: C's value lands in the file, the game goes on
    // in C, and the other worlds go.
    ok(&mut d, "proposal.show", json!({ "variant": "C" }));
    let picked = ok(&mut d, "proposal.pick", json!({ "id": 1, "variant": "C" }));
    assert_eq!(picked["proposal"]["status"], "picked");
    assert!(read(t.path(), "scenes/arena/player.toml").contains("v = 4.0"));
    assert_eq!((x(&mut d), speed(&mut d)), (120.0, 4.0), "still in C");
    let worlds = ok(&mut d, "sim.fork", json!({}));
    assert_eq!(worlds["active"], "main");
    assert_eq!(worlds["worlds"].as_array().unwrap().len(), 1, "{worlds}");
    let p = ok(&mut d, "proposal.get", json!({ "id": 1 }));
    assert_eq!(p["picked"]["variant"], "C");
    assert_eq!(p["picked"]["by"], "human");
    assert_eq!(p["picked"]["toggles"], 4);
    let feed = ok(&mut d, "edit.feed", json!({}));
    let last = feed["rows"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["kind"], "commit");
    assert_eq!(last["author"], "human");
    assert!(
        last["note"]
            .as_str()
            .unwrap()
            .starts_with("proposal 1 C picked by human"),
        "{last}"
    );
    // And undo takes it back like any commit.
    ok(&mut d, "edit.undo", json!({ "author": "human" }));
    assert!(read(t.path(), "scenes/arena/player.toml").contains("v = 2.0"));
    assert_eq!(speed(&mut d), 2.0);
    // Every switch, with its tick, is kept with the proposal.
    assert_eq!(
        p["picked"]["shown"],
        json!([["B", 40], ["C", 50], ["A", 50], ["C", 50]])
    );
}

#[test]
fn turning_a_proposal_down_leaves_the_world_as_it_was() {
    let t = temp("proposals-reject");
    project_with_speed(t.path());
    let mut d = open(t.path());
    propose(&mut d, 3.0, "speed 3", None);
    let before = read(t.path(), "scenes/arena/player.toml");
    ok(&mut d, "proposal.try", json!({ "id": 1 }));
    ok(&mut d, "proposal.show", json!({ "variant": "B" }));
    assert_eq!(speed(&mut d), 3.0);
    // Keeping A is turning it down.
    let r = ok(&mut d, "proposal.pick", json!({ "id": 1, "variant": "A" }));
    assert_eq!(r["proposal"]["status"], "rejected");
    assert_eq!(speed(&mut d), 2.0, "back in A");
    assert_eq!(read(t.path(), "scenes/arena/player.toml"), before);
    assert_eq!(
        ok(&mut d, "sim.fork", json!({}))["worlds"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let feed = ok(&mut d, "edit.feed", json!({}));
    assert_eq!(
        feed["rows"].as_array().unwrap().last().unwrap()["kind"],
        json!({ "rejected": 1 })
    );
    let e = err(&mut d, "proposal.try", json!({ "id": 1 }));
    assert!(e.message.contains("not pending"), "{}", e.message);

    // A seek ends a trial.
    propose(&mut d, 5.0, "speed 5", None);
    ok(&mut d, "sim.step", json!({ "n": 30 }));
    ok(&mut d, "proposal.try", json!({ "id": 2 }));
    ok(&mut d, "proposal.show", json!({ "variant": "B" }));
    ok(&mut d, "sim.seek", json!({ "frame": 10 }));
    assert!(ok(&mut d, "proposal.list", json!({}))["trial"].is_null());
    assert_eq!(speed(&mut d), 2.0);
}
