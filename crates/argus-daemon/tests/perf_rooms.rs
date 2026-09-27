//! §17's Hollow rows against the real Hollow at 200 rooms (phase 3 item 6,
//! the phase 3 gate: "Hollow reaches 200 rooms with the §17 open and
//! stream-in numbers green"). Run in release:
//!
//! `cargo test -p argus-daemon --release --test perf_rooms -- --ignored --nocapture`
//!
//! - Authored entities per scene: 10,000 across 200 rooms.
//! - Cold scene open, the index warm: under 300 ms ("opening the project
//!   must not be the slow part of the agent's loop"). A new daemon opens
//!   Hollow after another has built its index.
//! - Room stream-in: under 4 ms; Hollow's `tests/deep.luau` measures it as
//!   a player meets it, a walk of 25 rooms, in `rooms.stream`'s worst tick.

use argus_daemon::daemon::Daemon;
use serde_json::{json, Value as Json};

fn hollow() -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/hollow")
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn open(root: &str) -> Json {
    let mut d = Daemon::new();
    d.call(0, "session.open", &json!({ "project": root }))
        .unwrap_or_else(|e| panic!("session.open: {e}"))
}

#[test]
#[ignore]
fn hollow_at_two_hundred_rooms_opens_cold_under_300_ms() {
    let root = hollow();
    // The first open builds or checks the index; the next is the cold open
    // an agent's loop pays each time it starts a daemon.
    let first = open(&root);
    assert!(first.get("issues").is_none(), "{first}");
    let mut worst: f64 = 0.0;
    let mut runs = Vec::new();
    for _ in 0..5 {
        let r = open(&root);
        let ms = r["ms"].as_f64().unwrap();
        runs.push((
            ms,
            r["index_ms"].as_f64().unwrap(),
            r["load"]["ms"].as_f64().unwrap(),
        ));
        worst = worst.max(ms);
    }
    let authored = first["authored"].as_u64().unwrap();
    let rooms = first["rooms"].as_array().map(Vec::len).unwrap_or(0);
    println!("first open (index built or checked): {} ms", first["ms"]);
    for (ms, index_ms, load_ms) in &runs {
        println!("cold open, index warm: {ms:.1} ms (index {index_ms:.1} ms, spawning the live rooms {load_ms:.1} ms)");
    }
    println!("{authored} authored entities live of the scene's ~10,000; {rooms} rooms loaded");
    assert!(worst < 300.0, "cold open {worst:.1} ms >= 300 ms");
}
