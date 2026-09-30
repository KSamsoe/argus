//! A game reads its speed (Luau's `q:speed()`; Outpost's HUD showed 1x
//! while the editor ran it at 10x), so the speed is part of the run: a
//! game's request takes effect at the end of the tick that asked, inside
//! the run, and a speed set from outside is a change between ticks. A seek
//! lands on the same world either side of both.

mod common;

use argus_daemon::Daemon;
use common::*;
use serde_json::{json, Value as Json};

/// The fixture, with a system that writes the speed it sees into a field
/// every tick and asks for 5x at tick 90.
fn paced(name: &str) -> (Temp, Daemon) {
    let t = temp(name);
    project(t.path());
    write(
        t.path(),
        "components/pace.luau",
        "component \"Pace\" { doc = \"The speed a system saw\", x = field.f32 { default = 0 } }\n",
    );
    write(
        t.path(),
        "systems/pace.luau",
        r#"system "sim.pace" {
  stage = "sim",
  writes = { "Pace" },
  run = function(q)
    for _, p in q:each("Pace") do p.x = q:speed() end
    if q.tick == 90 then q:speed(5) end
  end,
}
"#,
    );
    write(t.path(), "scenes/arena/pace.toml", "[Pace]\n");
    let d = open(t.path());
    (t, d)
}

fn pace(d: &mut Daemon) -> Json {
    ok(d, "world.get", json!({ "entity": "@pace" }))["components"]["Pace"]["x"].clone()
}

fn call(d: &mut Daemon, method: &str, params: Json) -> Json {
    match d.call(1, method, &params) {
        Ok(v) => v,
        Err(e) => panic!("{method} failed: {e}"),
    }
}

#[test]
fn a_seek_lands_on_the_same_speed_either_side_of_a_change() {
    let (_t, mut d) = paced("speed-seek");
    call(&mut d, "sim.step", json!({ "n": 60 }));
    assert_eq!(pace(&mut d), 1.0);
    // From outside (the editor's bar): a change between ticks.
    call(&mut d, "sim.speed", json!({ "x": 10 }));
    call(&mut d, "sim.step", json!({ "n": 15 }));
    let at75 = pace(&mut d);
    assert_eq!(at75, 10.0, "the game sees the clock's speed");
    let ring = ok(&mut d, "sim.ring", json!({}));
    assert!(
        ring["barrier_ticks"]
            .as_array()
            .unwrap()
            .contains(&json!(60)),
        "a keyframe where it changed: {ring}"
    );
    // The game's own request at 90: from the tick after.
    call(&mut d, "sim.step", json!({ "n": 25 }));
    let at100 = pace(&mut d);
    assert_eq!(at100, 5.0);
    assert_eq!(ok(&mut d, "sim.status", json!({}))["speed"], 5.0);

    // Back and forth across both: the same worlds, the clock with them.
    for (frame, want, speed) in [
        (75, at75.clone(), 10.0),
        (50, json!(1.0), 1.0),
        (100, at100.clone(), 5.0),
    ] {
        let s = call(
            &mut d,
            "sim.seek",
            json!({ "frame": frame, "verify": true }),
        );
        assert!(s["diverged"].is_null(), "{frame}: {s}");
        assert_ne!(s["verified"]["ok"], false, "{frame}: {s}");
        assert_eq!(pace(&mut d), want, "at {frame}");
        assert_eq!(s["speed"], speed, "the clock at {frame}");
    }
    // The same speed again is no change.
    let before = ok(&mut d, "sim.ring", json!({}))["barrier_ticks"]
        .as_array()
        .unwrap()
        .len();
    call(&mut d, "sim.speed", json!({ "x": 5 }));
    call(&mut d, "sim.step", json!({ "n": 1 }));
    let after = ok(&mut d, "sim.ring", json!({}))["barrier_ticks"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(before, after);
}
