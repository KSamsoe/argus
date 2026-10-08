//! Phase 5 item 3, the daemon's side of effect numbers in the tuning panel.
//! §13: "Feel is the thing the human tunes; every knob it needs must be a
//! field the tuning panel can show and the recorder can log." On a copy of
//! Swarm: the sparks' knobs (`assets/fx/pop.toml`) are rows of
//! `tune.fields`; a slider plays them from the next tick, logged with its
//! frame, the file left alone; the session's end writes the sidecar with
//! its comments; one undo puts back its bytes and the game plays the old
//! sparks; put back plays the file's sparks again. An effect is project
//! data, as a def row is: a change to one between ticks is a barrier
//! keyframe, and a replay from it reaches the same world.

mod common;

use argus_daemon::Daemon;
use common::*;
use serde_json::{json, Value as Json};
use std::path::Path;

const ME: u64 = 2;

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let name = e.file_name();
        let n = name.to_string_lossy();
        if n.starts_with('.') || n == "dist" {
            continue;
        }
        let (a, b) = (e.path(), to.join(&name));
        if a.is_dir() {
            copy(&a, &b);
        } else {
            std::fs::copy(&a, &b).unwrap();
        }
    }
}

fn me(d: &mut Daemon, method: &str, params: Json) -> Json {
    d.call(ME, method, &params)
        .unwrap_or_else(|e| panic!("{method}: {} ({})", e.message, e.hint))
}

fn refused(d: &mut Daemon, params: Json) -> argus_ecs::ArgusError {
    d.call(ME, "tune.set", &params)
        .expect_err("tune.set should have refused")
}

/// A copy of Swarm whose player is down, so nothing fires, nothing dies,
/// and the only sparks are the ones a test throws.
fn swarm(tag: &str) -> (Temp, Daemon) {
    let t = temp(tag);
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/swarm"),
        t.path(),
    );
    let mut d = open(t.path());
    me(&mut d, "session.identify", json!({ "author": "human" }));
    me(
        &mut d,
        "sim.set",
        json!({ "entity": "@player", "path": "Player.dead", "value": true }),
    );
    ok(&mut d, "sim.step", json!({ "n": 2 }));
    (t, d)
}

fn pop_rows(d: &mut Daemon) -> Vec<Json> {
    ok(d, "tune.fields", json!({ "asset": "fx/pop" }))["rows"]
        .as_array()
        .unwrap()
        .clone()
}

fn knob(d: &mut Daemon, path: &str) -> f64 {
    pop_rows(d)
        .iter()
        .find(|r| r["path"] == path)
        .unwrap_or_else(|| panic!("no knob {path}"))["value"]
        .as_f64()
        .unwrap()
}

