//! Action maps through the protocol: inject, script, the log, record and
//! replay (§5 input namespace, §6 "a run is a value", §12).

mod common;

use argus_daemon::Daemon;
use common::*;
use serde_json::json;

#[test]
fn injected_actions_reach_gameplay_and_never_name_keys() {
    let t = temp("input");
    project(t.path());
    let mut d = open(t.path());

    let b = ok(&mut d, "input.bindings", json!({}));
    assert_eq!(b["file"], "input.toml");
    assert_eq!(b["actions"][0]["name"], "fire");
    assert_eq!(
        b["actions"][1]["bindings"]["gamepad"],
        json!(["left_stick"])
    );
    assert_eq!(b["slots"][1]["devices"], json!(["gamepad:1"]));

    // The generated component is a real component: described, queryable, read by a script system.
    let desc = ok(&mut d, "world.describe", json!({ "component": "Actions" }));
    let fields: Vec<&str> = desc["rows"][0]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(fields, ["fire", "fire_pressed", "fire_released", "move"]);
    assert_eq!(desc["rows"][0]["written_by"], json!(["input.actions"]));
    assert_eq!(
        desc["rows"][0]["read_by"],
        json!(["input.actions", "sim.move"]),
        "a writer reads too"
    );
    let sys = ok(&mut d, "script.systems", json!({}));
    assert_eq!(sys["rows"][0]["name"], "input.actions");
    assert_eq!(sys["rows"][0]["stage"], "input");

    // Walk right for 10 ticks, tap fire at tick 3.
    let r = ok(
        &mut d,
        "input.inject",
        json!({ "action": "move", "value": [1, 0], "frames": 10 }),
    );
    assert_eq!(r["tick"], 0);
    assert_eq!(r["until"], 10);
    ok(
        &mut d,
        "input.script",
        json!({ "steps": [{ "at": 3, "action": "fire" }] }),
    );
    ok(&mut d, "sim.step", json!({ "n": 12 }));
    let g = ok(&mut d, "world.get", json!({ "entity": "@player" }));
    assert_eq!(
        g["components"]["Transform"]["pos"],
        json!([20.0, 0.0]),
        "10 ticks at 2 per tick"
    );
    assert_eq!(
        g["components"]["Actions"]["move"],
        json!([0.0, 0.0]),
        "released after 10"
    );
    let shots = ok(&mut d, "obs.events", json!({ "name": "shot" }));
    assert_eq!(shots["total"], 1);
    assert_eq!(shots["rows"][0]["frame"], 3);
    assert_eq!(shots["rows"][0]["fields"]["by"], "@player");

    // The log is actions by tick, never keys.
    let log = ok(&mut d, "input.log.get", json!({}));
    assert_eq!(log["clock"], "tick");
    assert_eq!(log["columns"], json!(["tick", "slot", "action", "value"]));
    assert_eq!(
        log["rows"],
        json!([
            [0, 1, "move", [1.0, 0.0]],
            [3, 1, "fire", true],
            [4, 1, "fire", false],
            [10, 1, "move", [0.0, 0.0]]
        ])
    );
    let log = ok(
        &mut d,
        "input.log.get",
        json!({ "action": "fire", "since": 4 }),
    );
    assert_eq!(log["total"], 1);
    let st = ok(&mut d, "input.state", json!({ "slot": 1 }));
    assert_eq!(st["actions"]["move"], json!([0.0, 0.0]));

    // Errors are documentation.
    let e = err(&mut d, "input.inject", json!({ "action": "jump" }));
    assert_eq!(e.code, argus_input::codes::UNKNOWN_ACTION);
    assert!(e.hint.contains("fire, move"));
    let e = err(
        &mut d,
        "input.inject",
        json!({ "action": "move", "value": true }),
    );
    assert_eq!(e.code, argus_input::codes::BAD_VALUE);
    let e = err(
        &mut d,
        "input.inject",
        json!({ "action": "fire", "slot": 5 }),
    );
    assert_eq!(e.code, argus_input::codes::BAD_SLOT);
    let e = err(&mut d, "input.state", json!({ "slot": 9 }));
    assert_eq!(e.code, argus_input::codes::BAD_SLOT);

    // A project without input.toml says so.
    std::fs::remove_file(t.path().join("input.toml")).unwrap();
    std::fs::remove_file(t.path().join("systems/move.luau")).unwrap();
    write(
        t.path(),
        "scenes/arena/player.toml",
        "[Transform]\npos = [0.0, 0.0]\n\n[Health]\ncur = 3\n",
    );
    let mut d2 = Daemon::new();
    ok(
        &mut d2,
        "session.open",
        json!({ "project": t.path().to_string_lossy() }),
    );
    let e = err(&mut d2, "input.inject", json!({ "action": "fire" }));
    assert_eq!(e.code, argus_input::codes::NO_INPUT_MAP);
    assert!(e.hint.contains("input.toml"));
}

