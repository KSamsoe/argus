//! Marks (§10, phase 2 item 2) through the protocol: dropped, queued,
//! reproduced by a daemon that did not take them, compared after a code
//! change, and imported from the web build.

mod common;

use common::*;
use serde_json::{json, Value as Json};
use std::path::Path;

/// Walk right, fire once, walk on: 400 ticks of the fixture's arena.
fn play(d: &mut argus_daemon::daemon::Daemon) {
    ok(
        d,
        "input.inject",
        json!({ "action": "move", "value": [1, 0], "frames": 200 }),
    );
    ok(d, "sim.step", json!({ "n": 150 }));
    ok(d, "input.inject", json!({ "action": "fire", "frames": 3 }));
    ok(d, "sim.step", json!({ "n": 250 }));
}

fn has_adapter(m: &Json) -> bool {
    m["picture"]["skipped"].is_null()
}

#[test]
fn a_mark_is_dropped_with_its_files_and_queued() {
    let t = temp("marks-drop");
    project(t.path());
    let mut d = open(t.path());
    play(&mut d);
    let m = ok(
        &mut d,
        "mark.drop",
        json!({ "note": "the shot went nowhere" }),
    );
    assert_eq!(m["id"], 1);
    assert_eq!(m["frame"], 400);
    assert_eq!(m["status"], "open");
    let dir = Path::new(m["dir"].as_str().unwrap());
    assert!(dir.join("mark.json").is_file() && dir.join("replay.bin").is_file());

    let g = ok(&mut d, "mark.get", json!({ "id": 1 }));
    assert_eq!(g["scene"], "arena");
    assert_eq!(g["seed"], 7);
    assert_eq!(g["note"], "the shot went nowhere");
    assert_eq!(g["digest"].as_str().unwrap().len(), 16);
    // The five seconds before: the move held since before them, the shot.
    assert_eq!(g["input"]["from"], 100);
    assert_eq!(g["input"]["held"][0][2], "move", "{}", g["input"]);
    assert!(g["input"]["changes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r[2] == "fire"));
    assert_eq!(g["events"]["counts"]["shot"], 1, "{}", g["events"]);
    assert!(g["replay"]["from"].as_u64().unwrap() <= 100);
    if has_adapter(&g) {
        assert!(dir.join("frame.png").is_file());
        // Two seconds at 30 pictures a second, and the first.
        assert_eq!(g["clip"]["frames"], 61, "{}", g["clip"]);
        let png = std::fs::read(dir.join("clip.png")).unwrap();
        assert!(png.windows(4).any(|w| w == b"acTL"), "an animated PNG");
        assert!(
            dir.join("clip-sheet.png").is_file(),
            "and eight of it in one picture"
        );
    } else {
        eprintln!(
            "no graphics adapter: the pictures were skipped: {}",
            g["picture"]
        );
    }

    let n = ok(
        &mut d,
        "mark.note",
        json!({ "id": 1, "note": "the shot never lands" }),
    );
    assert_eq!(n["note"], "the shot never lands");
    ok(&mut d, "sim.step", json!({ "n": 30 }));
    let m2 = ok(&mut d, "mark.drop", json!({}));
    assert_eq!(m2["id"], 2);
    let q = ok(&mut d, "mark.list", json!({}));
    assert_eq!(q["open"], 2);
    assert_eq!(ok(&mut d, "session.status", json!({}))["marks_open"], 2);
    assert_eq!(q["rows"][0]["id"], 1, "oldest first");
    ok(&mut d, "mark.close", json!({ "id": 1, "note": "fixed" }));
    let q = ok(&mut d, "mark.list", json!({}));
    assert_eq!(
        (q["open"].clone(), q["closed"].clone()),
        (json!(1), json!(1))
    );
    assert_eq!(
        ok(&mut d, "mark.list", json!({ "all": true }))["rows"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let ev = ok(&mut d, "obs.events", json!({ "name": "mark.dropped" }));
    assert_eq!(ev["rows"].as_array().unwrap().len(), 2, "{ev}");
    assert_eq!(
        err(&mut d, "mark.get", json!({ "id": 9 })).code,
        argus_daemon::codes::NO_MARK
    );
}

#[test]
fn a_mark_reproduces_in_a_daemon_that_did_not_take_it() {
    let t = temp("marks-reproduce");
    project(t.path());
    let (digest, x) = {
        let mut d = open(t.path());
        play(&mut d);
        let m = ok(&mut d, "mark.drop", json!({ "note": "here" }));
        let g = ok(&mut d, "mark.get", json!({ "id": m["id"] }));
        let x = ok(&mut d, "world.get", json!({ "entity": "@player" }))["components"]["Transform"]
            ["pos"][0]
            .clone();
        // The run goes on, then this daemon goes away.
        ok(&mut d, "sim.step", json!({ "n": 200 }));
        (g["digest"].clone(), x)
    };
    let mut d = open(t.path());
    let r = ok(&mut d, "mark.open", json!({ "id": 1 }));
    assert_eq!(r["frame"], 400, "{r}");
    assert_eq!(r["digest"]["mark"], digest);
    assert_eq!(r["digest"]["same"], true, "{r}");
    assert_eq!(r["against_mark"]["same"], true, "{r}");
    assert_eq!(
        r["replayed"].as_u64().unwrap() + r["from"].as_u64().unwrap(),
        400
    );
    let p = ok(&mut d, "world.get", json!({ "entity": "@player" }));
    assert_eq!(p["components"]["Transform"]["pos"][0], x);
    // The lead-in played: its events are in the log, and a seek moves in it.
    let shots = ok(&mut d, "obs.events", json!({ "name": "shot" }));
    assert_eq!(shots["rows"].as_array().unwrap().len(), 1, "{shots}");
    let s = ok(&mut d, "sim.seek", json!({ "frame": 300 }));
    assert_eq!(s["tick"], 300);
    assert!(s["diverged"].is_null(), "{s}");
    let s = ok(&mut d, "sim.seek", json!({ "frame": 400 }));
    assert_eq!(s["tick"], 400);
}

#[test]
fn a_mark_opened_after_a_change_says_what_changed_at_its_frame() {
    let t = temp("marks-changed");
    project(t.path());
    {
        let mut d = open(t.path());
        play(&mut d);
        ok(&mut d, "mark.drop", json!({ "note": "walks too slowly" }));
    }
    // The fix: a faster walk, and Health gains a field.
    let mv = read(t.path(), "systems/move.luau").replace("m.x * 2", "m.x * 3");
    write(t.path(), "systems/move.luau", &mv);
    let hp = read(t.path(), "components/health.luau").replace(
        "  max = field.i32 { default = 10, doc = \"Maximum\" },",
        "  max = field.i32 { default = 10, doc = \"Maximum\" },\n  regen = field.f32 { default = 0.5 },",
    );
    write(t.path(), "components/health.luau", &hp);
    let mut d = open(t.path());
    let r = ok(&mut d, "mark.open", json!({ "id": 1 }));
    assert_eq!(r["digest"]["same"], false, "{r}");
    assert_eq!(r["remapped"], json!(["Health"]), "{r}");
    let changed = &r["against_mark"];
    assert_eq!(changed["same"], false);
    let first = changed["first"].as_array().unwrap();
    let player = first
        .iter()
        .find(|c| c["entity"] == "@player")
        .expect("the player moved differently");
    let pos = player["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f[0] == "Transform.pos")
        .unwrap();
    assert!(
        pos[2][0].as_f64().unwrap() > pos[1][0].as_f64().unwrap(),
        "faster: {pos}"
    );
}

/// Copy a project, without what a daemon writes.
fn copy(from: &Path, to: &Path) {
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let name = e.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let (src, dst) = (e.path(), to.join(&name));
        if src.is_dir() {
            std::fs::create_dir_all(&dst).unwrap();
            copy(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).unwrap();
        }
    }
}

#[test]
fn a_web_mark_reaches_the_same_world_in_the_daemon() {
    let hollow = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hollow");
    let t = temp("marks-web");
    copy(&hollow, t.path());
    // The web player, as the browser runs it: right for a second and a half.
    let mut p = argus_web::Player::open(t.path(), None, None).unwrap();
    p.key("ArrowRight", true);
    p.step(90).unwrap();
    p.key("ArrowRight", false);
    p.step(30).unwrap();
    let web = p.mark("from the browser", "web-test");
    std::fs::write(t.path().join("web-mark.json"), web.to_string()).unwrap();

    let mut d = open(t.path());
    let m = ok(&mut d, "mark.import", json!({ "path": "web-mark.json" }));
    assert_eq!(m["source"], "web");
    assert_eq!(m["note"], "from the browser");
    let r = ok(&mut d, "mark.open", json!({ "id": m["id"] }));
    assert_eq!(r["frame"], 120, "{r}");
    assert_eq!(
        r["digest"]["same"], true,
        "the daemon reached the browser's world: {r}"
    );
    assert!(r["against_mark"].is_null());
}

#[test]
fn a_mark_on_a_discrete_clock_counts_turns() {
    let t = temp("marks-turns");
    project(t.path());
    write(
        t.path(),
        "scenes/delve/hero.toml",
        "tags = [\"player\"]

[PlayerSlot]
slot = 1

[Actions]

[Transform]
pos = [0.0, 0.0]

[Health]
cur = 5
",
    );
    let mut d = open(t.path());
    ok(&mut d, "scene.load", json!({ "scene": "delve" }));
    for _ in 0..14 {
        ok(
            &mut d,
            "input.inject",
            json!({ "action": "move", "value": [0, 1], "frames": 1 }),
        );
        ok(&mut d, "sim.advance", json!({ "n": 1 }));
    }
    let m = ok(&mut d, "mark.drop", json!({ "note": "turn fourteen" }));
    assert_eq!(m["frame"], 14);
    let g = ok(&mut d, "mark.get", json!({ "id": 1 }));
    assert_eq!(g["clock"], "discrete");
    assert_eq!(g["input"]["from"], 4, "ten turns of lead-in");
    if !g["clip"]["skipped"].is_string() {
        assert_eq!(
            g["clip"]["frames"], 9,
            "the last eight turns, and the one before: {}",
            g["clip"]
        );
        assert_eq!(g["clip"]["fps"], 4.0);
    }
    let y = ok(&mut d, "world.get", json!({ "entity": "@hero" }))["components"]["Transform"]["pos"]
        [1]
    .clone();
    assert!(y.as_f64().unwrap() > 20.0, "the hero walked: {y}");
    ok(&mut d, "sim.advance", json!({ "n": 5 }));
    drop(d);
    let mut d = open(t.path());
    let r = ok(&mut d, "mark.open", json!({ "id": 1 }));
    assert_eq!(r["frame"], 14);
    assert_eq!(r["digest"]["same"], true, "{r}");
    let p = ok(&mut d, "world.get", json!({ "entity": "@hero" }));
    assert_eq!(p["components"]["Transform"]["pos"][1], y);
}