/// Throw 300 sparks at the emitter, step `ticks`, and say how wide their
/// cloud is on screen (pixels of a 960 x 540 frame) and how many live.
fn burst(d: &mut Daemon, ticks: u64) -> (f64, u64) {
    me(
        d,
        "sim.set",
        json!({ "entity": "@sparks", "path": "Emitter.burst", "value": 300 }),
    );
    ok(d, "sim.step", json!({ "n": ticks }));
    let f = ok(
        d,
        "obs.frame",
        json!({ "format": "semantic", "width": 960, "height": 540 }),
    );
    let width = f["effects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "particles" && e["emitter"] == "@sparks")
        .map(|e| e["bounds"][2].as_f64().unwrap() - e["bounds"][0].as_f64().unwrap())
        .unwrap_or(0.0);
    (width, live(d))
}

/// Sparks alive, as the world has it.
fn live(d: &mut Daemon) -> u64 {
    ok(d, "world.get", json!({ "entity": "@sparks" }))["components"]["Emitter"]["live"]
        .as_u64()
        .unwrap()
}

#[test]
fn the_sparks_knobs_are_tuned_live_kept_in_their_file_and_undone_byte_for_byte() {
    let (t, mut d) = swarm("tune-effects");
    let file = t.path().join("assets/fx/pop.toml");
    let original = std::fs::read_to_string(&file).unwrap();

    // The panel's rows: the numbers a person tunes, in the file's shape.
    let all = ok(&mut d, "tune.fields", json!({}));
    let rows: Vec<Json> = all["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["asset"] == "fx/pop")
        .cloned()
        .collect();
    let paths: Vec<&str> = rows.iter().map(|r| r["path"].as_str().unwrap()).collect();
    assert_eq!(
        paths,
        vec![
            "lifetime[0]",
            "lifetime[1]",
            "speed[0]",
            "speed[1]",
            "spread",
            "drag",
            "size[0][1]",
            "size[1][1]"
        ],
        "not max, particle, material, z, or the colours"
    );
    let max = rows.iter().find(|r| r["path"] == "speed[1]").unwrap();
    assert_eq!(max["value"], 7.0);
    assert_eq!(max["range"], json!([0.0, 50.0]));
    assert_eq!(max["name"], "speed max");
    assert_eq!(max["label"], "fx/pop");
    assert_eq!(max["file"], "assets/fx/pop.toml");
    assert!(max["doc"].as_str().unwrap().contains("speed"), "{max}");
    assert!(
        all["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["asset"] != "fx/glow"),
        "a material has no knobs"
    );
    assert_eq!(pop_rows(&mut d).len(), 8, "one effect's, by asset");

    // The sparks as the file has them: a small cloud, gone in half a second.
    let (narrow, alive) = burst(&mut d, 6);
    assert!(narrow > 0.0 && alive == 300, "{narrow} {alive}");
    ok(&mut d, "sim.step", json!({ "n": 40 }));
    assert_eq!(live(&mut d), 0, "gone at 0.77 s");

    // Refused, and nothing changes: a min past its max (the importer's
    // sentence), out of the knob's range, a key that is no knob, an asset
    // that is no effect.
    let e = refused(
        &mut d,
        json!({ "asset": "fx/pop", "path": "speed[0]", "value": 30 }),
    );
    assert!(e.message.contains("min first"), "{}", e.message);
    let e = refused(
        &mut d,
        json!({ "asset": "fx/pop", "path": "speed[1]", "value": 1000 }),
    );
    assert!(e.message.contains("[0, 50]"), "{}", e.message);
    let e = refused(
        &mut d,
        json!({ "asset": "fx/pop", "path": "max", "value": 10 }),
    );
    assert!(e.message.contains("not one of its knobs"), "{}", e.message);
    assert!(e.hint.contains("speed[1]"), "{}", e.hint);
    let e = refused(
        &mut d,
        json!({ "asset": "fx/glow", "path": "speed", "value": 1 }),
    );
    assert!(e.message.contains("not a particle effect"), "{}", e.message);
    assert!(
        ok(&mut d, "tune.fields", json!({}))["session"].is_null(),
        "no session opened by a refusal"
    );

    // The slider: faster sparks, the max first. Live, logged, file as it was.
    let at = ok(&mut d, "sim.status", json!({}))["tick"]
        .as_u64()
        .unwrap();
    let r = me(
        &mut d,
        "tune.set",
        json!({ "asset": "fx/pop", "path": "speed[1]", "value": 40 }),
    );
    assert_eq!(
        (r["from"].clone(), r["to"].clone()),
        (json!(7.0), json!(40.0))
    );
    me(
        &mut d,
        "tune.set",
        json!({ "asset": "fx/pop", "path": "speed[0]", "value": 30 }),
    );
    assert_eq!(knob(&mut d, "speed[0]"), 30.0);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
    let (wide, _) = burst(&mut d, 6);
    assert!(
        wide > 3.0 * narrow,
        "the sparks born after the slider fly faster: {wide} px against {narrow}"
    );
    // A change between ticks: a barrier keyframe where it was made.
    let ring = ok(&mut d, "sim.ring", json!({}));
    assert!(
        ring["barrier_ticks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b == at),
        "a barrier at {at}: {ring}"
    );
    ok(&mut d, "sim.step", json!({ "n": 40 }));

    // Longer-lived sparks: the world sees it (Emitter.live).
    me(
        &mut d,
        "tune.set",
        json!({ "asset": "fx/pop", "path": "lifetime[1]", "value": 2 }),
    );
    me(
        &mut d,
        "tune.set",
        json!({ "asset": "fx/pop", "path": "lifetime[0]", "value": 1.5 }),
    );
    let (_, alive) = burst(&mut d, 46);
    assert_eq!(alive, 300, "alive at 0.77 s, where the file's were gone");

    // A run stays a value: back to a frame after the change, replayed, the
    // same world.
    let now = ok(&mut d, "sim.status", json!({ "digest": true }));
    let tick = now["tick"].as_u64().unwrap();
    let s = ok(&mut d, "sim.seek", json!({ "frame": tick - 20 }));
    assert!(s["diverged"].is_null(), "{s}");
    ok(&mut d, "sim.step", json!({ "n": 20 }));
    let again = ok(&mut d, "sim.status", json!({ "digest": true }));
    assert_eq!(again["tick"], now["tick"]);
    assert_eq!(
        again["digest"], now["digest"],
        "the replay reaches the same world"
    );

    // The search path, as the agent reads it.
    let log = ok(&mut d, "tune.log", json!({}));
    let story = log["story"].to_string();
    assert!(
        story.contains("human set fx/pop speed[1] 7→40 at frame"),
        "{story}"
    );
    assert!(story.contains("fx/pop speed[0] 3→30"), "{story}");
    assert!(story.contains("fx/pop lifetime[1] 0.45→2"), "{story}");
    let set = log["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == "set" && r["path"] == "speed[1]")
        .unwrap()
        .clone();
    assert_eq!(
        (set["asset"].clone(), set["via"].clone()),
        (json!(true), json!("tune"))
    );

    // Kept: one change by the person, the sidecar written, comments kept.
    let end = me(
        &mut d,
        "tune.end",
        json!({ "note": "faster, longer sparks" }),
    );
    assert!(end["commit"]["id"].is_u64(), "{end}");
    assert_eq!(end["commit"]["files"], json!(["assets/fx/pop.toml"]));
    let kept = std::fs::read_to_string(&file).unwrap();
    assert_eq!(
        kept,
        original
            .replace("lifetime = [0.2, 0.45]", "lifetime = [1.5, 2.0]")
            .replace("speed = [3, 7]", "speed = [30, 40]"),
        "the two values, and every comment and line as it was"
    );
    let feed = ok(&mut d, "edit.feed", json!({ "limit": 500 }));
    let last = feed["rows"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["author"], "human");
    assert_eq!(last["note"], "faster, longer sparks");
    assert!(
        last["ops"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| *o == json!(["asset", "fx/pop", "speed[1]", 40.0])),
        "edit.asset ops: {last}"
    );
    assert!(ok(&mut d, "tune.fields", json!({}))["session"].is_null());
    assert_eq!(knob(&mut d, "speed[1]"), 40.0);
    ok(&mut d, "sim.step", json!({ "n": 120 }));
    let (kept_wide, _) = burst(&mut d, 6);
    assert!(
        kept_wide > 3.0 * narrow,
        "the file's sparks now: {kept_wide}"
    );

    // One undo: the exact bytes, and the game plays the old sparks.
    me(&mut d, "edit.undo", json!({ "author": "human" }));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
    ok(&mut d, "sim.step", json!({ "n": 120 }));
    let (back, _) = burst(&mut d, 6);
    assert!(
        back < 2.0 * narrow,
        "imported again: {back} against {narrow}"
    );
    let log = ok(&mut d, "tune.log", json!({ "path": "speed[1]" }));
    assert!(
        log["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["via"] == "undo"
                && r["entity"] == "fx/pop"
                && r["from"] == 40.0
                && r["to"] == 7.0),
        "the undo is logged: {log}"
    );

    // Put back: the slider moved, then the session ended without keeping.
    ok(&mut d, "sim.step", json!({ "n": 40 }));
    me(
        &mut d,
        "tune.set",
        json!({ "asset": "fx/pop", "path": "speed[1]", "value": 45 }),
    );
    let (fast, _) = burst(&mut d, 6);
    assert!(fast > 3.0 * narrow, "{fast}");
    let end = me(&mut d, "tune.end", json!({ "commit": false }));
    assert!(end["commit"].is_null(), "{end}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
    assert_eq!(knob(&mut d, "speed[1]"), 7.0);
    ok(&mut d, "sim.step", json!({ "n": 40 }));
    let (again, _) = burst(&mut d, 6);
    assert!(again < 2.0 * narrow, "the file's sparks again: {again}");
    println!(
        "sparks across, in pixels: the file's {narrow}, tuned {wide}, kept {kept_wide}, undone {back}, tuned again {fast}, put back {again}"
    );

    // Another author's lock on the effect keeps the slider off it.
    ok(
        &mut d,
        "edit.lock",
        json!({ "author": "agent:claude", "assets": ["fx/pop"], "note": "mine" }),
    );
    let e = refused(
        &mut d,
        json!({ "asset": "fx/pop", "path": "drag", "value": 1 }),
    );
    assert_eq!(e.code, argus_daemon::codes::LOCKED, "{}", e.message);
}
