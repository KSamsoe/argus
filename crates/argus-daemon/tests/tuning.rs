//! The tuning log (§3, §10, phase 2 item 5) through the protocol: a tuning
//! session's slider moves logged with their frames and committed as one
//! transaction at its end; commits, undo, and picked proposals logged too;
//! previews and tests not; the stream to a subscriber.

mod common;

use argus_daemon::Daemon;
use common::*;
use serde_json::{json, Value as Json};
use std::path::Path;

/// The fixture with a feel field on the player, as §10's example has it.
fn project_with_feel(root: &Path) {
    project(root);
    write(
        root,
        "components/feel.luau",
        "component \"HitStop\" { doc = \"Freeze on a hit\", scale = field.f32 { default = 0, range = {0, 1}, tune = true, doc = \"How much\" } }\n",
    );
    let player = read(root, "scenes/arena/player.toml") + "\n[HitStop]\n";
    write(root, "scenes/arena/player.toml", &player);
}

fn scale(d: &mut Daemon) -> f64 {
    ok(d, "world.get", json!({ "entity": "@player" }))["components"]["HitStop"]["scale"]
        .as_f64()
        .unwrap()
}

fn tick(d: &mut Daemon) -> u64 {
    ok(d, "sim.status", json!({}))["tick"].as_u64().unwrap()
}

#[test]
fn a_tuning_session_logs_the_search_path_and_commits_the_end() {
    let t = temp("tuning-session");
    project_with_feel(t.path());
    let mut d = open(t.path());
    let before = read(t.path(), "scenes/arena/player.toml");
    let fields = ok(&mut d, "tune.fields", json!({ "entity": "@player" }));
    assert_eq!(fields["rows"][0]["path"], "HitStop.scale", "{fields}");
    assert_eq!(fields["rows"][0]["range"], json!([0.0, 1.0]));

    // The slider moves: the live world only, each logged with its frame.
    let mut frames = Vec::new();
    for (n, v) in [(10, 0.15), (79, 0.08), (50, 0.1)] {
        ok(&mut d, "sim.step", json!({ "n": n }));
        frames.push(tick(&mut d));
        let r = ok(
            &mut d,
            "tune.set",
            json!({ "entity": "@player", "path": "HitStop.scale", "value": v }),
        );
        assert_eq!(r["session"], 1);
    }
    assert!((scale(&mut d) - 0.1).abs() < 1e-6);
    assert_eq!(
        read(t.path(), "scenes/arena/player.toml"),
        before,
        "no file yet"
    );
    let log = ok(&mut d, "tune.log", json!({}));
    assert_eq!(
        log["story"],
        json!([format!(
            "human set @player HitStop.scale 0→0.15 at frame {}, →0.08 at frame {}, →0.1 at frame {}",
            frames[0], frames[1], frames[2]
        )]),
        "{log}"
    );

    // Out of range, or not a tune field: refused.
    let e = err(
        &mut d,
        "tune.set",
        json!({ "entity": "@player", "path": "HitStop.scale", "value": 3 }),
    );
    assert_eq!(e.code, argus_ecs::codes::OUT_OF_RANGE);
    let e = err(
        &mut d,
        "tune.set",
        json!({ "entity": "@player", "path": "Health.cur", "value": 1 }),
    );
    assert!(e.message.contains("not a tune field"), "{}", e.message);

    // The end: one transaction by the human, in the feed and the file.
    let end = ok(&mut d, "tune.end", json!({ "note": "hit-stop by feel" }));
    assert_eq!(end["fields"], 1);
    assert_eq!(end["commit"]["author"], "human");
    assert_eq!(end["commit"]["note"], "hit-stop by feel");
    assert!(read(t.path(), "scenes/arena/player.toml").contains("scale = 0.1"));
    let feed = ok(&mut d, "edit.feed", json!({}));
    assert_eq!(
        feed["rows"].as_array().unwrap().len(),
        1,
        "one transaction: {feed}"
    );
    // One undo goes back to where the session started.
    ok(&mut d, "edit.undo", json!({ "author": "human" }));
    assert_eq!(scale(&mut d), 0.0);
    let rows = ok(&mut d, "tune.log", json!({}))["rows"]
        .as_array()
        .unwrap()
        .clone();
    let kinds: Vec<(&str, &str)> = rows
        .iter()
        .map(|r| (r["kind"].as_str().unwrap(), r["via"].as_str().unwrap_or("")))
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("begin", ""),
            ("set", "tune"),
            ("set", "tune"),
            ("set", "tune"),
            ("end", ""),
            ("set", "undo")
        ],
        "{rows:?}"
    );
    assert_eq!(
        rows[4]["values"],
        json!([["@player", "HitStop.scale", 0.0, rows[3]["to"]]])
    );

    // A session put back, not committed.
    ok(
        &mut d,
        "tune.set",
        json!({ "entity": "@player", "path": "HitStop.scale", "value": 0.5 }),
    );
    let end = ok(&mut d, "tune.end", json!({ "commit": false }));
    assert!(end["commit"].is_null());
    assert_eq!(scale(&mut d), 0.0);
    assert_eq!(
        ok(&mut d, "tune.log", json!({ "session": 2 }))["rows"]
            .as_array()
            .unwrap()
            .len(),
        3
    );

    // The log is a file: another daemon reads it.
    drop(d);
    let mut d = open(t.path());
    assert_eq!(ok(&mut d, "tune.log", json!({}))["total"], 9);
}

