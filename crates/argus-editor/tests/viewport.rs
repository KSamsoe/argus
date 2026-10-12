//! Phase 4 item 1: the viewport, driven headless as a person would, on
//! Outpost over a real socket. Frames arrive through the shared file; a
//! click puts the keys in the game, and Tab opens Outpost's inspector as
//! in the game window; F8 drops a mark whose note is typed there; a click
//! while paused selects a colonist. The keys are in the input log as
//! actions, and what F8 and Enter did is in the change feed, by the person.

#[path = "../../argus-daemon/tests/common/mod.rs"]
mod common;

use argus_client::Client;
use argus_daemon::{serve, socket_name_for, Transport};
use argus_editor::Editor;
use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2};
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

fn daemon(root: &Path) -> (common::Served, String) {
    let name = format!("{}-viewport", socket_name_for(root));
    let (r, n) = (root.to_path_buf(), name.clone());
    let thread = std::thread::spawn(move || {
        let d = common::open(&r);
        let _ = serve(d, Transport::Socket(n), Some(&r));
    });
    let served = common::Served::new(&name, thread);
    for _ in 0..500 {
        if Client::connect(&name).is_ok() {
            return (served, name);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the daemon did not come up");
}

fn until(h: &mut Harness<'_, Editor>, what: &str, mut ok: impl FnMut(&Editor) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        h.step();
        if ok(h.state()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "timed out waiting for {what}; frames {} {:?}; problem {:?}; status {}",
        h.state().viewport.frames_shown,
        h.state().viewport.info,
        h.state().problem,
        h.state().status
    );
}

fn call(c: &mut Client, method: &str, params: Json) -> Json {
    let r = c.call(method, params).unwrap();
    assert!(r.get("error").is_none(), "{method}: {r}");
    r["result"].clone()
}

fn click(h: &mut Harness<'_, Editor>, at: Pos2, modifiers: Modifiers) {
    h.input_mut().events.push(Event::PointerMoved(at));
    h.step();
    h.input_mut().events.push(Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed: true,
        modifiers,
    });
    h.step();
    h.input_mut().events.push(Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed: false,
        modifiers,
    });
    h.step();
}

fn press(h: &mut Harness<'_, Editor>, key: Key) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        h.step();
        std::thread::sleep(Duration::from_millis(60));
        h.step();
    }
}

fn inspector(c: &mut Client) -> bool {
    let q = call(
        c,
        "world.query",
        json!({ "select": ["UiRect.shown"], "where": { "has": ["Inspector"] } }),
    );
    q["rows"][0][0].as_bool().unwrap_or(false)
}

