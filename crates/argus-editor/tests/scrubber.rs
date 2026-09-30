//! Phase 4 item 5: the replay scrubber, driven headless over a real socket
//! on a copy of the fixture project. The run is drawn with its mark and
//! the agent's edit; a drag back is one seek command and the mark stays
//! drawn; a frame at a time either way; Play replays the recorded run and a
//! drag while playing pauses and plays on; Try from here makes a world to
//! play in and the run stays; a failing test's replay opens from the list.

#[path = "../../argus-daemon/tests/common/mod.rs"]
mod common;

use argus_client::Client;
use argus_daemon::{serve, socket_name_for, Transport};
use argus_editor::Editor;
use eframe::egui::{self, Event, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use serde_json::{json, Value as Json};
use std::path::Path;
use std::time::{Duration, Instant};

fn daemon(root: &Path) -> String {
    common::project(root);
    let name = format!("{}-scrub", socket_name_for(root));
    let (r, n) = (root.to_path_buf(), name.clone());
    std::thread::spawn(move || {
        let d = common::open(&r);
        let _ = serve(d, Transport::Socket(n), Some(&r));
    });
    for _ in 0..200 {
        if Client::connect(&name).is_ok() {
            return name;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the daemon did not come up");
}

fn until(h: &mut Harness<'_, Editor>, what: &str, ok: impl Fn(&Editor) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        h.step();
        if ok(h.state()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "timed out waiting for {what}; problem {:?}; status {}; ring {}",
        h.state().problem,
        h.state().status,
        h.state().scrubber.ring
    );
}

fn result(c: &mut Client, method: &str, params: Json) -> Json {
    let r = c.call(method, params).unwrap();
    assert!(r.get("error").is_none(), "{method}: {r}");
    r["result"].clone()
}

fn tick(ed: &Editor) -> u64 {
    ed.status["tick"].as_u64().unwrap_or(0)
}

/// The track's x for a tick, from its place on screen and the run's range.
fn x_for(h: &Harness<'_, Editor>, t: u64) -> (f32, f32) {
    let b = h
        .get_all_by_label("Run")
        .find(|n| n.role() == egui::accesskit::Role::Slider)
        .expect("the run's track")
        .bounding_box()
        .expect("placed");
    let (lo, hi) = h.state().scrubber.range();
    let f = (t - lo) as f32 / (hi - lo).max(1) as f32;
    (
        b.x0 as f32 + (b.x1 - b.x0) as f32 * f,
        ((b.y0 + b.y1) / 2.0) as f32,
    )
}

/// Press on the track at one tick, move in steps, let go at another.
fn drag(h: &mut Harness<'_, Editor>, from: u64, to: u64) {
    let (x0, y) = x_for(h, from);
    let (x1, _) = x_for(h, to);
    let button = |h: &mut Harness<'_, Editor>, x: f32, pressed: bool| {
        let p = Pos2::new(x, y);
        h.input_mut().events.push(Event::PointerMoved(p));
        h.input_mut().events.push(Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    };
    button(h, x0, true);
    for i in 1..=8 {
        let x = x0 + (x1 - x0) * i as f32 / 8.0;
        h.input_mut()
            .events
            .push(Event::PointerMoved(Pos2::new(x, y)));
        h.step();
        std::thread::sleep(Duration::from_millis(30));
    }
    button(h, x1, false);
    h.step();
}

fn near(a: u64, b: u64) -> bool {
    a.abs_diff(b) <= 2
}

#[test]
fn a_person_scrubs_the_run_tries_from_a_frame_and_opens_a_replay() {
    let t = common::temp("editor-scrub");
    let name = daemon(t.path());
    let mut agent = Client::connect(&name).unwrap();
    result(
        &mut agent,
        "session.identify",
        json!({ "author": "agent:claude" }),
    );
    result(
        &mut agent,
        "input.inject",
        json!({ "action": "move", "value": [1, 0], "frames": 600 }),
    );
    result(&mut agent, "sim.step", json!({ "n": 300 }));
    result(
        &mut agent,
        "mark.drop",
        json!({ "note": "the shot went nowhere", "author": "human", "clip": false }),
    );
    let tx = result(
        &mut agent,
        "edit.begin",
        json!({ "author": "agent:claude", "note": "tougher" }),
    )["tx"]
        .clone();
    result(
        &mut agent,
        "edit.set",
        json!({ "tx": tx, "entity": "@player", "path": "Health.max", "value": 5 }),
    );
    result(&mut agent, "edit.commit", json!({ "tx": tx }));
    result(&mut agent, "sim.step", json!({ "n": 300 }));
    // A failing test's replay, as the harness saves one.
    result(
        &mut agent,
        "replay.save",
        json!({
            "dir": ".argus/replays/a_shot_lands/seed3",
            "note": "expected 1 shot, got 0",
            "about": { "scenario": "a shot lands", "seed": 3 },
            "clip": false,
        }),
    );

    let mut h = Harness::builder()
        .with_size(egui::vec2(1500.0, 950.0))
        .wgpu()
        .build_state(|ctx, ed: &mut Editor| ed.ui(ctx), {
            let mut ed = Editor::new(&name, "human");
            ed.audio = false;
            ed
        });
    until(&mut h, "the run", |ed| {
        ed.scrubber.ring["end"] == 600
            && ed
                .scrubber
                .markers
                .values()
                .any(|m| m.name == "mark.dropped")
    });
    h.step();
    let run = h.state().scrubber.ring["run"].clone();
    assert!(h.state().scrubber.ring["barrier_ticks"]
        .as_array()
        .unwrap()
        .contains(&json!(300)));
    let mark = h
        .state()
        .scrubber
        .markers
        .values()
        .find(|m| m.name == "mark.dropped")
        .unwrap()
        .clone();
    assert_eq!(mark.tick, 300);
    assert_eq!(mark.text, "mark #1: the shot went nowhere");

    // A drag back: the game follows, one command, one row.
    drag(&mut h, 600, 150);
    until(&mut h, "back at 150", |ed| {
        near(tick(ed), 150) && ed.scrubber.at.is_none()
    });
    let at = tick(h.state());
    let lines: Vec<&String> = h
        .state()
        .commands
        .iter()
        .filter(|l| l.starts_with("argus sim seek"))
        .collect();
    assert_eq!(lines, vec![&format!("argus sim seek frame={at}")]);
    let feed = result(&mut agent, "edit.feed", json!({ "limit": 200 }));
    let seeks: Vec<&Json> = feed["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["method"] == "sim.seek")
        .collect();
    assert_eq!(seeks.len(), 1, "{feed}");
    assert_eq!(seeks[0]["author"], "human");
    assert_eq!(seeks[0]["command"], format!("argus sim seek frame={at}"));
    until(&mut h, "the run still there", |ed| {
        ed.scrubber.ring["end"] == 600
    });
    assert!(
        h.state().scrubber.markers.values().any(|m| m.tick == 300),
        "the mark is still on the run"
    );
    h.get_by_label_contains("Back in the run");
    save(&mut h, "editor-scrubber.png");

    // A frame at a time.
    h.get_by_label("Frame +1").click();
    until(&mut h, "one on", |ed| tick(ed) == at + 1);
    h.get_by_label("Frame -1").click();
    until(&mut h, "one back", |ed| tick(ed) == at);

    // A click lands where it is, and the frame on screen follows.
    let (x, y) = x_for(&h, 450);
    let started = Instant::now();
    h.input_mut()
        .events
        .push(Event::PointerMoved(Pos2::new(x, y)));
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton {
            pos: Pos2::new(x, y),
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    until(&mut h, "the frame at 450 on screen", |ed| {
        ed.viewport.info.as_ref().is_some_and(|i| near(i.tick, 450))
    });
    let ms = started.elapsed().as_secs_f64() * 1e3;
    println!("a click on the run to its frame on screen: {ms:.0} ms");
    assert!(ms < 1000.0, "{ms:.0} ms");

    // Play replays the run as it went; a drag while playing pauses and
    // plays on.
    h.get_by_label("Play").click();
    until(&mut h, "playing on", |ed| tick(ed) > 470);
    assert_eq!(
        h.state().scrubber.ring["end"],
        600,
        "replaying, not branching"
    );
    let now = tick(h.state());
    drag(&mut h, now, 100);
    until(&mut h, "playing on from 100", |ed| {
        ed.status["playing"] == true
            && ed.status["paused"] == false
            && (100..300).contains(&tick(ed))
    });
    let lines = h.state().commands.join("\n");
    assert!(
        lines.contains("argus sim pause\nargus sim seek frame=")
            && lines.ends_with("argus sim play"),
        "{lines}"
    );
    h.get_by_label("Pause").click();
    until(&mut h, "paused", |ed| ed.status["paused"] == true);

    // Try from here: a world of its own; the run stays.
    let from = tick(h.state());
    h.get_by_label("Try from here").click();
    until(&mut h, "in try-1", |ed| ed.scrubber.ring["fork"] == "try-1");
    assert_ne!(h.state().scrubber.ring["run"], run);
    result(
        &mut agent,
        "input.inject",
        json!({ "action": "move", "value": [-1, 0], "frames": 30, "fork": "try-1" }),
    );
    h.get_by_label("Frame +1").click();
    until(&mut h, "a step in the fork", |ed| tick(ed) == from + 1);
    let worlds = result(&mut agent, "sim.fork", json!({}));
    let main = worlds["worlds"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["name"] == "main")
        .cloned()
        .unwrap();
    assert_eq!(main["tick"], from, "main waits where it was");
    // Back to the run, by the world list.
    h.get_all_by_label("World")
        .find(|n| n.role() == egui::accesskit::Role::ComboBox)
        .expect("the world list")
        .click();
    h.step();
    h.get_by_label("main").click();
    until(&mut h, "in main", |ed| ed.scrubber.ring["fork"] == "main");
    until(&mut h, "the run as it was", |ed| {
        ed.scrubber.ring["run"] == run && ed.scrubber.ring["end"] == 600
    });

    // The failing test's replay, from the list.
    until(&mut h, "the replay listed", |ed| {
        ed.scrubber.replays["total"] == 1
    });
    h.get_by_label("Replays (1)").click();
    h.step();
    h.get_by_label("a shot lands");
    h.get_by_label("expected 1 shot, got 0");
    h.get_by_label("Open").click();
    until(&mut h, "the replay open", |ed| {
        ed.scrubber.ring["run"] != run && ed.scrubber.ring["tick"] == 600
    });
    let r = h.state().scrubber.ring.clone();
    assert_eq!(r["end"], 600);
    assert!(
        r["floor"].as_u64().unwrap() < 600,
        "its lead-in to scrub: {r}"
    );
    assert!(h
        .state()
        .commands
        .iter()
        .any(|l| l == "argus replay open path=.argus/replays/a_shot_lands/seed3"));
    // (kittest clicks through the accessibility tree, and egui closes a
    // popup on a pointer's click: the world list is still open.)
    h.press_key(egui::Key::Escape);
    h.step();
    save(&mut h, "editor-scrubber-replay.png");
}

fn save(h: &mut Harness<'_, Editor>, name: &str) {
    h.step();
    let img = h.render().expect("drawn");
    let (w, hh) = img.dimensions();
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let f = std::io::BufWriter::new(std::fs::File::create(&out).unwrap());
    let mut enc = png::Encoder::new(f, w, hh);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&img.into_raw())
        .unwrap();
}
