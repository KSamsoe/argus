//! Phase 4 item 9: the timeline editor, driven headless over a real socket
//! on a copy of the fixture with a lamp and a timeline of two cues on its
//! track. The person plays it with the lamp as its actor, drags a cue
//! sooner and a tween longer, is refused a cue that cannot read, adds a cue
//! at the playhead, scrubs the play along the ruler, and undoes; each
//! change is one transaction in the table's file with its comment kept,
//! and each is a command in the feed.

#[path = "../../argus-daemon/tests/common/mod.rs"]
mod common;

use argus_client::Client;
use argus_daemon::{serve, socket_name_for, Transport};
use argus_editor::app::Tab;
use argus_editor::Editor;
use eframe::egui::{self, Event, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use serde_json::{json, Value as Json};
use std::path::Path;
use std::time::{Duration, Instant};

const SHOW: &str = r#"# The lamp comes up over a second, then flares.
["show"]
length = 2.0

["show.glow"]
timeline = "timeline:show"
track = "lamp"
at = 0.5
kind = "tween"
who = "actor"
path = "Light.intensity"
to = [5.0, 0.0]
duration = 1.0

["show.flare"]
timeline = "timeline:show"
track = "lamp"
at = 1.5
kind = "event"
effects = "actor:Light.intensity = 9; emit flared"
"#;

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
        "timed out waiting for {what}; problem {:?}; list {}",
        h.state().problem,
        h.state().timelines.list
    );
}

fn until_file(h: &mut Harness<'_, Editor>, root: &Path, what: &str, ok: impl Fn(&str) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        h.step();
        if ok(&common::read(root, "defs/timeline.toml")) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "timed out waiting for {what}; problem {:?}; file {}",
        h.state().problem,
        common::read(root, "defs/timeline.toml")
    );
}

fn result(c: &mut Client, method: &str, params: Json) -> Json {
    let r = c.call(method, params).unwrap();
    assert!(r.get("error").is_none(), "{method}: {r}");
    r["result"].clone()
}

