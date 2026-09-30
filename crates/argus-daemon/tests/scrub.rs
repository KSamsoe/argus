//! What the editor's replay scrubber reads and does (phase 4 item 5):
//! `sim.ring` is the run to draw, with no keyframe taken; a drag's seeks are
//! one row in the change feed; listing the worlds is not a change; and
//! `replay.list` finds the replays a failing test saves, for the editor to
//! open.

mod common;

use argus_daemon::Daemon;
use common::*;
use serde_json::{json, Value as Json};

fn call(d: &mut Daemon, conn: u64, method: &str, params: Json) -> Json {
    match d.call(conn, method, &params) {
        Ok(v) => v,
        Err(e) => panic!("{method} failed: {e}"),
    }
}

fn ticks(v: &Json, key: &str) -> Vec<u64> {
    v[key]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Json::as_u64)
        .collect()
}

fn seeks(d: &mut Daemon) -> Vec<Json> {
    ok(d, "edit.feed", json!({ "limit": 500 }))["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["method"] == "sim.seek")
        .cloned()
        .collect()
}

#[test]
fn the_ring_is_the_run_to_draw_and_a_drag_is_one_row() {
    let t = temp("scrub-ring");
    project(t.path());
    let mut d = open(t.path());
    let (human, agent, editor) = (1, 2, 3);
    call(
        &mut d,
        human,
        "session.identify",
        json!({ "author": "human" }),
    );
    call(
        &mut d,
        agent,
        "session.identify",
        json!({ "author": "agent:claude" }),
    );
    call(
        &mut d,
        editor,
        "session.identify",
        json!({ "author": "human" }),
    );
    call(
        &mut d,
        editor,
        "obs.subscribe",
        json!({ "topics": ["edits"] }),
    );
    ok(
        &mut d,
        "input.inject",
        json!({ "action": "move", "value": [1, 0], "frames": 600 }),
    );
    ok(&mut d, "sim.step", json!({ "n": 300 }));
    // An edit at 300: a change between ticks, a barrier.
    commit(
        &mut d,
        "agent:claude",
        "tougher",
        &[(
            "edit.set",
            json!({ "entity": "@player", "path": "Health.max", "value": 5 }),
        )],
    );
    ok(&mut d, "sim.step", json!({ "n": 300 }));

    let before = ok(&mut d, "sim.ring", json!({}));
    assert_eq!(
        (
            before["floor"].clone(),
            before["end"].clone(),
            before["tick"].clone()
        ),
        (json!(0), json!(600), json!(600))
    );
    assert_eq!(before["driver"], "continuous");
    assert_eq!(before["hz"], 60.0);
    assert_eq!(before["dense_from"], 0, "all of it within five minutes");
    let keys = ticks(&before, "ticks");
    assert!(
        keys.contains(&0) && keys.contains(&540) && keys.contains(&600),
        "{keys:?}"
    );
    let barriers = ticks(&before, "barrier_ticks");
    assert!(
        barriers.contains(&0) && barriers.contains(&300),
        "{barriers:?}"
    );
    assert_eq!(before["fork"], "main");
    assert_eq!(before["worlds"][0]["name"], "main");
    // Reading it takes no keyframe (sim.snapshot does).
    let again = ok(&mut d, "sim.ring", json!({}));
    assert_eq!(again["keyframes"], before["keyframes"]);
    assert_eq!(again["run"], before["run"]);

    // A drag back: three seeks by the person, one row, announced each time.
    d.outbox.clear();
    for f in [450, 320, 150] {
        call(&mut d, human, "sim.seek", json!({ "frame": f }));
    }
    let rows = seeks(&mut d);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["command"], "argus sim seek frame=150");
    assert_eq!(rows[0]["author"], "human");
    let heard: Vec<&Json> = d
        .outbox
        .iter()
        .filter(|(c, n)| *c == editor && n["method"] == "edit.command")
        .map(|(_, n)| n)
        .collect();
    assert_eq!(heard.len(), 3);
    assert!(heard.iter().all(|n| n["params"]["id"] == rows[0]["id"]));
    // The run is still there to go forward along.
    let back = ok(&mut d, "sim.ring", json!({}));
    assert_eq!(
        (back["tick"].clone(), back["end"].clone()),
        (json!(150), json!(600))
    );
    assert_eq!(back["run"], before["run"]);

    // Another author's seek, or anything between, is a row of its own.
    call(&mut d, agent, "sim.seek", json!({ "frame": 200 }));
    call(&mut d, human, "sim.seek", json!({ "frame": 210 }));
    call(&mut d, human, "sim.step", json!({ "n": 1 }));
    call(&mut d, human, "sim.seek", json!({ "frame": 100 }));
    let rows = seeks(&mut d);
    let lines: Vec<&str> = rows
        .iter()
        .map(|r| r["command"].as_str().unwrap())
        .collect();
    assert_eq!(
        lines,
        vec![
            "argus sim seek frame=150",
            "argus sim seek frame=200",
            "argus sim seek frame=210",
            "argus sim seek frame=100"
        ]
    );

    // Listing the worlds is a look, not a change; making one is.
    let n = ok(&mut d, "edit.feed", json!({ "limit": 500 }))["rows"]
        .as_array()
        .unwrap()
        .len();
    call(&mut d, human, "sim.fork", json!({}));
    let feed = ok(&mut d, "edit.feed", json!({ "limit": 500 }));
    assert_eq!(feed["rows"].as_array().unwrap().len(), n, "{feed}");
    call(
        &mut d,
        human,
        "sim.fork",
        json!({ "name": "try-1", "switch": true }),
    );
    let feed = ok(&mut d, "edit.feed", json!({ "limit": 500 }));
    assert_eq!(feed["rows"].as_array().unwrap().len(), n + 1);
    let fork = ok(&mut d, "sim.ring", json!({}));
    assert_eq!(fork["fork"], "try-1");
    assert_ne!(fork["run"], before["run"], "another world, another run");
    assert_eq!(
        (fork["floor"].clone(), fork["tick"].clone()),
        (json!(100), json!(100)),
        "the fork starts where it was made"
    );
    assert_eq!(fork["worlds"].as_array().unwrap().len(), 2);
    call(
        &mut d,
        human,
        "sim.fork",
        json!({ "name": "main", "switch": true }),
    );
    let main = ok(&mut d, "sim.ring", json!({}));
    assert_eq!(main["run"], before["run"], "back to the run it was");
    assert_eq!(
        (main["tick"].clone(), main["end"].clone()),
        (json!(100), json!(600))
    );

    // New input from here branches the run: its end comes back to here.
    ok(
        &mut d,
        "input.inject",
        json!({ "action": "move", "value": [-1, 0], "frames": 5 }),
    );
    ok(&mut d, "sim.step", json!({ "n": 5 }));
    let branched = ok(&mut d, "sim.ring", json!({}));
    assert_eq!(branched["end"], 105);
    assert_eq!(branched["run"], before["run"], "the same run, shorter");

    // A scene load is another run.
    ok(&mut d, "scene.load", json!({ "scene": "arena" }));
    let fresh = ok(&mut d, "sim.ring", json!({}));
    assert_ne!(fresh["run"], before["run"]);
    assert_eq!(fresh["end"], 0);
}

