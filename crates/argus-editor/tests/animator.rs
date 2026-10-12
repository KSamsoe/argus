//! Phase 4 item 8: the animator editor, driven headless over a real socket
//! on a copy of the fixture with a hero whose animator runs while the game
//! plays. The tab shows the hero's state and frame live and the events it
//! fires; the person drags the run's rate, adds an event, a transition,
//! and a state, and each is one change in the sidecar with its comment
//! kept, played at once, undone by Ctrl+Z.

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

const MACHINE: &str = r#"# The hero: idle until hurt enough to run.
type = "animator"
initial = "idle"

[states.idle]
clip = "idle"

[[states.idle.transitions]]
to = "run"
when = "Health.cur > 5"

[states.run]
clip = "run"
fps = 8.0
events = [{ frame = 1, name = "footstep" }]

[[states.run.transitions]]
to = "idle"
when = "Health.cur <= 5"
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
        "timed out waiting for {what}; problem {:?}; live {}; sidecar {}",
        h.state().problem,
        h.state().animator.live,
        h.state().animator.sidecar["doc"]
    );
}

fn result(c: &mut Client, method: &str, params: Json) -> Json {
    let r = c.call(method, params).unwrap();
    assert!(r.get("error").is_none(), "{method}: {r}");
    r["result"].clone()
}

fn doc(ed: &Editor) -> &Json {
    &ed.animator.sidecar["doc"]
}

