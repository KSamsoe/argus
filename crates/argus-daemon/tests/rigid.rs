//! The rigid-body module through the protocol (phase 5 item 7), on the
//! toybox example: §6, a seek replays to the same world, since every
//! keyframe carries Rapier's own state; the module's components are
//! described like any; a game that does not list the module has none.

use argus_daemon::Daemon;
use serde_json::{json, Value as Json};
use std::path::Path;

fn toybox() -> Daemon {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/toybox");
    let mut d = Daemon::new();
    d.call(
        0,
        "session.open",
        &json!({ "project": root.canonicalize().unwrap().to_string_lossy() }),
    )
    .unwrap();
    d
}

fn digest(d: &mut Daemon) -> Json {
    d.call(0, "sim.status", &json!({ "digest": true })).unwrap()["digest"].clone()
}

#[test]
fn a_seek_replays_the_rigid_bodies_to_the_same_world() {
    let mut d = toybox();
    d.call(0, "sim.step", &json!({ "n": 300 })).unwrap();
    let at_300 = digest(&mut d);
    let s = d.call(0, "sim.seek", &json!({ "frame": 130 })).unwrap();
    assert!(s["diverged"].is_null(), "the replay matched its log: {s}");
    d.call(0, "sim.step", &json!({ "n": 170 })).unwrap();
    assert_eq!(d.call(0, "sim.status", &json!({})).unwrap()["tick"], 300);
    assert_eq!(digest(&mut d), at_300, "the same world at tick 300");
    // Balls fell and bounced on the way: there was something to replay.
    let ev = d
        .call(
            0,
            "obs.events",
            &json!({ "name": "contact.begin", "limit": 1 }),
        )
        .unwrap();
    assert!(ev["total"].as_u64().unwrap_or(0) > 0, "{ev}");
}

#[test]
fn the_modules_components_are_described_and_its_system_is_listed() {
    let mut d = toybox();
    let rb = d
        .call(0, "world.describe", &json!({ "component": "RigidBody" }))
        .unwrap();
    assert!(rb.to_string().contains("restitution"), "{rb}");
    let sys = d.call(0, "script.systems", &json!({})).unwrap();
    let names: Vec<&str> = sys["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["name"].as_str())
        .filter(|n| n.starts_with("physics."))
        .collect();
    assert_eq!(
        names,
        vec![
            "physics.control",
            "physics.move",
            "physics.rigid",
            "physics.overlap"
        ]
    );
}

#[test]
fn a_game_that_does_not_list_the_module_has_none_of_it() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/delve");
    let mut d = Daemon::new();
    d.call(
        0,
        "session.open",
        &json!({ "project": root.canonicalize().unwrap().to_string_lossy() }),
    )
    .unwrap();
    let e = d
        .call(0, "world.describe", &json!({ "component": "RigidBody" }))
        .unwrap_err();
    assert!(e.message.contains("RigidBody"), "{e:?}");
    let sys = d.call(0, "script.systems", &json!({})).unwrap();
    assert!(!sys.to_string().contains("physics.rigid"));
}