#[test]
fn a_discrete_ring_counts_turns() {
    let t = temp("scrub-discrete");
    project(t.path());
    let mut d = open(t.path());
    ok(&mut d, "scene.load", json!({ "scene": "delve" }));
    ok(&mut d, "sim.advance", json!({ "n": 25 }));
    let r = ok(&mut d, "sim.ring", json!({}));
    assert_eq!(r["driver"], "discrete");
    assert!(r["hz"].is_null());
    assert_eq!(r["end"], 25);
    assert!(ticks(&r, "ticks").contains(&20), "every ten turns: {r}");
}

#[test]
fn a_failing_tests_replay_is_listed_for_the_editor_to_open() {
    let t = temp("scrub-replays");
    project(t.path());
    let mut d = open(t.path());
    assert_eq!(ok(&mut d, "replay.list", json!({}))["total"], 0);
    ok(&mut d, "sim.step", json!({ "n": 120 }));
    // As the harness saves one: .argus/replays/<test>/seed<n>/.
    ok(
        &mut d,
        "replay.save",
        json!({
            "dir": ".argus/replays/a_shot_lands/seed3",
            "note": "expected 1 shot, got 0",
            "about": { "scenario": "a shot lands", "file": "tests/shot.luau", "seed": 3 },
            "clip": false,
        }),
    );
    ok(&mut d, "sim.step", json!({ "n": 60 }));
    ok(
        &mut d,
        "replay.save",
        json!({ "dir": ".argus/replays/by-hand", "clip": false }),
    );
    let l = ok(&mut d, "replay.list", json!({}));
    assert_eq!(l["total"], 2, "{l}");
    let test = l["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["path"] == ".argus/replays/a_shot_lands/seed3")
        .cloned()
        .expect("the test's, by its path");
    assert_eq!(test["about"]["scenario"], "a shot lands");
    assert_eq!(test["note"], "expected 1 shot, got 0");
    assert_eq!(test["frame"], 120);
    assert_eq!(test["seed"], 7);
    assert_eq!(
        ok(&mut d, "replay.list", json!({ "limit": 1 }))["rows"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // Opened by that path: paused at its frame, the lead-in to scrub.
    let o = ok(&mut d, "replay.open", json!({ "path": test["path"] }));
    assert_eq!(o["frame"], 120);
    let r = ok(&mut d, "sim.ring", json!({}));
    assert_eq!(r["tick"], 120);
    assert!(r["floor"].as_u64().unwrap() <= 60, "the lead-in: {r}");
    assert_eq!(r["end"], 120);
}
