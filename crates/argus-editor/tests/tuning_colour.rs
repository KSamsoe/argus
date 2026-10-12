//! Phase 5 item 3: a light's colour in the tuning panel. A field that is not
//! a number has the control its value needs: a colour is its swatch and its
//! hex, sent as a whole when Enter is pressed (a half-typed colour is no
//! colour), logged as any tune is, and kept to the entity's file.

#[path = "../../argus-daemon/tests/common/mod.rs"]
mod common;

use argus_client::Client;
use argus_daemon::{serve, socket_name_for, Transport};
use argus_editor::Editor;
use eframe::egui::{self, accesskit::Role, Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use serde_json::{json, Value as Json};
use std::time::{Duration, Instant};

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

fn key(h: &mut Harness<'_, Editor>, key: Key, modifiers: Modifiers) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers,
        });
        h.step();
    }
}

fn result(c: &mut Client, method: &str, params: Json) -> Json {
    let r = c.call(method, params).unwrap();
    assert!(r.get("error").is_none(), "{method}: {r}");
    r["result"].clone()
}

#[test]
fn a_person_types_a_lights_colour_and_keeps_it() {
    let t = common::temp("tuning-colour");
    common::project(t.path());
    common::write(
        t.path(),
        "scenes/arena/lamp.toml",
        "[Transform]\npos = [2.0, 1.0]\n\n[Light]\ncolor = \"#FFAA00\"\nradius = 4.0\n",
    );
    let name = format!("{}-colour", socket_name_for(t.path()));
    let (r, n) = (t.path().to_path_buf(), name.clone());
    let thread = std::thread::spawn(move || {
        let d = common::open(&r);
        let _ = serve(d, Transport::Socket(n), Some(&r));
    });
    let _daemon = common::Served::new(&name, thread);
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut agent = loop {
        if let Ok(c) = Client::connect(&name) {
            break c;
        }
        assert!(Instant::now() < deadline, "the daemon did not come up");
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
    until(&mut h, "the bar", |ed| ed.status["project"] == "fixture");
    h.get_by_label("Tuning").click();
    until(&mut h, "the lamp's colour", |ed| {
        ed.tuning
            .rows
            .iter()
            .any(|r| r["entity"] == "@lamp" && r["path"] == "Light.color")
    });
    h.state_mut().tuning.filter = "lamp".into();
    h.step();
    h.step();

    // The colour's box: all of it chosen, the new colour typed, Enter.
    h.get_all_by_label("color")
        .find(|n| n.role() == Role::TextInput)
        .expect("the colour's box")
        .focus();
    h.step();
    key(&mut h, Key::A, Modifiers::COMMAND);
    h.input_mut().events.push(Event::Text("#40C0FF".into()));
    h.step();
    // Half typed, nothing is sent yet.
    assert!(
        h.state().tuning.session.is_null(),
        "no session before Enter"
    );
    key(&mut h, Key::Enter, Modifiers::NONE);
    until(&mut h, "the session", |ed| !ed.tuning.session.is_null());
    let g = result(&mut agent, "world.get", json!({ "entity": "@lamp" }));
    assert_eq!(g["components"]["Light"]["color"], "#40C0FF");
    let log = result(&mut agent, "tune.log", json!({ "path": "Light.color" }));
    let sets: Vec<&Json> = log["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["kind"] == "set")
        .collect();
    assert_eq!(sets.len(), 1, "one change, the whole colour: {log}");
    assert_eq!(sets[0]["from"], "#FFAA00");

    // Kept: the lamp's file has it.
    h.get_by_label("Keep").click();
    until(&mut h, "the session's end", |ed| {
        ed.tuning.session.is_null()
    });
    let file = std::fs::read_to_string(t.path().join("scenes/arena/lamp.toml")).unwrap();
    assert!(file.contains("color = \"#40C0FF\""), "{file}");
}
