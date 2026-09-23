//! Release-only gates for §17's seek rows: a full keyframe is invisible
//! during play (the snapshot row is under 5 ms at 100k entities), and a seek
//! to any point in the last five minutes is under 500 ms. Measured on
//! Swarm's arena, a real game with waves in Luau.
//!
//! cargo test -p argus-daemon --release --test perf_seek -- --ignored --nocapture

use argus_daemon::Daemon;
use serde_json::json;
use std::path::Path;
use std::time::Instant;

fn swarm() -> Daemon {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/swarm");
    let mut d = Daemon::new();
    d.call(
        0,
        "session.open",
        &json!({ "project": root.canonicalize().unwrap().to_string_lossy(), "scene": "arena" }),
    )
    .unwrap();
    d
}

#[test]
#[ignore = "release-only perf gate; run with --release -- --ignored"]
fn perf_seek_anywhere_in_the_last_five_minutes_under_500ms() {
    let mut d = swarm();
    // Five minutes and a bit of sim time, in one-second steps, so the
    // wall-clock cost of keyframes shows against the same run without them.
    let started = Instant::now();
    for _ in 0..320 {
        d.call(0, "sim.step", &json!({ "n": 60 })).unwrap();
    }
    let run_ms = started.elapsed().as_secs_f64() * 1e3;
    let ring = d.call(0, "sim.snapshot", &json!({})).unwrap();
    let end = ring["end"].as_u64().unwrap();
    println!(
        "ran {end} ticks in {run_ms:.0} ms ({:.3} ms a tick with keyframes); ring: {} keyframes, {:.1} MB",
        run_ms / end as f64,
        ring["keyframes"],
        ring["bytes"].as_f64().unwrap() / 1e6
    );
    // The worst seeks: one tick before a keyframe (the longest replay), at
    // the far end of the five minutes, in the middle, and near the end.
    let five = 5 * 60 * 60;
    let mut worst: f64 = 0.0;
    for target in [
        end - five + 59,
        end - five / 2 + 59,
        end - 1,
        end - five + 1,
        end - 61,
    ] {
        let s = d.call(0, "sim.seek", &json!({ "frame": target })).unwrap();
        let ms = s["ms"].as_f64().unwrap();
        println!(
            "seek to {target}: from {} ({} ticks replayed) in {ms:.1} ms, diverged {}",
            s["from"], s["replayed"], s["diverged"]
        );
        assert!(s["diverged"].is_null(), "{s}");
        worst = worst.max(ms);
    }
    println!("worst seek {worst:.1} ms (bound 500)");
    assert!(worst < 500.0, "worst seek {worst:.1} ms");
}

#[test]
#[ignore = "release-only perf gate; run with --release -- --ignored"]
fn perf_a_keyframe_of_the_stress_scene() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/swarm");
    let mut d = Daemon::new();
    d.call(
        0,
        "session.open",
        &json!({ "project": root.canonicalize().unwrap().to_string_lossy(), "scene": "stress" }),
    )
    .unwrap();
    d.call(0, "sim.step", &json!({ "n": 2 })).unwrap();
    let entities = d.world.entity_count();
    let mut times = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        d.call(0, "sim.snapshot", &json!({})).unwrap();
        times.push(t.elapsed().as_secs_f64() * 1e3);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = times[times.len() / 2];
    println!("a keyframe of {entities} entities: median {median:.2} ms (bound 5 at 100k)");
    assert!(
        median < 5.0 * entities as f64 / 100_000.0 + 1.0,
        "{median:.2} ms"
    );
}
