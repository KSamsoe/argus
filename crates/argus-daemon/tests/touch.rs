//! Reaching players item 1: touch through the daemon. Fingers by
//! `input.device`, the controls they hold, the actions those press, the
//! semantic frame's `touch`, and a seek that plays the same run again.

mod common;

use common::*;
use serde_json::{json, Value as Json};

const INPUT: &str = r#"[actions.move]
type = "axis2"
doc = "Walk direction"
[actions.move.bindings]
keyboard = ["wasd"]
touch = ["stick"]

[actions.fire]
type = "button"
doc = "Shoot"
[actions.fire.bindings]
keyboard = ["space"]
touch = ["fire"]

[touch.stick]
kind = "stick"
anchor = "bottom-left"
at = [120, 120]
size = 160

[touch.fire]
kind = "button"
anchor = "bottom-right"
at = [90, 90]
size = 100

[slots.1]
devices = ["keyboard", "touch"]
"#;

fn touch(d: &mut argus_daemon::Daemon, finger: u64, phase: &str, x: f32, y: f32) {
    ok(
        d,
        "input.device",
        json!({ "events": [{ "touch": finger, "phase": phase, "at": [x, y] }] }),
    );
}

fn player(d: &mut argus_daemon::Daemon, path: &str) -> Json {
    let q = ok(
        d,
        "world.query",
        json!({ "select": [path], "where": { "has": ["PlayerSlot"] } }),
    );
    q["rows"][0][0].clone()
}

#[test]
fn a_finger_on_the_stick_walks_the_hero_and_a_seek_plays_it_again() {
    let t = temp("touch");
    project(t.path());
    write(t.path(), "input.toml", INPUT);
    let mut d = open(t.path());
    let start = player(&mut d, "Transform.pos");
    let begin = ok(&mut d, "sim.status", json!({}))["frame"]
        .as_u64()
        .unwrap();

    // The stick's centre is (120, 420) on the 960 by 540 canvas: a finger
    // there, then all the way right, walks the hero right.
    touch(&mut d, 1, "start", 120.0, 420.0);
    touch(&mut d, 1, "move", 200.0, 420.0);
    ok(&mut d, "sim.step", json!({ "n": 10 }));
    let walked = player(&mut d, "Transform.pos");
    assert!(
        walked[0].as_f64().unwrap() > start[0].as_f64().unwrap() + 10.0,
        "{start} to {walked}"
    );
    assert_eq!(player(&mut d, "Actions.move"), json!([1.0, 0.0]));
    assert_eq!(player(&mut d, "Actions.device"), json!(5.0), "touch in use");

    // A second finger on the button (centre 870, 450) fires.
    touch(&mut d, 2, "start", 870.0, 450.0);
    ok(&mut d, "sim.step", json!({ "n": 1 }));
    assert_eq!(player(&mut d, "Actions.fire"), json!(true));
    let dev = ok(&mut d, "input.devices", json!({}));
    let holds: Vec<&str> = dev["touches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["holds"].as_str().unwrap())
        .collect();
    assert_eq!(holds, ["stick", "fire"], "{dev}");

    // The semantic frame lists the controls drawn, both held, the button
    // with its action's name for words, in the frame's pixels (640 by 360:
    // two thirds of the canvas's).
    let f = ok(&mut d, "obs.frame", json!({ "format": "semantic" }));
    let controls = f["touch"].as_array().unwrap_or_else(|| panic!("{f}"));
    assert_eq!(controls.len(), 2, "{f}");
    let fire = controls.iter().find(|c| c["name"] == "fire").unwrap();
    assert_eq!(fire["kind"], "button");
    assert_eq!(fire["pressed"], true);
    assert_eq!(fire["label"], "Fire");
    assert_eq!(fire["bounds"], json!([546.7, 266.7, 613.3, 333.3]));

    // Lifted, the hero stops; a key hides the controls again.
    touch(&mut d, 1, "end", 200.0, 420.0);
    touch(&mut d, 2, "end", 870.0, 450.0);
    ok(&mut d, "sim.step", json!({ "n": 5 }));
    assert_eq!(player(&mut d, "Actions.move"), json!([0.0, 0.0]));
    let end = ok(&mut d, "sim.status", json!({}))["frame"]
        .as_u64()
        .unwrap();
    let there = player(&mut d, "Transform.pos");
    ok(
        &mut d,
        "input.device",
        json!({ "events": [{ "key": "KeyD", "pressed": true }, { "key": "KeyD", "pressed": false }] }),
    );
    let f = ok(&mut d, "obs.frame", json!({ "format": "semantic" }));
    assert!(f.get("touch").is_none(), "hidden: {f}");

    // The run recorded the actions the fingers pressed: back to the start
    // and forward again, the hero walks to the same place.
    ok(&mut d, "sim.seek", json!({ "frame": begin }));
    assert_eq!(player(&mut d, "Transform.pos"), start);
    ok(&mut d, "sim.seek", json!({ "frame": end }));
    assert_eq!(player(&mut d, "Transform.pos"), there);

    // A touch event that does not read is refused, saying what it needs.
    let e = err(
        &mut d,
        "input.device",
        json!({ "events": [{ "touch": 3, "phase": "tap", "at": [1, 2] }] }),
    );
    assert!(
        e.message.contains("start, move, end, or cancel"),
        "{}",
        e.message
    );
}
