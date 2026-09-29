//! Phase 4 item 1: what the editor's viewport costs the daemon. A frame
//! stream draws the game offscreen, reads it back, and copies it into the
//! shared file, every frame the game plays: at 1280 x 720 that has to fit
//! beside the tick inside a 16.6 ms frame. Run in release:
//! `cargo test -p argus-daemon --release --test perf_stream -- --ignored --nocapture`.

mod common;

use common::*;
use serde_json::json;
use std::path::Path;
use std::time::Instant;

fn measure(game: &str) -> (f64, f64, f64, f64) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(game)
        .canonicalize()
        .unwrap();
    let mut d = open(&root);
    d.call(
        2,
        "render.stream",
        &json!({ "width": 1280, "height": 720, "audio": false }),
    )
    .unwrap();
    ok(&mut d, "sim.play", json!({}));
    let (mut stream, mut whole) = (Vec::new(), Vec::new());
    for i in 0..360 {
        let t0 = Instant::now();
        d.frame().unwrap();
        // Due at once: the timing is the drawing, not the pacing.
        d.stream_frame();
        let dt = t0.elapsed().as_secs_f64() * 1000.0;
        let st = ok(&mut d, "render.status", json!({}));
        d.outbox.clear();
        if i >= 60 {
            stream.push(st["stream"]["last_ms"].as_f64().unwrap());
            whole.push(dt);
        }
        std::thread::sleep(std::time::Duration::from_millis(17));
    }
    let frames = ok(&mut d, "render.status", json!({}))["stream"]["frames"]
        .as_u64()
        .unwrap();
    assert!(frames >= 300, "a frame every frame: {frames}");
    let p95 = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() * 95 / 100]
    };
    let mean = stream.iter().sum::<f64>() / stream.len() as f64;
    let sp = p95(&mut stream);
    let wmean = whole.iter().sum::<f64>() / whole.len() as f64;
    let wp = p95(&mut whole);
    println!(
        "{game}: stream frame mean {mean:.2} ms, p95 {sp:.2} ms; tick and frame together mean {wmean:.2} ms, p95 {wp:.2} ms"
    );
    (mean, sp, wmean, wp)
}

#[test]
#[ignore = "release-only perf gate; run with --release -- --ignored"]
fn perf_a_1280_by_720_viewport_fits_in_a_frame() {
    for game in ["hollow", "outpost"] {
        let (_, sp, _, wp) = measure(game);
        assert!(sp < 6.0, "{game}: the stream's frame p95 {sp:.2} ms");
        assert!(wp < 10.0, "{game}: a tick and its frame p95 {wp:.2} ms");
    }
}
