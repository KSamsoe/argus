//! Rust plugins through the protocol (phase 5 item 6), on a copy of Swarm,
//! whose chase is its plugin `swarm-hot`.
//!
//! §7: "The agent sees a compile error as a structured script.errors
//! response with file:line and the compiler's message, never as a crash";
//! "For Rust plugins, the daemon snapshots, unloads the old dylib, loads
//! the new one, restores, and continues." §17: "Rust plugin reload < 2 s
//! after compile" (the release gate below).

use argus_daemon::Daemon;
use serde_json::{json, Value as Json};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// A temp directory removed on drop.
struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let name = e.file_name();
        if name == ".argus" || name == "dist" {
            continue;
        }
        let (a, b) = (e.path(), to.join(&name));
        if a.is_dir() {
            copy_dir(&a, &b);
        } else {
            std::fs::copy(&a, &b).unwrap();
        }
    }
}

fn swarm(tag: &str) -> (Temp, Daemon) {
    let root = std::env::temp_dir().join(format!("argus-plugin-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/swarm"),
        &root,
    );
    let mut d = Daemon::new();
    d.call(
        0,
        "session.open",
        &json!({ "project": root.to_string_lossy(), "scene": "arena" }),
    )
    .unwrap();
    (Temp(root), d)
}

const LIB: &str = "plugins/swarm-hot/src/lib.rs";

fn edit(root: &Path, from: &str, to: &str) {
    let p = root.join(LIB);
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(text.contains(from), "{from}");
    std::fs::write(&p, text.replacen(from, to, 1)).unwrap();
}

fn row<'a>(rows: &'a Json, name: &str) -> &'a Json {
    rows["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == name)
        .unwrap_or_else(|| panic!("no {name}"))
}

#[test]
fn a_plugin_s_systems_errors_and_reload_go_through_the_protocol() {
    let (t, mut d) = swarm("protocol");
    let sys = d.call(0, "script.systems", &json!({})).unwrap();
    let chase = row(&sys, "sim.chase");
    assert_eq!(chase["plugin"], "swarm-hot");
    // After the dasher's rules too (the phase 5 gate's dasher): a held
    // dasher is left alone by the chase.
    assert_eq!(chase["after"], json!(["sim.waves", "sim.dash"]));
    assert_eq!(chase["writes"], json!(["Transform"]));
    assert!(chase["source"]
        .as_str()
        .unwrap()
        .starts_with("plugins/swarm-hot/src/lib.rs:"));
    assert!(row(&sys, "sim.waves")["plugin"].is_null());
    assert_eq!(d.call(0, "script.errors", &json!({})).unwrap()["total"], 0);
    d.call(0, "sim.step", &json!({ "n": 30 })).unwrap();

    // A mistake: a structured row, and the old library keeps running.
    edit(&t.0, "let len = d.length();", "let len: u32 = d.length();");
    let r = d.call(0, "script.reload", &json!({})).unwrap();
    assert_eq!(r["plugins"][0]["swapped"], false, "{r}");
    let errs = d.call(0, "script.errors", &json!({})).unwrap();
    let e = &errs["rows"][0];
    assert_eq!(e["phase"], "compile", "{errs}");
    assert_eq!(e["file"], LIB);
    assert_eq!(e["plugin"], "swarm-hot");
    assert!(
        e["message"].as_str().unwrap().contains("mismatched types"),
        "{e}"
    );
    let at = std::fs::read_to_string(t.0.join(LIB))
        .unwrap()
        .lines()
        .position(|l| l.contains("let len: u32"))
        .unwrap()
        + 1;
    assert_eq!(e["line"], at);
    let before = d.call(0, "sim.status", &json!({ "digest": true })).unwrap();
    d.call(0, "sim.step", &json!({ "n": 1 })).unwrap();
    assert!(
        d.call(0, "sim.status", &json!({})).unwrap()["tick"]
            == before["tick"].as_u64().unwrap() + 1
    );

    // Fixed, and changed: enemies stop dead. The swap keeps the world.
    edit(&t.0, "let len: u32 = d.length();", "let len = d.length();");
    edit(
        &t.0,
        "t.pos = t.pos + d * k;",
        "t.pos = t.pos + d * (k * 0.0);",
    );
    let digest = d.call(0, "sim.status", &json!({ "digest": true })).unwrap()["digest"].clone();
    let r = d.call(0, "script.reload", &json!({})).unwrap();
    assert_eq!(r["plugins"][0]["swapped"], true, "{r}");
    assert_eq!(r["errors"], 0, "{r}");
    assert_eq!(
        d.call(0, "sim.status", &json!({ "digest": true })).unwrap()["digest"],
        digest,
        "the world after the swap is the world before it"
    );
    assert_eq!(d.call(0, "script.errors", &json!({})).unwrap()["total"], 0);
    let where_ = |d: &mut Daemon| {
        d.call(
            0,
            "world.query",
            &json!({ "where": { "has": ["Enemy"] }, "select": ["Transform.pos"], "limit": 3 }),
        )
        .unwrap()["rows"]
            .clone()
    };
    let a = where_(&mut d);
    d.call(0, "sim.step", &json!({ "n": 5 })).unwrap();
    assert_eq!(where_(&mut d), a, "the new code runs: nobody moves");
}