#[test]
fn a_recorded_run_replays_bit_for_bit_and_discrete_runs_log_turns() {
    let t = temp("input-replay");
    project(t.path());
    let mut d = open(t.path());
    let r = ok(&mut d, "input.record", json!({}));
    assert_eq!(r["recording"], true);
    let log_path = r["path"].as_str().unwrap().to_string();
    let run_dir = r["run"].as_str().unwrap().to_string();
    assert!(std::path::Path::new(&run_dir).join("run.toml").exists());
    let run_toml =
        std::fs::read_to_string(std::path::Path::new(&run_dir).join("run.toml")).unwrap();
    assert!(run_toml.contains("scene = \"arena\""), "{run_toml}");
    assert!(run_toml.contains("seed = 7"), "{run_toml}");
    assert!(run_toml.contains("clock = \"continuous@60\""), "{run_toml}");

    ok(&mut d, "sim.play", json!({}));
    ok(
        &mut d,
        "input.inject",
        json!({ "action": "move", "value": [1, 0], "frames": 5 }),
    );
    ok(&mut d, "sim.step", json!({ "n": 2 }));
    ok(
        &mut d,
        "input.inject",
        json!({ "action": "move", "value": [0, 1], "frames": 3 }),
    );
    ok(&mut d, "input.inject", json!({ "action": "fire" }));
    ok(&mut d, "sim.step", json!({ "n": 8 }));
    let r = ok(&mut d, "input.record", json!({ "on": false }));
    assert_eq!(r["recording"], false);
    assert_eq!(r["entries"], 5);
    let lines = std::fs::read_to_string(&log_path).unwrap();
    assert_eq!(lines.lines().count(), 6, "{lines}");
    let world_a = ok(
        &mut d,
        "world.query",
        json!({ "select": ["id", "Transform.pos"], "where": { "has": ["PlayerSlot"] } }),
    );
    let events_a = ok(&mut d, "obs.events", json!({ "name": "shot" }))["total"].clone();

    // Same scene, seed, code, and log: the same world.
    let mut d2 = Daemon::new();
    ok(
        &mut d2,
        "session.open",
        json!({ "project": t.path().to_string_lossy() }),
    );
    let r = ok(&mut d2, "input.replay", json!({ "path": log_path }));
    assert_eq!(r["entries"], 5);
    assert_eq!(r["last_tick"], 5);
    ok(&mut d2, "sim.play", json!({}));
    ok(&mut d2, "sim.step", json!({ "n": 10 }));
    let world_b = ok(
        &mut d2,
        "world.query",
        json!({ "select": ["id", "Transform.pos"], "where": { "has": ["PlayerSlot"] } }),
    );
    assert_eq!(world_a, world_b);
    // Right for ticks 0-1 (2 each), then up for ticks 2-4, then the first hold's release lands on the same tick.
    assert_eq!(world_a["rows"][0][1], json!([4.0, 6.0]));
    assert_eq!(
        ok(&mut d2, "obs.events", json!({ "name": "shot" }))["total"],
        events_a
    );
    assert_eq!(
        ok(&mut d, "input.log.get", json!({}))["rows"],
        ok(&mut d2, "input.log.get", json!({}))["rows"]
    );

    // A discrete scene logs commands by turn.
    write(
        t.path(),
        "scenes/delve/hero.toml",
        "[PlayerSlot]\nslot = 1\n\n[Actions]\n\n[Transform]\n\n[Health]\ncur = 5\n",
    );
    ok(&mut d, "scene.load", json!({ "scene": "delve" }));
    let r = ok(
        &mut d,
        "input.inject",
        json!({ "action": "move", "value": [1, 0], "frames": 50 }),
    );
    assert_eq!(r["until"], 1, "a command lasts one turn");
    ok(&mut d, "sim.advance", json!({ "n": 2 }));
    let log = ok(&mut d, "input.log.get", json!({}));
    assert_eq!(log["clock"], "turn");
    assert_eq!(
        log["rows"],
        json!([[0, 1, "move", [1.0, 0.0]], [1, 1, "move", [0.0, 0.0]]])
    );
    let g = ok(&mut d, "world.get", json!({ "entity": "@hero" }));
    assert_eq!(
        g["components"]["Transform"]["pos"],
        json!([2.0, 0.0]),
        "moved on the one turn"
    );
}