/// The middle of a cue's bar (its header, once selected, has its name too).
fn centre(h: &Harness<'_, Editor>, label: &str) -> (f32, f32) {
    let b = h
        .get_all_by_label(label)
        .find(|n| n.role() == egui::accesskit::Role::Button)
        .unwrap_or_else(|| panic!("no bar {label}"))
        .bounding_box()
        .unwrap();
    (((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)
}

/// Press at a point, move by `dx` in steps, and let go: a person's drag.
fn drag(h: &mut Harness<'_, Editor>, (x, y): (f32, f32), dx: f32) {
    let button = |h: &mut Harness<'_, Editor>, x: f32, pressed: bool| {
        h.input_mut()
            .events
            .push(Event::PointerMoved(Pos2::new(x, y)));
        h.input_mut().events.push(Event::PointerButton {
            pos: Pos2::new(x, y),
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    };
    button(h, x, true);
    for i in 1..=10 {
        h.input_mut()
            .events
            .push(Event::PointerMoved(Pos2::new(x + dx * i as f32 / 10.0, y)));
        h.step();
    }
    button(h, x + dx, false);
    h.step();
}

fn number(text: &str, row: &str, field: &str) -> f64 {
    let at = text.find(&format!("[\"{row}\"]")).expect(row);
    let rest = &text[at..];
    let end = rest[1..].find("\n[").map(|e| e + 1).unwrap_or(rest.len());
    rest[..end]
        .lines()
        .find_map(|l| l.strip_prefix(&format!("{field} = ")))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or_else(|| panic!("{row} has no {field}: {text}"))
}

fn commands(ed: &Editor, start: &str) -> usize {
    ed.commands.iter().filter(|l| l.starts_with(start)).count()
}

#[test]
fn a_person_plays_drags_and_scrubs_a_timeline() {
    let t = common::temp("editor-timeline");
    common::project(t.path());
    common::write(
        t.path(),
        "scenes/arena/lamp.toml",
        "[Transform]\npos = [5.0, 0.0]\n\n[Light]\nintensity = 1.0\n",
    );
    common::write(t.path(), "defs/timeline.toml", SHOW);
    let name = format!("{}-timeline", socket_name_for(t.path()));
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
        .with_size(egui::vec2(1500.0, 1000.0))
        .wgpu()
        .build_state(|ctx, ed: &mut Editor| ed.ui(ctx), {
            let mut ed = Editor::new(&name, "human");
            ed.audio = false;
            ed
        });
    until(&mut h, "the bar", |ed| ed.status["project"] == "fixture");
    h.get_by_label("Timeline").click();
    until(&mut h, "the show's two cues", |ed| {
        ed.tab == Tab::Timeline
            && ed.timelines.chosen.as_deref() == Some("show")
            && ed.timelines.cues().len() == 2
    });
    h.step();
    h.get_by_label("show.glow");
    h.get_by_label("show.flare");
    h.get_by_label("show.glow end");
    assert!(
        h.query_by_label("show.flare end").is_none(),
        "an event has no duration to stretch"
    );

    // Played with the lamp as its actor, a second in.
    h.state_mut().select(Some("@lamp".into()));
    h.step();
    h.step();
    h.get_all_by_label("selected").next().unwrap().click();
    h.step();
    assert_eq!(h.state().timelines.actor, "@lamp");
    h.get_by_label("Play timeline").click();
    until(&mut h, "a play", |ed| ed.timelines.playing().is_some());
    result(&mut agent, "sim.step", json!({ "n": 60 }));
    until(&mut h, "a second in", |ed| {
        ed.timelines.head().is_some_and(|t| (t - 1.0).abs() < 0.02)
    });
    let lamp = result(&mut agent, "world.get", json!({ "entity": "@lamp" }));
    let i = lamp["components"]["Light"]["intensity"].as_f64().unwrap();
    assert!((i - 3.0).abs() < 1e-4, "halfway up its glow: {lamp}");
    let shown = h.state().timelines.playing().unwrap()["time"]
        .as_f64()
        .unwrap();
    h.get_by_label_contains(&format!("plays: {shown:.2} s of 2 s"));
    let feed = result(&mut agent, "edit.feed", json!({ "limit": 50 }));
    assert!(
        feed["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["command"] == "argus timeline play actor=@lamp name=show"),
        "{feed}"
    );

    // The flare dragged sooner: one change, its time snapped.
    let at = centre(&h, "show.flare");
    drag(&mut h, at, -45.0);
    until_file(&mut h, t.path(), "the flare sooner", |f| {
        number(f, "show.flare", "at") < 1.5
    });
    let text = common::read(t.path(), "defs/timeline.toml");
    assert!(
        text.starts_with("# The lamp comes up"),
        "comment kept: {text}"
    );
    let at = number(&text, "show.flare", "at");
    assert!(
        (at * 20.0 - (at * 20.0).round()).abs() < 1e-6,
        "{at} snapped"
    );
    assert_eq!(
        commands(h.state(), "argus edit author=human note='show.flare at"),
        1,
        "one change for the drag: {:?}",
        h.state().commands
    );
    // The glow's end dragged later: a longer tween.
    until(&mut h, "the flare moved", |ed| {
        ed.timelines
            .cues()
            .iter()
            .any(|c| c.key == "show.flare" && c.at < 1.5)
    });
    h.step();
    // Now inside the glow: a row of its own in the lane, to click.
    assert!(
        centre(&h, "show.flare").1 > centre(&h, "show.glow").1 + 10.0,
        "the flare below the glow it overlaps"
    );
    let at = centre(&h, "show.glow end");
    drag(&mut h, at, 30.0);
    until_file(&mut h, t.path(), "a longer glow", |f| {
        number(f, "show.glow", "duration") > 1.0
    });

    // A wait on an event needs a when: refused, with why, and nothing kept.
    h.get_all_by_label("show.flare")
        .find(|n| n.role() == egui::accesskit::Role::Button)
        .unwrap()
        .click();
    until(&mut h, "the flare selected", |ed| {
        ed.timelines.cue.as_deref() == Some("show.flare")
    });
    h.step();
    let before = common::read(t.path(), "defs/timeline.toml");
    h.get_by_label("hold the timeline until it is done").click();
    until(&mut h, "the refusal", |ed| {
        ed.problem
            .as_ref()
            .is_some_and(|p| p.message.contains("would not read"))
    });
    let p = h.state().problem.clone().unwrap();
    assert!(p.message.contains("needs a when"), "{p:?}");
    assert_eq!(common::read(t.path(), "defs/timeline.toml"), before);

    // A cue added at the playhead, its effects typed.
    // The new cue's field, not the flare's (a field of many lines).
    h.get_all_by_label("effects")
        .filter(|n| n.role() == egui::accesskit::Role::TextInput)
        .next_back()
        .expect("the new cue's effects")
        .type_text("actor:Light.intensity = 2; emit dimmed");
    h.step();
    h.get_by_label("Add cue").click();
    until_file(&mut h, t.path(), "the new cue", |f| {
        f.contains("[\"show.event1\"]")
    });
    let text = common::read(t.path(), "defs/timeline.toml");
    assert_eq!(
        number(&text, "show.event1", "at"),
        1.0,
        "at the playhead: {text}"
    );
    until(&mut h, "the new cue listed", |ed| {
        ed.timelines.cues().len() == 3
    });

    // Played on, then scrubbed back along the ruler (a seek goes over
    // what was played): one row in the feed, where it let go.
    result(&mut agent, "sim.step", json!({ "n": 50 }));
    until(&mut h, "1.8 s in", |ed| {
        ed.timelines.head().is_some_and(|t| t > 1.8)
    });
    h.step();
    let ruler = h.get_by_label("Timeline time").bounding_box().unwrap();
    let (x0, y) = (ruler.x0 as f32, ((ruler.y0 + ruler.y1) / 2.0) as f32);
    let width = (ruler.x1 - ruler.x0) as f32;
    drag(&mut h, (x0 + width * 0.85, y), -width * 0.55);
    until(&mut h, "the scrub let go", |ed| {
        commands(ed, "argus timeline seek") == 1 && ed.timelines.at.is_some()
    });
    let let_go = h.state().timelines.at.unwrap();
    assert!(let_go > 0.3 && let_go < 1.0, "{let_go}");
    until(&mut h, "the play there", |ed| {
        ed.timelines
            .playing()
            .and_then(|p| p["time"].as_f64())
            .is_some_and(|t| (t - let_go).abs() < 0.02)
    });
    let feed = result(&mut agent, "edit.feed", json!({ "limit": 50 }));
    let seeks: Vec<&Json> = feed["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| {
            r["command"]
                .as_str()
                .unwrap_or("")
                .starts_with("argus timeline seek")
        })
        .collect();
    assert_eq!(seeks.len(), 1, "one row for the drag: {feed}");
    assert!(
        h.state().timelines.note.is_none(),
        "{:?}",
        h.state().timelines.note
    );

    let img = h.render().expect("drawn");
    let (w, hh) = img.dimensions();
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("editor-timeline.png");
    let f = std::io::BufWriter::new(std::fs::File::create(&out).unwrap());
    let mut enc = png::Encoder::new(f, w, hh);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&img.into_raw())
        .unwrap();

    // Ctrl+Z: the new cue out again, the file as it was before it.
    h.press_key_modifiers(Modifiers::COMMAND, egui::Key::Z);
    until_file(&mut h, t.path(), "the cue undone", |f| {
        !f.contains("show.event1")
    });
}
