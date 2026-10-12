//! Phase 5 item 4 (§3): a prefab changed from the inspector. With "Change
//! the prefab" ticked, a field's change goes to the entity's prefab: the
//! prefab's file, every entity made from it (one whose own file sets the
//! field keeps its own), one line in the feed naming them all.

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
        "timed out waiting for {what}; problem {:?}",
        h.state().problem
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

fn call(c: &mut Client, method: &str, params: Json) -> Json {
    let r = c.call(method, params).unwrap();
    assert!(r.get("error").is_none(), "{method}: {r}");
    r["result"].clone()
}

fn health(c: &mut Client, id: &str) -> Json {
    call(c, "world.get", json!({ "entity": id }))["components"]["Health"].clone()
}

#[test]
fn a_person_changes_a_prefab_from_the_inspector() {
    let t = common::temp("inspector-prefab");
    common::project(t.path());
    common::write(
        t.path(),
        "scenes/arena/bat.big.toml",
        "prefab = \"enemy/bat\"\n\n[Transform]\npos = [6.0, 0.0]\n\n[Health]\nmax = 9\n",
    );
    common::write(
        t.path(),
        "scenes/arena/bat.small.toml",
        "prefab = \"enemy/bat\"\n\n[Transform]\npos = [8.0, 0.0]\n",
    );
    let socket = format!("{}-inspector-prefab", socket_name_for(t.path()));
    let (r, n) = (t.path().to_path_buf(), socket.clone());
    let thread = std::thread::spawn(move || {
        let d = common::open(&r);
        let _ = serve(d, Transport::Socket(n), Some(&r));
    });
    let _daemon = common::Served::new(&socket, thread);
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut agent = loop {
        if let Ok(c) = Client::connect(&socket) {
            break c;
        }
        assert!(Instant::now() < deadline, "the daemon did not come up");
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .wgpu()
        .build_state(|ctx, ed: &mut Editor| ed.ui(ctx), {
            let mut ed = Editor::new(&socket, "human");
            ed.audio = false;
            ed
        });
    until(&mut h, "the top level", |ed| {
        ed.hierarchy.top.rows.iter().any(|r| r.id == "@bat")
    });
    h.step();
    h.get_by_label("@bat").click();
    until(&mut h, "the bat inspected", |ed| {
        ed.inspector.got["resolved"]["prefab"] == "enemy/bat"
    });
    h.step();

    // Changes go to the prefab now.
    h.get_by_label("Change the prefab instead").click();
    h.step();
    assert!(h.state().inspector.to_prefab);
    // Health.max's box, on the row of its name.
    let row = h
        .get_all_by_label("max")
        .find(|n| n.role() == Role::Label)
        .expect("the max field's name")
        .bounding_box()
        .unwrap();
    let mid = (row.y0 + row.y1) / 2.0;
    h.get_all_by_role(Role::SpinButton)
        .find(|n| n.bounding_box().is_some_and(|b| b.y0 <= mid && mid <= b.y1))
        .expect("max's box")
        .click();
    h.step();
    key(&mut h, Key::A, Modifiers::COMMAND);
    h.input_mut().events.push(Event::Text("5".into()));
    h.step();
    key(&mut h, Key::Enter, Modifiers::NONE);

    // The prefab's file, both plain bats, and the big one keeping its own.
    let file = t.path().join("prefabs/enemy/bat.toml");
    let deadline = Instant::now() + Duration::from_secs(15);
    while !std::fs::read_to_string(&file).unwrap().contains("max = 5") {
        assert!(Instant::now() < deadline, "the prefab's file never changed");
        h.step();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(health(&mut agent, "@bat")["max"], 5);
    assert_eq!(health(&mut agent, "@bat.small")["max"], 5);
    assert_eq!(health(&mut agent, "@bat.big")["max"], 9);
    // The bat's own file says nothing of it.
    let own = std::fs::read_to_string(t.path().join("scenes/arena/bat.toml")).unwrap();
    assert!(!own.contains("max"), "{own}");
    // One line in the feed, naming every bat.
    let feed = call(&mut agent, "edit.feed", json!({ "limit": 5 }));
    let row = feed["rows"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|r| r["author"] == "human")
        .cloned()
        .expect("the person's change");
    let touched = row["touched"].to_string();
    for id in ["@bat", "@bat.small", "@bat.big", "prefab:enemy/bat"] {
        assert!(touched.contains(id), "{id} in {row}");
    }
}
