//! Phase 4 item 2: the hierarchy and the inspector, driven headless over a
//! real socket on a copy of the fixture project. The hierarchy is pages of
//! `world.query` with totals, a parent opened for its children; the
//! inspector shows where each value comes from (the entity's own file, a
//! prefab, the default), and every change is a transaction by the person,
//! in the change feed as the `argus edit` line; undo is the person's own,
//! and a lock fences the agent out.

#[path = "../../argus-daemon/tests/common/mod.rs"]
mod common;

use argus_client::Client;
use argus_daemon::{serve, socket_name_for, Transport};
use argus_editor::Editor;
use eframe::egui::{self, Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use serde_json::{json, Value as Json};
use std::time::{Duration, Instant};

fn daemon(root: &std::path::Path) -> (common::Served, String) {
    common::project(root);
    let name = format!("{}-inspector", socket_name_for(root));
    let (r, n) = (root.to_path_buf(), name.clone());
    let thread = std::thread::spawn(move || {
        let d = common::open(&r);
        let _ = serve(d, Transport::Socket(n), Some(&r));
    });
    let served = common::Served::new(&name, thread);
    for _ in 0..200 {
        if Client::connect(&name).is_ok() {
            return (served, name);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the daemon did not come up");
}

fn until(h: &mut Harness<'_, Editor>, what: &str, ok: impl Fn(&Editor) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        h.step();
        if ok(h.state()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "timed out waiting for {what}; problem {:?}; top {:?}; got {}",
        h.state().problem,
        h.state()
            .hierarchy
            .top
            .rows
            .iter()
            .map(|r| r.id.clone())
            .collect::<Vec<_>>(),
        h.state().inspector.got
    );
}

fn call(c: &mut Client, method: &str, params: Json) -> Json {
    c.call(method, params).unwrap()
}

fn agent_edit(c: &mut Client, ops: &[(&str, Json)]) -> Json {
    let tx = call(
        c,
        "edit.begin",
        json!({ "author": "agent:claude", "note": "agent" }),
    )["result"]["tx"]
        .clone();
    for (m, p) in ops {
        let mut p = p.clone();
        p["tx"] = tx.clone();
        let r = call(c, m, p);
        assert!(r.get("error").is_none(), "{m}: {r}");
    }
    call(c, "edit.commit", json!({ "tx": tx }))
}

fn has_row(ed: &Editor, author: &str, command: &str) -> bool {
    ed.feed
        .iter()
        .any(|r| r["author"] == author && r["command"] == command)
}

fn field(ed: &Editor, comp: &str, f: &str) -> Json {
    ed.inspector.got["components"][comp][f].clone()
}

#[test]
fn a_person_finds_inspects_edits_undoes_and_locks() {
    let t = common::temp("inspector");
    let (_daemon, socket) = daemon(t.path());
    let mut agent = Client::connect(&socket).unwrap();
    call(
        &mut agent,
        "session.identify",
        json!({ "author": "agent:claude" }),
    );
    // A child for the bat, so the tree has a branch.
    let r = agent_edit(
        &mut agent,
        &[(
            "edit.spawn",
            json!({ "id": "wing", "components": { "Parent": { "entity": "@bat" }, "Transform": { "pos": [0.5, 0.0] } } }),
        )],
    );
    assert!(r.get("error").is_none(), "{r}");

    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .wgpu()
        .build_state(|ctx, ed: &mut Editor| ed.ui(ctx), {
            let mut ed = Editor::new(&socket, "human");
            ed.audio = false;
            ed
        });

    // The hierarchy: the top level, with its total, and the bat's branch.
    until(&mut h, "the top level", |ed| {
        let ids: Vec<&str> = ed
            .hierarchy
            .top
            .rows
            .iter()
            .map(|r| r.id.as_str())
            .collect();
        ids.contains(&"@bat")
            && ids.contains(&"@door")
            && ids.contains(&"@player")
            && !ids.contains(&"@wing")
    });
    assert_eq!(
        h.state().hierarchy.top.total,
        3,
        "the child is not at the top"
    );
    until(&mut h, "the bat's children known", |ed| {
        ed.hierarchy.parents.contains("@bat")
    });
    h.step();
    h.get_by_label(">").click();
    until(&mut h, "the branch", |ed| {
        ed.hierarchy
            .children
            .get("@bat")
            .is_some_and(|p| p.rows.iter().any(|r| r.id == "@wing"))
    });
    h.step();
    h.get_by_label("@wing");

    // Select the bat: its health from the prefab, its place its own.
    h.get_by_label("@bat").click();
    until(&mut h, "the bat inspected", |ed| {
        field(ed, "Health", "cur") == 2
    });
    let got = h.state().inspector.got.clone();
    assert_eq!(
        got["resolved"]["components"]["Health"]["fields"]["cur"]["from"], "enemy/bat",
        "{got}"
    );
    assert_eq!(
        got["resolved"]["components"]["Transform"]["fields"]["pos"]["from"], "own",
        "{got}"
    );
    until(&mut h, "the prefab's count", |ed| {
        ed.inspector.instances.is_some()
    });
    h.step();
    h.get_by_label_contains("prefab enemy/bat");
    h.get_by_label("saved in scenes/arena/bat.toml");
    // The panels as a person sees them.
    h.step();
    let img = h.render().expect("drawn");
    let (w, hh) = img.dimensions();
    let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("editor-inspector.png");
    let f = std::io::BufWriter::new(std::fs::File::create(&out).unwrap());
    let mut enc = png::Encoder::new(f, w, hh);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&img.into_raw())
        .unwrap();

    // A change: a transaction by the person, one line in the feed, the file
    // written, the value now the entity's own.
    h.state_mut().edit(
        "Health.max on @bat",
        vec![json!({ "op": "set", "entity": "@bat", "path": "Health.max", "value": 9 })],
    );
    let line = "argus edit author=human note='Health.max on @bat' ops='[{entity: @bat, op: set, path: Health.max, value: 9}]'";
    until(&mut h, "the edit in the feed", |ed| {
        has_row(ed, "human", line)
    });
    until(&mut h, "the edit shown", |ed| {
        field(ed, "Health", "max") == 9
    });
    assert!(common::read(t.path(), "scenes/arena/bat.toml").contains("max = 9"));
    until(&mut h, "its provenance own", |ed| {
        ed.inspector.got["resolved"]["components"]["Health"]["fields"]["max"]["from"] == "own"
    });
    assert_eq!(
        h.state().commands.last().map(String::as_str),
        Some(line),
        "show as command"
    );

    // A box ticked on the door is a change too, from the widget itself.
    h.get_by_label("@door").click();
    until(&mut h, "the door inspected", |ed| {
        ed.inspector.got["id"] == "door"
    });
    h.step();
    let boxes: Vec<_> = h
        .get_all_by_role(egui::accesskit::Role::CheckBox)
        .filter(|n| n.label().is_none_or(|l| l.is_empty()))
        .collect();
    assert_eq!(boxes.len(), 1, "the door's one bool field");
    boxes[0].click();
    until(&mut h, "the door locked", |ed| {
        field(ed, "Door", "locked") == true
    });
    assert!(common::read(t.path(), "scenes/arena/door.toml").contains("locked = true"));

    // Undo takes back the person's last change, and only theirs.
    agent_edit(
        &mut agent,
        &[(
            "edit.set",
            json!({ "entity": "@player", "path": "Health.cur", "value": 1 }),
        )],
    );
    h.get_by_label("Undo").click();
    until(&mut h, "the door back", |ed| {
        field(ed, "Door", "locked") == false
    });
    until(&mut h, "the undo in the feed", |ed| {
        has_row(ed, "human", "argus edit undo author=human")
    });
    let p = call(&mut agent, "world.get", json!({ "entity": "@player" }));
    assert_eq!(
        p["result"]["components"]["Health"]["cur"], 1,
        "the agent's change stands"
    );
    // Ctrl+Z, with nothing holding the keys: the bat's max goes back too.
    h.input_mut().events.push(Event::Key {
        key: Key::Z,
        physical_key: Some(Key::Z),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    });
    h.step();
    let b = || {
        call(
            &mut Client::connect(&socket).unwrap(),
            "world.get",
            json!({ "entity": "@bat" }),
        )
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while b()["result"]["components"]["Health"]["max"] != 2 {
        assert!(Instant::now() < deadline, "Ctrl+Z undid the max");
        h.step();
        std::thread::sleep(Duration::from_millis(20));
    }

    // A lock: the agent's write into the bat fails, naming the person.
    h.get_by_label("@bat").click();
    until(&mut h, "the bat again", |ed| {
        ed.inspector.got["id"] == "bat"
    });
    h.step();
    h.get_all_by_label("Lock")
        .next()
        .expect("the entity's Lock")
        .click();
    until(&mut h, "the lock held", |ed| {
        ed.inspector.locks.iter().any(|l| l["author"] == "human")
    });
    let r = agent_edit(
        &mut agent,
        &[(
            "edit.set",
            json!({ "entity": "@bat", "path": "Health.cur", "value": 7 }),
        )],
    );
    let msg = r["error"]["message"].as_str().unwrap_or("").to_string();
    assert!(
        msg.contains("human"),
        "the agent is fenced out, and told by whom: {r}"
    );
    h.step();
    h.get_by_label_contains("locked by human");
    h.get_by_label("Unlock").click();
    until(&mut h, "the lock gone", |ed| ed.inspector.locks.is_empty());
    let r = agent_edit(
        &mut agent,
        &[(
            "edit.set",
            json!({ "entity": "@bat", "path": "Health.cur", "value": 7 }),
        )],
    );
    assert!(r.get("error").is_none(), "{r}");

    // A component added, through the same transaction path.
    h.state_mut().edit(
        "add Light to @bat",
        vec![json!({ "op": "add", "entity": "@bat", "component": "Light" })],
    );
    until(&mut h, "the light", |ed| {
        !field(ed, "Light", "intensity").is_null()
    });
    h.step();
    h.get_by_label("Light");

    // A field locked by itself: right-click its name.
    let b = h.get_by_label("intensity").bounding_box().expect("placed");
    let at = egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerMoved(at));
        h.input_mut().events.push(Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.step();
    h.get_by_label("Lock this field").click();
    until(&mut h, "the field's lock", |ed| {
        ed.inspector
            .locks
            .iter()
            .any(|l| l["paths"] == json!(["Light.intensity"]))
    });
    let r = agent_edit(
        &mut agent,
        &[(
            "edit.set",
            json!({ "entity": "@bat", "path": "Light.intensity", "value": 3.0 }),
        )],
    );
    assert!(
        r["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("human"),
        "{r}"
    );
    let r = agent_edit(
        &mut agent,
        &[(
            "edit.set",
            json!({ "entity": "@bat", "path": "Health.cur", "value": 6 }),
        )],
    );
    assert!(r.get("error").is_none(), "only the field is fenced: {r}");

    // A component the prefab gives, taken away: the inspector says so.
    h.state_mut().edit(
        "remove Health from @bat",
        vec![json!({ "op": "remove", "entity": "@bat", "component": "Health" })],
    );
    until(&mut h, "the removal said", |ed| {
        ed.inspector.overrides["removed"]
            .as_array()
            .is_some_and(|a| a.iter().any(|x| x == "Health"))
    });
    h.step();
    h.get_by_label_contains("taken from its prefab: Health");

    // A runtime entity: live only, no file.
    agent_edit(
        &mut agent,
        &[(
            "edit.spawn",
            json!({ "components": { "Transform": { "pos": [1.0, 1.0] }, "Health": { "cur": 4 } } }),
        )],
    );
    h.state_mut().hierarchy.stale();
    until(&mut h, "the runtime entity", |ed| {
        ed.hierarchy.top.rows.iter().any(|r| r.id.starts_with("e:"))
    });
    let rt = h
        .state()
        .hierarchy
        .top
        .rows
        .iter()
        .find(|r| r.id.starts_with("e:"))
        .unwrap()
        .id
        .clone();
    h.get_by_label(&rt).click();
    until(&mut h, "it inspected", |ed| field(ed, "Health", "cur") == 4);
    h.step();
    h.get_by_label_contains("made at run time");

    // The filter, and pages: fifty at a time with the total said.
    let spawns: Vec<(&str, Json)> = (0..60)
        .map(|i| {
            (
                "edit.spawn",
                json!({ "components": { "Transform": { "pos": [i as f64, 5.0] } } }),
            )
        })
        .collect();
    agent_edit(&mut agent, &spawns);
    h.state_mut().hierarchy.stale();
    until(&mut h, "a page of the many", |ed| {
        ed.hierarchy.top.total >= 64
    });
    assert_eq!(
        h.state().hierarchy.top.rows.len(),
        50,
        "a page, not the scene"
    );
    h.step();
    h.get_by_label_contains("more").click();
    until(&mut h, "the next page", |ed| {
        ed.hierarchy.top.rows.len() as u64 == ed.hierarchy.top.total
    });
    h.state_mut().hierarchy.has = "Door".into();
    h.state_mut().hierarchy.stale();
    until(&mut h, "only the door", |ed| ed.hierarchy.top.total == 1);
}