#[test]
fn a_person_shapes_the_heros_run_while_it_plays() {
    let t = common::temp("editor-animator");
    common::project(t.path());
    // An 8-frame sheet: idle is frame 0, the run frames 1 to 4.
    std::fs::create_dir_all(t.path().join("assets/anim")).unwrap();
    {
        let f = std::io::BufWriter::new(
            std::fs::File::create(t.path().join("assets/hero.png")).unwrap(),
        );
        let mut enc = png::Encoder::new(f, 32, 8);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()
            .unwrap()
            .write_image_data(&[230, 110, 60, 255].repeat(256))
            .unwrap();
    }
    common::write(
        t.path(),
        "assets/hero.toml",
        "grid = [8, 1]\n\n[clips.idle]\nframes = [0]\n\n[clips.run]\nfrom = 1\nto = 4\nfps = 12\nloop = true\n",
    );
    common::write(t.path(), "assets/anim/hero.toml", MACHINE);
    common::write(
        t.path(),
        "scenes/arena/hero.toml",
        "[Transform]\npos = [2.0, 0.0]\n\n[Sprite]\natlas = \"hero\"\n\n[Animator]\nmachine = \"anim/hero\"\n\n[Health]\ncur = 3\nmax = 10\n",
    );
    let name = format!("{}-animator", socket_name_for(t.path()));
    let (r, n) = (t.path().to_path_buf(), name.clone());
    let thread = std::thread::spawn(move || {
        let d = common::open(&r);
        let _ = serve(d, Transport::Socket(n), Some(&r));
    });
    let _daemon = common::Served::new(&name, thread);
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
    h.state_mut().select(Some("@hero".into()));
    h.get_by_label("Animator").click();
    until(&mut h, "the machine and the hero live", |ed| {
        ed.tab == Tab::Animator
            && ed.animator.machine.as_deref() == Some("anim/hero")
            && doc(ed)["states"]["run"]["clip"] == "run"
            && ed.animator.live["state"] == "idle"
            && ed.animator.clips["clips"]["run"].is_object()
    });
    h.step();
    h.get_by_label_contains("@hero: idle, frame 0 of 1");
    h.get_by_label("assets/anim/hero.toml, its file");

    // Play; hurt the hero's health up past 5 (as the agent): it runs, and
    // its footsteps fire.
    h.get_by_label("Play").click();
    let tx = result(
        &mut agent,
        "edit.begin",
        json!({ "author": "agent:claude", "note": "run" }),
    )["tx"]
        .clone();
    result(
        &mut agent,
        "edit.set",
        json!({ "tx": tx, "entity": "@hero", "path": "Health.cur", "value": 9 }),
    );
    result(&mut agent, "edit.commit", json!({ "tx": tx }));
    until(&mut h, "running", |ed| ed.animator.live["state"] == "run");
    until(&mut h, "a footstep", |ed| {
        ed.animator.fired.iter().any(|(_, n, _)| n == "footstep")
    });
    assert!(h
        .state()
        .animator
        .fired
        .iter()
        .any(|(_, n, f)| n == "anim.entered" && f["state"] == "run"));
    h.step();
    h.get_by_label_contains("@hero: run, frame");

    // The run's state opened (and the idle one, open at the start,
    // closed), its rate dragged up from 8 as a person does.
    h.get_by_label("idle  (clip idle, 1 frame)").click();
    h.get_by_label("run  (clip run, 4 frames)").click();
    h.step();
    h.step();
    let rate = h
        .get_all_by_label("rate")
        .find(|n| n.role() == egui::accesskit::Role::SpinButton)
        .expect("the run's rate")
        .bounding_box()
        .unwrap();
    let (x, y) = (
        ((rate.x0 + rate.x1) / 2.0) as f32,
        ((rate.y0 + rate.y1) / 2.0) as f32,
    );
    let press = |h: &mut Harness<'_, Editor>, x: f32, pressed: bool| {
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
    press(&mut h, x, true);
    for i in 1..=8 {
        h.input_mut()
            .events
            .push(Event::PointerMoved(Pos2::new(x + i as f32 * 10.0, y)));
        h.step();
    }
    press(&mut h, x + 80.0, false);
    until(&mut h, "the new rate in the file", |ed| {
        doc(ed)["states"]["run"]["fps"]
            .as_f64()
            .is_some_and(|f| f > 8.0)
    });
    let fps = h.state().animator.sidecar["doc"]["states"]["run"]["fps"]
        .as_f64()
        .unwrap();
    let text = common::read(t.path(), "assets/anim/hero.toml");
    assert!(
        text.starts_with("# The hero: idle until hurt enough to run."),
        "the comment kept: {text}"
    );
    assert!(text.contains(&format!("fps = {fps}")), "{text}");
    let feed = result(&mut agent, "edit.feed", json!({ "limit": 100 }));
    let last = feed["rows"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["author"], "human");
    assert!(
        last["command"]
            .as_str()
            .unwrap()
            .starts_with("argus edit author=human note='anim/hero: states.run.fps 8"),
        "{}",
        last["command"]
    );
    let runs = h
        .state()
        .commands
        .iter()
        .filter(|l| l.contains("states.run.fps"))
        .count();
    assert_eq!(runs, 1, "one change for the drag");

    // An event, a transition, the loop, and a state: each one change.
    h.get_by_label("+ event").click();
    until(&mut h, "the event", |ed| {
        doc(ed)["states"]["run"]["events"]
            .as_array()
            .is_some_and(|e| e.len() == 2)
    });
    assert_eq!(
        h.state().animator.sidecar["doc"]["states"]["run"]["events"][1]["name"],
        "run.event"
    );
    h.step();
    let before = h.state().animator.sidecar["doc"]["states"]["run"]["transitions"]
        .as_array()
        .unwrap()
        .len();
    h.get_all_by_label("+ transition").next().unwrap().click();
    until(&mut h, "the transition", |ed| {
        doc(ed)["states"]["run"]["transitions"]
            .as_array()
            .is_some_and(|t| t.len() == before + 1)
    });
    h.step();
    h.get_by_label("loops").click();
    until(&mut h, "no loop", |ed| {
        doc(ed)["states"]["run"]["loop"] == false
    });
    h.state_mut().animator.new_state = ("dash".into(), "run".into());
    h.step();
    h.get_by_label("Add").click();
    until(&mut h, "the new state", |ed| {
        doc(ed)["states"]["dash"]["clip"] == "run"
    });
    assert!(common::read(t.path(), "assets/anim/hero.toml").contains("[states.dash]"));
    let img = h.render().expect("drawn");
    let (w, hh) = img.dimensions();
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("editor-animator.png");
    let f = std::io::BufWriter::new(std::fs::File::create(&out).unwrap());
    let mut enc = png::Encoder::new(f, w, hh);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&img.into_raw())
        .unwrap();

    // Ctrl+Z, five times: the file as it was, byte for byte.
    for n in 0..5 {
        h.press_key_modifiers(Modifiers::COMMAND, egui::Key::Z);
        let deadline = Instant::now() + Duration::from_secs(10);
        while h
            .state()
            .commands
            .iter()
            .filter(|l| l.starts_with("argus edit undo"))
            .count()
            <= n
            && Instant::now() < deadline
        {
            h.step();
        }
        for _ in 0..10 {
            h.step();
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while common::read(t.path(), "assets/anim/hero.toml") != MACHINE && Instant::now() < deadline {
        h.step();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(common::read(t.path(), "assets/anim/hero.toml"), MACHINE);
}
