//! The viewport keeps up with the game (Kevin's first play in the editor:
//! "the game has poor performance in the editor"). Two faults: the daemon
//! drew 37 frames a second of the sim's 60, since a frame's due time was
//! counted from when the last one happened to be drawn and landed a hair
//! after the loop's next turn; and a debug frame of the editor took 18 ms,
//! egui unoptimized and a frame's pixels turned into colours one by one.
//! This runs Hollow playing through the server's real loop over a socket,
//! the editor drawing as a window would, in the debug build Kevin runs.

#[path = "../../argus-daemon/tests/common/mod.rs"]
mod common;

use argus_client::Client;
use argus_daemon::{serve, socket_name_for, Transport};
use argus_editor::Editor;
use eframe::egui;
use egui_kittest::Harness;
use serde_json::{json, Value as Json};
use std::path::Path;
use std::time::{Duration, Instant};

fn result(c: &mut Client, method: &str) -> Json {
    c.call(method, json!({})).unwrap()["result"].clone()
}

#[test]
fn the_viewport_shows_every_frame_the_game_plays() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/hollow")
        .canonicalize()
        .unwrap();
    let name = format!("{}-keep-up", socket_name_for(&root));
    let (r, n) = (root.clone(), name.clone());
    std::thread::spawn(move || {
        let d = common::open(&r);
        let _ = serve(d, Transport::Socket(n), None);
    });
    let mut agent = loop {
        if let Ok(c) = Client::connect(&name) {
            break c;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut h = Harness::builder()
        .with_size(egui::vec2(1600.0, 900.0))
        .build_state(|ctx, ed: &mut Editor| ed.ui(ctx), {
            let mut ed = Editor::new(&name, "human");
            ed.audio = false;
            ed
        });
    let t0 = Instant::now();
    while h.state().viewport.frames_shown < 3 && t0.elapsed() < Duration::from_secs(20) {
        h.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    agent.call("sim.play", json!({})).unwrap();
    let tick0 = result(&mut agent, "sim.status")["tick"].as_u64().unwrap();
    let drawn0 = result(&mut agent, "render.status")["stream"]["frames"]
        .as_u64()
        .unwrap();
    let shown0 = h.state().viewport.frames_shown;
    let mut steps = Vec::new();
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(4) {
        let s = Instant::now();
        h.step();
        steps.push(s.elapsed().as_secs_f64() * 1000.0);
        // A window repaints as frames arrive, about 60 times a second.
        std::thread::sleep(Duration::from_millis(16).saturating_sub(s.elapsed()));
    }
    let secs = t0.elapsed().as_secs_f64();
    let ticks = (result(&mut agent, "sim.status")["tick"].as_u64().unwrap() - tick0) as f64 / secs;
    let drawn = (result(&mut agent, "render.status")["stream"]["frames"]
        .as_u64()
        .unwrap()
        - drawn0) as f64
        / secs;
    let shown = (h.state().viewport.frames_shown - shown0) as f64 / secs;
    steps.sort_by(f64::total_cmp);
    let p95 = steps[steps.len() * 95 / 100];
    println!(
        "ticks {ticks:.1}/s, frames drawn {drawn:.1}/s, shown {shown:.1}/s, an editor frame p95 {p95:.1} ms"
    );
    assert!(
        ticks > 55.0,
        "the game plays in real time: {ticks:.1} ticks a second"
    );
    assert!(
        drawn > 50.0,
        "the daemon draws a frame each tick: {drawn:.1} a second"
    );
    assert!(shown > 45.0, "the editor shows them: {shown:.1} a second");
    assert!(
        p95 < 12.0,
        "an editor frame fits in a display's: p95 {p95:.1} ms"
    );
}

/// The window's path: frames straight from the shared file into a GPU
/// texture of the editor's own (`viewport::Gpu`), which the headless
/// harness does not draw with; here it is made on a device of its own, as
/// eframe makes it for the window, and takes Outpost's frames.
#[test]
fn frames_go_straight_into_the_windows_gpu() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/outpost")
        .canonicalize()
        .unwrap();
    let name = format!("{}-gpu", socket_name_for(&root));
    let (r, n) = (root.clone(), name.clone());
    std::thread::spawn(move || {
        let d = common::open(&r);
        let _ = serve(d, Transport::Socket(n), None);
    });
    let mut agent = loop {
        if let Ok(c) = Client::connect(&name) {
            break c;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let config = eframe::egui_wgpu::WgpuConfiguration::default();
    let instance = pollster::block_on(config.wgpu_setup.new_instance());
    let state = pollster::block_on(eframe::egui_wgpu::RenderState::create(
        &config, &instance, None, None, 1, false,
    ))
    .expect("a GPU");
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_state(|ctx, ed: &mut Editor| ed.ui(ctx), {
            let mut ed = Editor::new(&name, "human").with_gpu(Some(state));
            ed.audio = false;
            ed
        });
    agent.call("sim.play", json!({})).unwrap();
    let t0 = Instant::now();
    while h.state().viewport.frames_shown < 30 && t0.elapsed() < Duration::from_secs(20) {
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(h.state().viewport.gpu.is_some());
    assert!(
        h.state().viewport.frames_shown >= 30,
        "{} frames into the texture",
        h.state().viewport.frames_shown
    );
}
