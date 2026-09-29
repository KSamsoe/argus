//! A person playing, without a window (the phase 3 gate): Outpost's
//! inspector reached the way a window reaches it, the game playing, frames
//! in real time, keys and the mouse as the window sends them
//! (`Daemon::device_input`), not injected actions. Two of Kevin's play
//! sessions found faults on this path that tests driving actions missed:
//! a window opened stopped, and a scene load stopped the game.

mod common;

use argus_daemon::Daemon;
use argus_input::DeviceInput;
use common::*;
use serde_json::json;
use std::time::{Duration, Instant};

fn frames(d: &mut Daemon, ms: u64) {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_millis(ms) {
        if d.wants_frames() {
            d.frame().unwrap();
        }
        std::thread::sleep(Duration::from_millis(16));
    }
}

fn key(d: &mut Daemon, name: &str) {
    d.device_input(&[DeviceInput::Key {
        name: name.into(),
        pressed: true,
    }]);
    frames(d, 80);
    d.device_input(&[DeviceInput::Key {
        name: name.into(),
        pressed: false,
    }]);
    frames(d, 80);
}

fn state(d: &mut Daemon) -> (bool, String) {
    let q = ok(
        d,
        "world.query",
        json!({ "select": ["UiRect.shown", "Inspector.page"], "where": { "has": ["Inspector"] } }),
    );
    (
        q["rows"][0][0].as_bool().unwrap_or(false),
        q["rows"][0][1].as_str().unwrap_or("").to_string(),
    )
}

#[test]
fn a_person_opens_the_inspector_with_keys_and_the_mouse() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/outpost");
    let mut d = open(&root.canonicalize().unwrap());
    ok(&mut d, "sim.play", json!({}));
    frames(&mut d, 300);
    let t0 = ok(&mut d, "sim.status", json!({}))["tick"]
        .as_u64()
        .unwrap();
    assert!(t0 > 5, "it plays: tick {t0}");
    key(&mut d, "Tab");
    assert_eq!(state(&mut d), (true, "colony".to_string()), "Tab opens it");
    key(&mut d, "ArrowRight");
    key(&mut d, "Enter");
    assert_eq!(
        state(&mut d).1,
        "people",
        "the arrows and Enter reach People"
    );
    // The mouse: over the Jobs tab, and a click.
    let b = ok(
        &mut d,
        "world.query",
        json!({ "select": ["UiRect.pos", "UiRect.size", "UiButton.event"], "where": { "has": ["UiButton"] } }),
    );
    let mut at = (0.0, 0.0);
    for r in b["rows"].as_array().unwrap() {
        if r[2] == "inspect.tab.jobs" {
            at = (
                r[0][0].as_f64().unwrap() + r[1][0].as_f64().unwrap() / 2.0,
                r[0][1].as_f64().unwrap() + r[1][1].as_f64().unwrap() / 2.0,
            );
        }
    }
    d.device_input(&[DeviceInput::Pointer {
        x: at.0 as f32,
        y: at.1 as f32,
    }]);
    frames(&mut d, 80);
    d.device_input(&[DeviceInput::Mouse {
        button: "left".into(),
        pressed: true,
    }]);
    frames(&mut d, 80);
    d.device_input(&[DeviceInput::Mouse {
        button: "left".into(),
        pressed: false,
    }]);
    frames(&mut d, 80);
    assert_eq!(state(&mut d).1, "jobs", "a click on Jobs");
    key(&mut d, "Escape");
    assert!(!state(&mut d).0, "Esc closes it");
    let t1 = ok(&mut d, "sim.status", json!({}))["tick"]
        .as_u64()
        .unwrap();
    assert!(t1 > t0 + 30, "the game played on the while: {t0} to {t1}");
}