#[test]
fn commits_and_picks_are_logged_and_previews_and_tests_are_not() {
    let t = temp("tuning-sources");
    project_with_feel(t.path());
    let mut d = open(t.path());
    d.call(7, "obs.subscribe", &json!({ "topics": ["tune"] }))
        .unwrap();

    // An edit commit of a tune field.
    let tx = ok(
        &mut d,
        "edit.begin",
        json!({ "author": "agent", "note": "a little" }),
    )["tx"]
        .clone();
    ok(
        &mut d,
        "edit.set",
        json!({ "tx": tx, "entity": "@player", "path": "HitStop.scale", "value": 0.2 }),
    );
    ok(
        &mut d,
        "edit.set",
        json!({ "tx": tx, "entity": "@player", "path": "Health.cur", "value": 2 }),
    );
    ok(&mut d, "edit.commit", json!({ "tx": tx }));
    // A proposal: its check on a fork is not logged; the pick is.
    let tx = ok(
        &mut d,
        "edit.begin",
        json!({ "author": "agent", "note": "more" }),
    )["tx"]
        .clone();
    ok(
        &mut d,
        "edit.set",
        json!({ "tx": tx, "entity": "@player", "path": "HitStop.scale", "value": 0.3 }),
    );
    ok(&mut d, "edit.propose", json!({ "tx": tx }));
    assert_eq!(
        ok(&mut d, "tune.log", json!({}))["total"],
        1,
        "a proposal changes nothing"
    );
    ok(&mut d, "proposal.try", json!({ "id": 1 }));
    ok(&mut d, "proposal.show", json!({ "variant": "B" }));
    ok(&mut d, "proposal.pick", json!({ "id": 1, "variant": "B" }));
    // A test's edits are not tuning.
    write(
        t.path(),
        "tests/feel.luau",
        r#"return scenario "hit-stop off" {
  scene = "arena", seed = 7,
  run = function(t)
    local tx = t:call("edit.begin", { author = "test", note = "off" }).tx
    t:call("edit.set", { tx = tx, entity = "@player", path = "HitStop.scale", value = 0 })
    t:call("edit.commit", { tx = tx })
    t:step(1)
    t.expect:field("@player", "HitStop.scale"):eq(0)
  end,
}
"#,
    );
    assert_eq!(ok(&mut d, "test.run", json!({}))["ok"], true);

    let log = ok(&mut d, "tune.log", json!({}));
    let rows = log["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{log}");
    assert_eq!(
        (rows[0]["author"].clone(), rows[0]["via"].clone()),
        (json!("agent"), json!("edit"))
    );
    assert_eq!(rows[1]["author"], "human");
    assert!(
        rows[1]["note"]
            .as_str()
            .unwrap()
            .starts_with("proposal 1 B picked"),
        "{}",
        rows[1]
    );
    assert_eq!(
        log["story"].as_array().unwrap().len(),
        2,
        "one line an author and field"
    );

    // The subscriber got each row as it came.
    let got: Vec<&Json> = d
        .outbox
        .iter()
        .filter(|(c, n)| *c == 7 && n["method"] == "tune.changed")
        .map(|(_, n)| n)
        .collect();
    assert_eq!(got.len(), 2);
    assert_eq!(got[0]["params"]["to"], json!(0.2f32 as f64));
}

#[test]
fn loading_a_scene_ends_a_tuning_session_and_keeps_it() {
    let t = temp("tuning-load");
    project_with_feel(t.path());
    let mut d = open(t.path());
    ok(
        &mut d,
        "tune.set",
        json!({ "entity": "@player", "path": "HitStop.scale", "value": 0.4 }),
    );
    ok(&mut d, "scene.load", json!({ "scene": "arena" }));
    assert!(
        (scale(&mut d) - 0.4).abs() < 1e-6,
        "committed, then loaded from the file"
    );
    assert!(ok(&mut d, "tune.fields", json!({}))["session"].is_null());
    assert!(read(t.path(), "scenes/arena/player.toml").contains("scale = 0.4"));
}