#[test]
fn a_person_plays_outpost_in_the_viewport() {
    let t = common::temp("viewport");
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/outpost"),
        t.path(),
    );
    let (_daemon, socket) = daemon(t.path());
    let mut agent = Client::connect(&socket).unwrap();

    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .wgpu()
        .build_state(|ctx, ed: &mut Editor| ed.ui(ctx), {
            let mut ed = Editor::new(&socket, "human");
            ed.audio = false;
            ed
        });
    // Frames, at the panel's size once it has settled.
    until(&mut h, "frames at the panel's size", |ed| {
        let v = &ed.viewport;
        v.frames_shown > 0 && v.info.is_some_and(|i| (i.width, i.height) == v.want)
    });
    h.step();
    let img = h.render().unwrap();
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("editor-viewport.png");
    let (w, hgt) = img.dimensions();
    let f = std::io::BufWriter::new(std::fs::File::create(&out).unwrap());
    let mut enc = png::Encoder::new(f, w, hgt);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&img.into_raw())
        .unwrap();
    println!("the editor with the valley: {}", out.display());

    // Play, click into the game, and Tab opens the inspector.
    h.get_by_label("Play").click();
    until(&mut h, "playing", |ed| ed.status["playing"] == true);
    let r = h.state().viewport.image_rect.unwrap();
    click(&mut h, r.center(), Modifiers::NONE);
    assert!(
        h.state().viewport.focused,
        "a click puts the keys in the game"
    );
    press(&mut h, Key::Tab);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !inspector(&mut agent) && Instant::now() < deadline {
        h.step();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        inspector(&mut agent),
        "Tab in the viewport opens the inspector"
    );
    // The frame shows it.
    let tick = h.state().viewport.info.unwrap().tick;
    until(&mut h, "a newer frame", |ed| {
        ed.viewport.info.is_some_and(|i| i.tick > tick)
    });
    press(&mut h, Key::Tab);

    // F8: a mark, its note typed in the viewport, Enter.
    press(&mut h, Key::F8);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut id = None;
    while id.is_none() && Instant::now() < deadline {
        h.step();
        let m = call(&mut agent, "mark.list", json!({}));
        id = m["rows"][0]["id"].as_u64();
    }
    let id = id.expect("F8 dropped a mark");
    h.input_mut().events.push(Event::Text("feels slow".into()));
    h.step();
    press(&mut h, Key::Enter);
    let deadline = Instant::now() + Duration::from_secs(5);
    while call(&mut agent, "mark.get", json!({ "id": id }))["note"] != "feels slow" {
        assert!(Instant::now() < deadline, "the note arrived");
        h.step();
        std::thread::sleep(Duration::from_millis(20));
    }
    let note = format!("argus mark note id={id} note='feels slow'");
    until(&mut h, "the mark's rows", |ed| {
        ed.feed
            .iter()
            .any(|r| r["author"] == "human" && r["command"] == note)
            && ed
                .feed
                .iter()
                .any(|r| r["author"] == "human" && r["command"] == "argus mark drop author=human")
    });

    // Paused, a click selects what is under it: a colonist.
    click(&mut h, Pos2::new(5.0, 5.0), Modifiers::NONE);
    assert!(
        !h.state().viewport.focused,
        "a click outside gives the keys back"
    );
    // Enter put play back on; the bar shows it at its next look.
    until(&mut h, "playing again", |ed| {
        ed.status["playing"] == true && ed.status["paused"] == false
    });
    h.step();
    h.get_by_label("Pause").click();
    until(&mut h, "paused", |ed| ed.status["paused"] == true);
    let info = h.state().viewport.info.unwrap();
    let sf = call(
        &mut agent,
        "obs.frame",
        json!({ "format": "semantic", "width": info.width, "height": info.height, "limit": 500 }),
    );
    let colonist = sf["entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| {
            e[1].as_str().is_some_and(|n| !n.is_empty())
                && e[0].as_str().is_some_and(|i| i.starts_with("e:"))
        })
        .unwrap_or_else(|| panic!("a named colonist on screen: {sf}"))
        .clone();
    let b: Vec<f32> = colonist[3]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap() as f32)
        .collect();
    let (px, py) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
    let r = h.state().viewport.image_rect.unwrap();
    let at = Pos2::new(
        r.min.x + px / info.width as f32 * r.width(),
        r.min.y + py / info.height as f32 * r.height(),
    );
    click(&mut h, at, Modifiers::NONE);
    until(&mut h, "the selection", |ed| ed.viewport.selected.is_some());
    let s = h.state().viewport.selected.clone().unwrap();
    assert_eq!(s.id, colonist[0].as_str().unwrap(), "{s:?} {colonist}");
    h.step();
    h.get_by_label(&format!("Selected {}", s.id));

    // The keys went in as actions; the commands the person gave are the
    // bar's, and the keys' calls are in the feed.
    let log = call(&mut agent, "input.log.get", json!({})).to_string();
    assert!(log.contains("inspect"), "{log}");
    assert_eq!(
        h.state().commands,
        vec!["argus sim play", "argus sim pause"],
        "keys are not commands; the bar's clicks are"
    );
}
