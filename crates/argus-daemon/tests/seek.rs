//! Phase 2 item 0: `sim.seek`, `sim.snapshot`, and the keyframe ring (§6).
//! A run is a value: a seek back rebuilds the world at any frame, and
//! stepping on replays what happened next until a live command branches it.

mod common;

use argus_daemon::Daemon;
use common::*;
use serde_json::{json, Value as Json};

/// The fixture, plus bats that come at random, drift at random, and go at
/// zero hit points: randomness, spawns, and despawns for a replay to get
/// right.
fn busy(name: &str, scene: &str) -> (Temp, Daemon) {
    let t = temp(name);
    project(t.path());
    write(
        t.path(),
        "systems/swarm.luau",
        r#"system "sim.swarm" {
  stage = "sim",
  after = { "sim.damage" },
  reads = { "Enemy", "Health", "Transform" },
  writes = { "Enemy", "Health", "Transform" },
  rng = "swarm",
  doc = "Bats come at random, drift at random, and go at zero hit points.",
  run = function(q)
    local rng = q:rng()
    local gone = {}
    for e, hp, tr in q:each("Health", "Transform") do
      if e:has("Enemy") then
        if hp.cur <= 0 then
          table.insert(gone, e:id())
        else
          local p = tr.pos
          tr.pos = vector.create(p.x + rng:range(-1, 1), p.y + rng:range(-1, 1), 0)
        end
      end
    end
    for _, id in ipairs(gone) do q:entity(id):despawn() end
    if rng:range(1, 3) == 1 then
      local b = q:spawn({ "Enemy", "Health", "Transform" })
      b:get("Health").cur = rng:range(3, 9)
      b:get("Transform").pos = vector.create(rng:range(-20, 20), rng:range(-20, 20), 0)
    end
  end,
}
"#,
    );
    let mut d = open(t.path());
    if scene != "arena" {
        ok(&mut d, "scene.load", json!({ "scene": scene }));
    }
    (t, d)
}

/// Everything with hit points, by id: what a seek must rebuild.
fn state(d: &mut Daemon) -> Json {
    let q = ok(
        d,
        "world.query",
        json!({ "select": ["id", "Transform.pos", "Health.cur"], "where": { "has": ["Health"] }, "limit": 1000 }),
    );
    json!({ "rows": q["rows"], "tick": ok(d, "sim.status", json!({}))["tick"] })
}

fn step(d: &mut Daemon, n: u64) {
    ok(d, "sim.step", json!({ "n": n }));
}

fn inject(d: &mut Daemon, action: &str, value: Json, frames: Option<u64>) {
    let mut p = json!({ "action": action, "value": value });
    match frames {
        Some(f) => p["frames"] = json!(f),
        None => p["hold"] = json!(true),
    }
    ok(d, "input.inject", p);
}

