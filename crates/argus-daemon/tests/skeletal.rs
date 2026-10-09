//! Skeletons through the protocol (phase 5 item 8), on the puppet example:
//! the asset list names a skeleton and what it holds; its components are
//! described like any and its system follows the animator's; §6, a seek
//! replays the puppets, crossfade and events included, to the same world.

use argus_daemon::Daemon;
use serde_json::{json, Value as Json};
use std::path::Path;

fn puppet() -> Daemon {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/puppet");
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
fn the_asset_list_names_the_skeleton_and_what_it_holds() {
    let mut d = puppet();
    let list = d
        .call(0, "asset.list", &json!({ "kind": "skeleton" }))
        .unwrap();
    let rows = list["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{list}");
    let r = &rows[0];
    assert_eq!(r[0], "puppet/puppet");
    assert_eq!(r[1], "skeleton");
    assert_eq!(r[2], "assets/puppet/puppet.json");
    assert_eq!(r[3], 12, "its bones");
    assert_eq!(r[4], json!(["idle", "walk", "wave"]));
    assert_eq!(r[5], 0, "no warnings");
    // Its images are sprites like any.
    let sprites = d
        .call(0, "asset.list", &json!({ "kind": "sprite" }))
        .unwrap();
    assert!(
        sprites.to_string().contains("puppet/images/helmet"),
        "{sprites}"
    );
}

#[test]
fn the_components_are_described_and_the_system_follows_the_animator() {
    let mut d = puppet();
    let sk = d
        .call(0, "world.describe", &json!({ "component": "Skeleton" }))
        .unwrap();
    let text = sk.to_string();
    assert!(text.contains("flip_x") && text.contains("skin"), "{sk}");
    let sys = d.call(0, "script.systems", &json!({})).unwrap();
    let names: Vec<&str> = sys["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["name"].as_str())
        .collect();
    let at = |n: &str| names.iter().position(|x| *x == n).unwrap_or(usize::MAX);
    assert!(at("anim.update") < at("skeletal.pose"), "{names:?}");
    assert!(at("skeletal.pose") < at("late.show"), "{names:?}");
}

#[test]
fn a_seek_replays_the_puppets_to_the_same_world() {
    let mut d = puppet();
    d.call(0, "sim.step", &json!({ "n": 300 })).unwrap();
    let at_300 = digest(&mut d);
    // Back to before the cued wave, and on through it again.
    let s = d.call(0, "sim.seek", &json!({ "frame": 200 })).unwrap();
    assert!(s["diverged"].is_null(), "the replay matched its log: {s}");
    d.call(0, "sim.step", &json!({ "n": 100 })).unwrap();
    assert_eq!(d.call(0, "sim.status", &json!({})).unwrap()["tick"], 300);
    assert_eq!(digest(&mut d), at_300, "the same world at tick 300");
    let ev = d
        .call(0, "obs.events", &json!({ "name": "waved", "limit": 4 }))
        .unwrap();
    assert!(ev["total"].as_u64().unwrap_or(0) >= 1, "{ev}");
}