/// §17 "Rust plugin reload < 2 s after compile": a one-line change to
/// Swarm's chase, rebuilt and swapped in through `script.reload`.
#[test]
#[ignore = "release-only perf gate; run with --release -- --ignored"]
fn perf_rust_plugin_reload_under_2_s_after_compile() {
    let (t, mut d) = swarm("perf");
    d.call(0, "sim.step", &json!({ "n": 60 })).unwrap();
    let mut worst: f64 = 0.0;
    for (i, k) in ["0.5", "0.25", "1.0"].iter().enumerate() {
        let from = if i == 0 {
            "d * k;".to_string()
        } else {
            format!("d * (k * {});", ["0.5", "0.25"][i - 1])
        };
        edit(&t.0, &from, &format!("d * (k * {k});"));
        let started = Instant::now();
        let r = d.call(0, "script.reload", &json!({})).unwrap();
        let all = started.elapsed().as_secs_f64() * 1e3;
        let p = &r["plugins"][0];
        assert_eq!(p["swapped"], true, "{r}");
        let reload = p["reload_ms"].as_f64().unwrap();
        println!(
            "change {i}: cargo {} ms, reload after compile {reload:.1} ms, script.reload whole {all:.0} ms",
            p["build_ms"]
        );
        worst = worst.max(reload);
        d.call(0, "sim.step", &json!({ "n": 10 })).unwrap();
    }
    assert!(worst < 2000.0, "reload after compile took {worst:.1} ms");
}

#[test]
fn a_plugin_that_never_built_leaves_the_game_running_without_it() {
    let root = std::env::temp_dir().join(format!("argus-plugin-never-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/swarm"),
        &root,
    );
    let t = Temp(root);
    edit(&t.0, "let len = d.length();", "let len = d.lenght();");
    let mut d = Daemon::new();
    d.call(
        0,
        "session.open",
        &json!({ "project": t.0.to_string_lossy(), "scene": "arena" }),
    )
    .expect("the session opens without its plugin");
    let errs = d.call(0, "script.errors", &json!({})).unwrap();
    let rows = errs["rows"].as_array().unwrap();
    assert!(
        rows.iter()
            .any(|r| r["phase"] == "compile" && r["file"] == LIB),
        "{errs}"
    );
    assert!(
        rows.iter()
            .any(|r| r["phase"] == "order" && r["system"] == "sim.fire"),
        "sim.fire ran after sim.chase: {errs}"
    );
    d.call(0, "sim.step", &json!({ "n": 10 })).unwrap();

    edit(&t.0, "let len = d.lenght();", "let len = d.length();");
    let r = d.call(0, "script.reload", &json!({})).unwrap();
    assert_eq!(r["plugins"][0]["swapped"], true, "{r}");
    assert_eq!(d.call(0, "script.errors", &json!({})).unwrap()["total"], 0);
    let sys = d.call(0, "script.systems", &json!({})).unwrap();
    assert_eq!(row(&sys, "sim.chase")["plugin"], "swarm-hot");
}

/// §17 Swarm "Sim tick, p95 < 6 ms", now on the game itself: its stress
/// scene's fifty thousand enemies, chased by the plugin. (The Luau chase
/// alone took 37 ms a tick here.)
#[test]
#[ignore = "release-only perf gate; run with --release -- --ignored"]
fn perf_swarm_stress_tick_under_6_ms_with_its_chase_in_rust() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/swarm");
    let mut d = Daemon::new();
    d.call(
        0,
        "session.open",
        &json!({ "project": root.canonicalize().unwrap().to_string_lossy(), "scene": "stress" }),
    )
    .unwrap();
    d.call(0, "sim.step", &json!({ "n": 30 })).unwrap();
    let enemies = d
        .call(
            0,
            "world.query",
            &json!({ "where": { "has": ["Enemy"] }, "limit": 1 }),
        )
        .unwrap()["total"]
        .as_u64()
        .unwrap();
    d.call(0, "sim.step", &json!({ "n": 120 })).unwrap();
    let p = d.call(0, "prof.systems", &json!({ "last": 120 })).unwrap();
    let p95 = p["tick_p95_ms"].as_f64().unwrap();
    let chase = p["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r[0] == "sim.chase")
        .map(|r| r[2].as_f64().unwrap())
        .unwrap();
    println!("Swarm stress, {enemies} enemies: tick p95 {p95:.3} ms, sim.chase mean {chase:.3} ms");
    assert!(enemies >= 49_000, "{enemies}");
    assert!(p95 < 6.0, "tick p95 {p95} ms");
    // And once the swarm has reached the hero, the heavy part of the run:
    // the window above ends before it, and the music's count of everything
    // near the hero cost 10.7 ms a tick there, a p95 of 12 to 13 ms (found
    // at the phase 5 gate by the agent who added the dasher).
    d.call(0, "sim.step", &json!({ "n": 930 })).unwrap();
    let p = d.call(0, "prof.systems", &json!({ "last": 120 })).unwrap();
    let close = p["tick_p95_ms"].as_f64().unwrap();
    let music = p["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r[0] == "late.music")
        .map(|r| r[2].as_f64().unwrap())
        .unwrap_or(0.0);
    println!("  at tick 1,200, the swarm on the hero: tick p95 {close:.3} ms, late.music mean {music:.3} ms");
    assert!(
        close < 6.0,
        "tick p95 {close} ms with the swarm on the hero"
    );
}