fn deaths(d: &mut Daemon) -> Vec<u64> {
    let ev = ok(d, "obs.events", json!({ "name": "death", "limit": 10000 }));
    ev["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["frame"].as_u64().unwrap())
        .collect()
}

#[test]
fn a_seek_back_rebuilds_the_world_and_stepping_on_replays_the_run() {
    let (_t, mut d) = busy("seek-back", "arena");
    inject(&mut d, "move", json!([1, 0]), Some(40));
    step(&mut d, 50);
    inject(&mut d, "fire", json!(true), Some(1));
    step(&mut d, 47);
    let at97 = state(&mut d);
    inject(&mut d, "move", json!([0, -1]), None);
    step(&mut d, 83);
    let at180 = state(&mut d);
    let died = deaths(&mut d);
    assert!(died.len() > 10, "the run has bats dying: {died:?}");
    assert!(at97["rows"].as_array().unwrap().len() > 2, "{at97}");

    let s = ok(&mut d, "sim.seek", json!({ "frame": 97 }));
    assert_eq!(s["tick"], 97);
    assert_eq!(s["from"], 60, "the nearest earlier keyframe, one a second");
    assert_eq!(s["replayed"], 37);
    assert!(s["diverged"].is_null(), "{s}");
    assert!(s["checked"].as_u64().unwrap() > 0, "{s}");
    assert_eq!(state(&mut d), at97, "the world at 97, rebuilt");
    assert!(
        deaths(&mut d).iter().all(|f| *f < 97),
        "what ticks from 97 on emitted is the future now"
    );

    // Stepping on replays the recorded future: the held move, the deaths.
    step(&mut d, 83);
    assert_eq!(state(&mut d), at180, "the same run, replayed");
    assert_eq!(deaths(&mut d), died, "and the same events, logged once");

    // And a seek can go forward along the run already played.
    ok(&mut d, "sim.seek", json!({ "frame": 30 }));
    let fwd = ok(&mut d, "sim.seek", json!({ "frame": 97 }));
    assert!(fwd["diverged"].is_null(), "{fwd}");
    assert_eq!(state(&mut d), at97);

    // verify: the keyframe before, replayed into the one used, is the same
    // world field for field.
    let v = ok(&mut d, "sim.seek", json!({ "frame": 150, "verify": true }));
    assert_eq!(v["verified"]["ok"], true, "{v}");
    assert_eq!(v["verified"]["from"], 60);
    assert_eq!(v["verified"]["to"], 120);
}

#[test]
fn a_live_command_after_a_seek_branches_the_run() {
    let (_t, mut d) = busy("seek-branch", "arena");
    inject(&mut d, "move", json!([1, 0]), None);
    step(&mut d, 120);
    let live120 = state(&mut d);
    ok(&mut d, "sim.seek", json!({ "frame": 60 }));
    assert_eq!(
        ok(&mut d, "sim.status", json!({}))["end"],
        120,
        "the recorded run"
    );
    inject(&mut d, "move", json!([-1, 0]), None);
    assert_eq!(
        ok(&mut d, "sim.status", json!({}))["end"],
        60,
        "a new command: what came after is gone"
    );
    step(&mut d, 60);
    let branch120 = state(&mut d);
    assert_ne!(branch120, live120, "the player walked the other way");
    let player = |s: &Json| {
        s["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r[0] == "@player")
            .unwrap()[1]
            .clone()
    };
    assert_eq!(player(&live120), json!([240.0, 0.0]));
    assert_eq!(player(&branch120), json!([0.0, 0.0]), "60 right, 60 left");
    // The new branch replays too.
    ok(&mut d, "sim.seek", json!({ "frame": 90 }));
    step(&mut d, 30);
    assert_eq!(state(&mut d), branch120);
    let e = err(&mut d, "sim.seek", json!({ "frame": 500 }));
    assert!(e.message.contains("past the end"), "{}", e.message);
}

#[test]
fn a_change_between_ticks_takes_a_keyframe_a_replay_never_crosses() {
    let (_t, mut d) = busy("seek-barrier", "arena");
    step(&mut d, 50);
    commit(
        &mut d,
        "agent:test",
        "a weaker player",
        &[(
            "edit.set",
            json!({ "entity": "@player", "path": "Health.cur", "value": 1 }),
        )],
    );
    step(&mut d, 50);
    let hp = |d: &mut Daemon| {
        ok(d, "world.get", json!({ "entity": "@player" }))["components"]["Health"]["cur"].clone()
    };
    ok(&mut d, "sim.seek", json!({ "frame": 75 }));
    assert_eq!(
        hp(&mut d),
        1,
        "after the edit: replayed from the keyframe it took"
    );
    let s = ok(&mut d, "sim.seek", json!({ "frame": 50 }));
    assert_eq!(s["from"], 50);
    assert_eq!(hp(&mut d), 1, "at 50 the edit had happened");
    ok(&mut d, "sim.seek", json!({ "frame": 30 }));
    assert_eq!(hp(&mut d), 3, "before it, the value the scene had");
    let ring = ok(&mut d, "sim.snapshot", json!({}));
    assert!(ring["barriers"].as_u64().unwrap() >= 2, "{ring}");
}

#[test]
fn a_discrete_run_seeks_by_turn() {
    let (_t, mut d) = busy("seek-turns", "delve");
    let mut at = Vec::new();
    for turn in 0..30u64 {
        if turn % 7 == 3 {
            inject(&mut d, "fire", json!(true), Some(1));
        }
        ok(&mut d, "sim.advance", json!({ "n": 1, "by": "turn" }));
        at.push(state(&mut d));
    }
    let s = ok(&mut d, "sim.seek", json!({ "turn": 12 }));
    assert_eq!(s["from"], 10, "a keyframe every ten turns");
    assert!(s["diverged"].is_null(), "{s}");
    assert_eq!(state(&mut d), at[11], "turn 12, as it was");
    ok(&mut d, "sim.advance", json!({ "n": 18, "by": "turn" }));
    assert_eq!(state(&mut d), at[29]);
}

#[test]
fn stop_puts_back_the_input_state_and_ends_the_run_it_stopped() {
    let (_t, mut d) = busy("seek-stop", "arena");
    step(&mut d, 10);
    let before = state(&mut d);
    ok(&mut d, "sim.play", json!({}));
    inject(&mut d, "move", json!([1, 0]), None);
    step(&mut d, 40);
    ok(&mut d, "sim.stop", json!({}));
    assert_eq!(state(&mut d), before);
    let input = ok(&mut d, "input.state", json!({}));
    assert_eq!(
        input["actions"]["move"],
        json!([0.0, 0.0]),
        "the held move was after play began: {input}"
    );
    assert_eq!(ok(&mut d, "sim.status", json!({}))["end"], 10);
    step(&mut d, 5);
    let p = ok(&mut d, "world.get", json!({ "entity": "@player" }));
    assert_eq!(p["components"]["Transform"]["pos"], json!([0.0, 0.0]));
}

// ----- sim.fork ---------------------------------------------------------------

fn player_pos(d: &mut Daemon, fork: Option<&str>) -> Json {
    let mut p = json!({ "entity": "@player" });
    if let Some(f) = fork {
        p["fork"] = json!(f);
    }
    ok(d, "world.get", p)["components"]["Transform"]["pos"].clone()
}

#[test]
fn a_fork_has_its_own_clock_and_any_method_can_name_it() {
    let (_t, mut d) = busy("fork-now", "arena");
    inject(&mut d, "move", json!([1, 0]), None);
    step(&mut d, 30);
    let made = ok(&mut d, "sim.fork", json!({ "name": "b" }));
    assert_eq!(made["made"]["frame"], 30);
    step(&mut d, 30);
    ok(&mut d, "sim.step", json!({ "n": 10, "fork": "b" }));
    assert_eq!(ok(&mut d, "sim.status", json!({}))["tick"], 60);
    assert_eq!(ok(&mut d, "sim.status", json!({ "fork": "b" }))["tick"], 40);
    assert_eq!(player_pos(&mut d, None), json!([120.0, 0.0]));
    assert_eq!(
        player_pos(&mut d, Some("b")),
        json!([80.0, 0.0]),
        "the fork kept the held move and walked on its own clock"
    );
    // Switching makes it the world in use; main is parked under its name.
    let sw = ok(&mut d, "sim.fork", json!({ "name": "b", "switch": true }));
    assert_eq!(sw["active"], "b");
    assert_eq!(ok(&mut d, "sim.status", json!({}))["fork"], "b");
    assert_eq!(player_pos(&mut d, None), json!([80.0, 0.0]));
    assert_eq!(player_pos(&mut d, Some("main")), json!([120.0, 0.0]));
    ok(
        &mut d,
        "sim.fork",
        json!({ "name": "main", "switch": true }),
    );
    let list = ok(&mut d, "sim.fork", json!({ "name": "b", "drop": true }));
    assert_eq!(list["worlds"].as_array().unwrap().len(), 1, "{list}");
    let e = err(&mut d, "sim.step", json!({ "fork": "b" }));
    assert!(e.message.contains("no fork named"), "{}", e.message);
}

#[test]
fn a_fork_from_a_past_frame_replays_the_same_inputs() {
    let (_t, mut d) = busy("fork-past", "arena");
    inject(&mut d, "move", json!([1, 0]), Some(30));
    step(&mut d, 50);
    inject(&mut d, "move", json!([0, 1]), None);
    step(&mut d, 70);
    let main120 = state(&mut d);
    let made = ok(&mut d, "sim.fork", json!({ "name": "past", "frame": 40 }));
    assert_eq!(made["made"]["from"], 0, "replayed from the load's keyframe");
    ok(&mut d, "sim.step", json!({ "n": 80, "fork": "past" }));
    let mut fork120 = state_of(&mut d, "past");
    fork120["tick"] = main120["tick"].clone();
    assert_eq!(
        fork120, main120,
        "the same inputs, replayed: the same world"
    );
    // A command of its own branches the fork, not main.
    ok(&mut d, "sim.seek", json!({ "frame": 60, "fork": "past" }));
    ok(
        &mut d,
        "input.inject",
        json!({ "action": "move", "value": [-1, 0], "hold": true, "fork": "past" }),
    );
    assert_eq!(
        ok(&mut d, "sim.status", json!({ "fork": "past" }))["end"],
        60
    );
    assert_eq!(
        ok(&mut d, "sim.status", json!({}))["end"],
        120,
        "main is untouched"
    );
}

fn state_of(d: &mut Daemon, fork: &str) -> Json {
    let q = ok(
        d,
        "world.query",
        json!({ "select": ["id", "Transform.pos", "Health.cur"], "where": { "has": ["Health"] }, "limit": 1000, "fork": fork }),
    );
    json!({ "rows": q["rows"], "tick": ok(d, "sim.status", json!({ "fork": fork }))["tick"] })
}

/// §6: forks are how the harness runs "same inputs, two code versions".
#[test]
fn a_fork_after_a_script_edit_runs_the_new_code_beside_the_old() {
    let (t, mut d) = busy("fork-code", "arena");
    inject(&mut d, "move", json!([1, 0]), None);
    step(&mut d, 10);
    let text = read(t.path(), "systems/move.luau").replace("m.x * 2", "m.x * 3");
    write(t.path(), "systems/move.luau", &text);
    ok(&mut d, "sim.fork", json!({ "name": "faster" }));
    step(&mut d, 10);
    ok(&mut d, "sim.step", json!({ "n": 10, "fork": "faster" }));
    assert_eq!(
        player_pos(&mut d, None),
        json!([40.0, 0.0]),
        "the old code, 2 a tick"
    );
    assert_eq!(
        player_pos(&mut d, Some("faster")),
        json!([50.0, 0.0]),
        "the new code, 3 a tick from the fork on"
    );
}

/// §19's open question (snapshot the Luau heap, or forbid per-system state)
/// is decided the strict way: the heap is not in keyframes. A system that
/// keeps state in a script-level variable replays differently, and a seek
/// says so instead of handing back a wrong world quietly.
#[test]
fn a_system_that_keeps_state_outside_the_world_shows_as_a_divergence() {
    let (t, _) = busy("seek-heap", "arena");
    write(
        t.path(),
        "systems/counter.luau",
        r#"local n = 0
system "sim.counter" {
  stage = "sim",
  doc = "Counts ticks in a variable of its own: the state a keyframe cannot hold.",
  run = function(q)
    n += 1
    if n % 10 == 0 then q:emit("tenth", { n = n }) end
  end,
}
"#,
    );
    let mut d = open(t.path());
    step(&mut d, 100);
    let s = ok(&mut d, "sim.seek", json!({ "frame": 55 }));
    let div = &s["diverged"];
    assert!(!div.is_null(), "{s}");
    assert_eq!(div["logged"]["name"], "tenth", "{div}");
    assert_eq!(
        div["frame"], 9,
        "the first tenth, where the replay's count differs: {div}"
    );
}
