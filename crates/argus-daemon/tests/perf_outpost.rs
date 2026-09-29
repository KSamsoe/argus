//! §17's Outpost rows against the real Outpost, for the phase 3 gate (its
//! core, with the inspector the gate asks for). Run in release:
//!
//! `cargo test -p argus-daemon --release --test perf_outpost -- --ignored --nocapture --test-threads 1`
//!
//! - Sim tick at 10x, 200 colonists, 20k items: under 1.6 ms ("ten ticks
//!   per render frame with pathfinding, jobs, and needs all running"), the
//!   stress scene, with the inspector closed and open on its people page
//!   (at 10x its words are written every hundredth tick).
//! - UI tree with text: under 1 ms layout ("inspector panels and overview
//!   screens are most of what the player looks at"): `ui.layout` with the
//!   inspector open.
//! - Headless fast-forward: over 2,000 ticks a second (a 30-day balance
//!   scenario in under a minute a seed).
//! - Save and load at 8 h of play: under 200 ms and 500 ms, with
//!   migrations; and the first week's save, through its four.

use argus_daemon::daemon::Daemon;
use serde_json::{json, Value as Json};
use std::time::Instant;

fn outpost() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/outpost")
        .canonicalize()
        .unwrap()
}

fn ok(d: &mut Daemon, method: &str, params: Json) -> Json {
    d.call(0, method, &params)
        .unwrap_or_else(|e| panic!("{method}: {e}"))
}

fn open(scene: &str) -> Daemon {
    let mut d = Daemon::new();
    let r = ok(
        &mut d,
        "session.open",
        json!({ "project": outpost().to_string_lossy(), "scene": scene, "seed": 1 }),
    );
    assert!(r.get("issues").is_none(), "{r}");
    d
}

fn tap(d: &mut Daemon, action: &str, value: Json) {
    let mut p = json!({ "action": action, "frames": 1 });
    if !value.is_null() {
        p["value"] = value;
    }
    ok(d, "input.inject", p);
    ok(d, "sim.step", json!({ "n": 2 }));
}

/// The tick's p95 and a system's p95 over the last `n` ticks, in ms.
fn prof(d: &mut Daemon, n: u64, system: &str) -> (f64, f64) {
    let p = ok(d, "prof.systems", json!({ "last": n }));
    let sys = p["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r[0] == system)
        .map(|r| r[3].as_f64().unwrap_or(0.0))
        .unwrap_or(0.0);
    (p["tick_p95_ms"].as_f64().unwrap(), sys)
}

#[test]
#[ignore]
fn perf_the_stress_scene_ticks_under_1_6_ms_with_the_inspector_open() {
    let mut d = open("stress");
    ok(&mut d, "sim.step", json!({ "n": 300 }));
    ok(&mut d, "sim.step", json!({ "n": 600 }));
    let (closed, _) = prof(&mut d, 600, "ui.layout");
    // At 10x, open on the people page: ten rows, rewritten six times a
    // second of the player's time (every hundredth tick at 10x).
    tap(&mut d, "speed_10", Json::Null);
    tap(&mut d, "inspect", Json::Null);
    tap(&mut d, "ui_move", json!([1, 0]));
    tap(&mut d, "ui_confirm", Json::Null);
    let page = ok(
        &mut d,
        "world.query",
        json!({ "select": ["Inspector.page"], "where": { "has": ["Inspector"] } }),
    );
    assert_eq!(page["rows"][0][0], "people");
    ok(&mut d, "sim.step", json!({ "n": 600 }));
    let (open_tick, layout) = prof(&mut d, 600, "ui.layout");
    let (_, inspector) = prof(&mut d, 600, "sim.colony.inspector");
    let nodes = ok(
        &mut d,
        "world.query",
        json!({ "select": ["count()"], "where": { "has": ["UiNode"] } }),
    );
    eprintln!(
        "stress, 200 colonists, 20k items: tick p95 {closed:.3} ms closed, {open_tick:.3} ms with the inspector open ({} UI nodes); ui.layout p95 {layout:.3} ms; the inspector's p95 {inspector:.3} ms",
        nodes["rows"][0][0]
    );
    assert!(closed < 1.6, "closed: {closed} ms");
    assert!(open_tick < 1.6, "open: {open_tick} ms");
    assert!(layout < 1.0, "layout: {layout} ms");
}

#[test]
#[ignore]
fn perf_thirty_days_fast_forward_and_eight_hours_save_and_load() {
    let mut d = open("valley");
    // Section 9's scenario's length: 30 days of 60 s at 60 Hz.
    let t = Instant::now();
    ok(&mut d, "sim.step", json!({ "n": 108_000 }));
    let secs = t.elapsed().as_secs_f64();
    let rate = 108_000.0 / secs;
    eprintln!("30 days (108,000 ticks) in {secs:.1} s: {rate:.0} ticks a second");
    assert!(rate > 2000.0, "{rate} ticks a second");
    // On to eight hours of play: 1,728,000 ticks, a game day a step, as a
    // session is played (the game saves itself as each day turns, and the
    // daemon writes the save when the step returns).
    let t = Instant::now();
    for _ in 0..(1_728_000 - 108_000) / 3600 {
        ok(&mut d, "sim.step", json!({ "n": 3600 }));
    }
    let st = ok(&mut d, "sim.status", json!({}));
    let alive = ok(
        &mut d,
        "world.query",
        json!({ "select": ["count()"], "where": { "has": ["Colonist"] } }),
    );
    eprintln!(
        "eight hours (tick {}) in {:.1} s more; day {}, {} colonists",
        st["tick"],
        t.elapsed().as_secs_f64(),
        ok(&mut d, "world.get", json!({ "entity": "@colony" }))["components"]["Colony"]["day"],
        alive["rows"][0][0]
    );
    let t = Instant::now();
    let saved = ok(&mut d, "sim.save", json!({ "name": "perf-8h" }));
    let save_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    ok(&mut d, "sim.load", json!({ "name": "perf-8h" }));
    let load_ms = t.elapsed().as_secs_f64() * 1000.0;
    eprintln!(
        "save at 8 h: {save_ms:.1} ms ({} bytes); load: {load_ms:.1} ms",
        saved["bytes"]
    );
    let _ = std::fs::remove_file(outpost().join(".argus/saves/perf-8h.save"));
    assert!(save_ms < 200.0, "save {save_ms} ms");
    assert!(load_ms < 500.0, "load {load_ms} ms");

    // The first week's save, through its migrations.
    let t = Instant::now();
    ok(
        &mut d,
        "sim.load",
        json!({ "path": "tests/saves/week1.save" }),
    );
    let week1_ms = t.elapsed().as_secs_f64() * 1000.0;
    let migrated = ok(
        &mut d,
        "obs.events",
        json!({ "name": "component.migrated" }),
    );
    eprintln!(
        "the first week's save: loaded in {week1_ms:.1} ms, {} components migrated",
        migrated["total"]
    );
    assert!(week1_ms < 500.0, "week 1 {week1_ms} ms");
}
