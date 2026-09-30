//! Phase 4 item 3: the tuning panel, driven headless as a person would, on
//! Outpost playing at 10x over a real socket (the gate: "a human tunes
//! Outpost's food economy at 10x with sliders; the agent sees the slider
//! path as data"). Sliders dragged step by step: each step a `tune.set`
//! logged with its frame; Show as command lists the value let go at; Keep
//! saves the colony's field to its file and a meal's food to its table.

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
        "timed out waiting for {what}; problem {:?}; session {}; story {:?}",
        h.state().problem,
        h.state().tuning.session,
        h.state().tuning.story
    );
}

/// Press on a slider a fraction along it, move in steps, and let go.
fn drag(h: &mut Harness<'_, Editor>, label: &str, from: f32, to: f32) {
    let b = h
        .get_all_by_label(label)
        .find(|n| n.role() == egui::accesskit::Role::Slider)
        .expect("a slider")
        .bounding_box()
        .expect("placed");
    let at = |f: f32| {
        Pos2::new(
            b.x0 as f32 + (b.x1 - b.x0) as f32 * f,
            ((b.y0 + b.y1) / 2.0) as f32,
        )
    };
    let press = |h: &mut Harness<'_, Editor>, p: Pos2, pressed: bool| {
        h.input_mut().events.push(Event::PointerMoved(p));
        h.input_mut().events.push(Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    };
    press(h, at(from), true);
    for i in 1..=8 {
        let p = at(from + (to - from) * i as f32 / 8.0);
        h.input_mut().events.push(Event::PointerMoved(p));
        h.step();
        // A person's hand, not a teleport: the game ticks between moves.
        std::thread::sleep(Duration::from_millis(40));
    }
    press(h, at(to), false);
    h.step();
}

fn result(c: &mut Client, method: &str, params: Json) -> Json {
    let r = c.call(method, params).unwrap();
    assert!(r.get("error").is_none(), "{method}: {r}");
    r["result"].clone()
}

#[test]
fn a_person_tunes_the_food_economy_at_10x_and_keeps_it() {
    let t = common::temp("tuning-panel");
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/outpost"),
        t.path(),
    );
    let name = format!("{}-tuning", socket_name_for(t.path()));
    let (r, n) = (t.path().to_path_buf(), name.clone());
    std::thread::spawn(move || {
        let d = common::open(&r);
        let _ = serve(d, Transport::Socket(n), Some(&r));
    });
    let mut agent = loop {
        if let Ok(c) = Client::connect(&name) {
            break c;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut h = Harness::builder()
        .with_size(egui::vec2(1500.0, 950.0))
        .wgpu()
        .build_state(|ctx, ed: &mut Editor| ed.ui(ctx), {
            let mut ed = Editor::new(&name, "human");
            ed.audio = false;
            ed
        });
    until(&mut h, "the bar", |ed| ed.status["project"] == "outpost");
    h.get_by_label("Play").click();
    h.get_by_label("10x").click();
    until(&mut h, "playing at 10x", |ed| {
        ed.status["playing"] == true && ed.status["speed"] == 10.0
    });

    h.get_by_label("Tuning").click();
    until(&mut h, "the tune fields", |ed| {
        let rows = &ed.tuning.rows;
        rows.iter().any(|r| r["path"] == "Colony.hunger_per_day")
            && rows.iter().any(|r| r["def"] == "item:meal")
            && rows.iter().any(|r| r["label"] == "JobKind cook")
    });
    h.step();

    // The colony's hunger, dragged up in steps while the colony runs.
    let t0 = result(&mut agent, "sim.status", json!({}))["tick"]
        .as_u64()
        .unwrap();
    drag(&mut h, "hunger_per_day", 0.1, 0.3);
    let t1 = result(&mut agent, "sim.status", json!({}))["tick"]
        .as_u64()
        .unwrap();
    println!("ticks during the drag: {t0} to {t1}");
    until(&mut h, "a session", |ed| !ed.tuning.session.is_null());
    until(&mut h, "the search path", |ed| {
        ed.tuning
            .story
            .iter()
            .any(|l| l.starts_with("human set @colony Colony.hunger_per_day 1→"))
    });
    let log = result(
        &mut agent,
        "tune.log",
        json!({ "path": "Colony.hunger_per_day" }),
    );
    let steps = log["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["kind"] == "set")
        .count();
    assert!(steps >= 3, "each step logged with its frame: {log}");
    let frames: Vec<u64> = log["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["frame"].as_u64())
        .collect();
    assert!(
        frames.windows(2).any(|w| w[1] > w[0]),
        "at different frames, the game running: {frames:?}"
    );
    let lines: Vec<&String> = h
        .state()
        .commands
        .iter()
        .filter(|l| l.starts_with("argus tune set"))
        .collect();
    assert_eq!(lines.len(), 1, "one command for the drag: {lines:?}");
    let settled = log["rows"].as_array().unwrap().last().unwrap()["to"]
        .as_f64()
        .unwrap();
    assert!(settled > 1.0, "{settled}");

    // A meal's food, from the data table's row.
    h.state_mut().tuning.filter = "meal".into();
    h.step();
    h.step();
    drag(&mut h, "food", 0.1, 0.06);
    until(&mut h, "the meal's path", |ed| {
        ed.tuning.story.iter().any(|l| l.contains("item:meal food"))
    });
    let food = result(
        &mut agent,
        "world.defs",
        json!({ "table": "items", "where": { "id": "meal" } }),
    );
    let i = food["columns"]
        .as_array()
        .unwrap()
        .iter()
        .position(|c| c == "food")
        .unwrap();
    let meal = food["rows"][0][i].as_f64().unwrap();
    assert!(meal < 1.0, "the colony eats it now: {meal}");
    h.step();
    h.get_by_label_contains("2 fields moved");
    h.state_mut().tuning.filter.clear();
    h.step();
    h.step();
    let img = h.render().expect("drawn");
    let (w, hh) = img.dimensions();
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("editor-tuning.png");
    let f = std::io::BufWriter::new(std::fs::File::create(&out).unwrap());
    let mut enc = png::Encoder::new(f, w, hh);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&img.into_raw())
        .unwrap();

    // Keep: one change by the person, the files written.
    h.state_mut().tuning.note = "hungrier colonists, cheaper meals".into();
    h.get_by_label("Keep").click();
    until(&mut h, "the session kept", |ed| ed.tuning.session.is_null());
    let colony = std::fs::read_to_string(t.path().join("scenes/valley/colony.toml")).unwrap();
    assert!(
        colony.contains(&format!("hunger_per_day = {}", settled)),
        "{colony}"
    );
    let items = std::fs::read_to_string(t.path().join("defs/items.toml")).unwrap();
    assert!(items.contains(&format!("food = {meal}")), "{items}");
    let feed = result(&mut agent, "edit.feed", json!({ "limit": 500 }));
    let last = feed["rows"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["author"], "human");
    assert_eq!(last["note"], "hungrier colonists, cheaper meals");
    assert_eq!(
        last["command"],
        "argus tune end commit=true note='hungrier colonists, cheaper meals'"
    );
    // The agent reads the path as data, for its sweep.
    let log = result(&mut agent, "tune.log", json!({}));
    let story = log["story"].to_string();
    assert!(story.contains("@colony Colony.hunger_per_day"), "{story}");
    assert!(story.contains("item:meal food"), "{story}");
}
