//! Phase 4 item 0: the editor, driven headless as a person would (clicking
//! by label, egui_kittest), against a real daemon on a real socket.
//! Everything it does is in the change feed as the command that makes it,
//! "show as command" prints the same lines, the agent's changes show up in
//! it as they happen, and a failed call says what, why, and what to try.

#[path = "../../argus-daemon/tests/common/mod.rs"]
mod common;

use argus_client::Client;
use argus_daemon::{serve, socket_name_for, Transport};
use argus_editor::Editor;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use serde_json::{json, Value as Json};
use std::time::{Duration, Instant};

fn daemon(root: &std::path::Path, tag: &str) -> String {
    common::project(root);
    let name = format!("{}-{tag}", socket_name_for(root));
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

/// Draw frames until `ok` holds, as a window would while a person waits.
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
        "timed out waiting for {what}; feed: {:#?}; problem: {:?}; status: {}",
        h.state()
            .feed
            .iter()
            .map(|r| r["command"].clone())
            .collect::<Vec<_>>(),
        h.state().problem,
        h.state().status
    );
}

fn has_row(ed: &Editor, author: &str, command: &str) -> bool {
    ed.feed
        .iter()
        .any(|r| r["author"] == author && r["command"] == command)
}

fn labels(h: &Harness<'_, Editor>, text: &str) -> usize {
    h.query_all_by_label(text).count()
}

#[test]
fn the_editor_does_everything_as_commands_the_agent_could_have_issued() {
    let t = common::temp("editor");
    let socket = daemon(t.path(), "editor");
    // Something already happened before the editor opened.
    let mut agent = Client::connect(&socket).unwrap();
    agent
        .call("session.identify", json!({ "author": "agent:claude" }))
        .unwrap();
    agent.call("sim.step", json!({ "n": 2 })).unwrap();

    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1280.0, 800.0))
        .wgpu()
        .build_state(|ctx, ed: &mut Editor| ed.ui(ctx), {
            let mut ed = Editor::new(&socket, "human");
            ed.audio = false;
            ed
        });
    until(&mut h, "the bar", |ed| ed.status["project"] == "fixture");
    until(&mut h, "the rows from before", |ed| {
        has_row(ed, "agent:claude", "argus sim step n=2")
    });
    h.get_by_label("fixture / arena");
    // The feed is a tab beside the inspector (item 2).
    h.get_by_label("Change feed").click();
    h.step();

    // A click is a command in the feed, by the person.
    h.get_by_label("Step").click();
    until(&mut h, "the step", |ed| {
        has_row(ed, "human", "argus sim step n=1")
    });
    until(&mut h, "the tick", |ed| ed.status["tick"] == 3);
    h.get_by_label("10x").click();
    until(&mut h, "the speed", |ed| {
        has_row(ed, "human", "argus sim speed x=10")
    });
    until(&mut h, "10x", |ed| ed.status["speed"] == 10.0);
    h.get_by_label("Play").click();
    until(&mut h, "playing", |ed| ed.status["playing"] == true);
    h.get_by_label("Pause").click();
    until(&mut h, "paused", |ed| ed.status["paused"] == true);
    assert!(has_row(h.state(), "human", "argus sim play"));
    assert!(has_row(h.state(), "human", "argus sim pause"));

    // Show as command: the same lines as the feed's, ready to paste.
    let feed_only = labels(&h, "argus sim step n=1");
    assert_eq!(feed_only, 1, "in the feed");
    h.get_by_label("Show as command").click();
    h.step();
    h.step();
    assert_eq!(
        labels(&h, "argus sim step n=1"),
        2,
        "in the feed and the commands"
    );
    assert_eq!(
        h.state().commands,
        vec![
            "argus sim step n=1",
            "argus sim speed x=10",
            "argus sim play",
            "argus sim pause"
        ]
    );
    for l in &h.state().commands {
        assert!(
            has_row(h.state(), "human", l),
            "{l} is a feed row by the person"
        );
    }

    // The agent's change shows up as it happens, in the agent's name.
    let tx = agent
        .call(
            "edit.begin",
            json!({ "author": "agent:claude", "note": "tougher" }),
        )
        .unwrap()["result"]["tx"]
        .clone();
    agent
        .call(
            "edit.set",
            json!({ "tx": tx, "entity": "@player", "path": "Health.max", "value": 9 }),
        )
        .unwrap();
    agent.call("edit.commit", json!({ "tx": tx })).unwrap();
    let line = "argus edit author=agent:claude note=tougher ops='[{entity: @player, op: set, path: Health.max, value: 9}]'";
    until(&mut h, "the agent's commit", |ed| {
        has_row(ed, "agent:claude", line)
    });
    h.step();
    h.get_by_label("tougher");

    // The window as a person sees it, drawn offscreen by the same egui and
    // wgpu the window uses.
    h.step();
    let img = h.render().expect("the editor draws");
    let (w, hgt) = img.dimensions();
    let px = img.into_raw();
    let colours: std::collections::HashSet<[u8; 4]> =
        px.chunks(4).map(|c| [c[0], c[1], c[2], c[3]]).collect();
    assert!(
        colours.len() > 50,
        "a drawn window, not a blank one: {} colours",
        colours.len()
    );
    let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("editor-item0.png");
    let f = std::io::BufWriter::new(std::fs::File::create(&out).unwrap());
    let mut enc = png::Encoder::new(f, w, hgt);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&px).unwrap();
    println!("the editor, drawn: {}", out.display());

    // A call that fails says what, why, and what to try.
    h.state_mut().act("sim.speed", json!({ "x": "fast" }));
    until(&mut h, "the problem", |ed| ed.problem.is_some());
    let p = h.state().problem.clone().unwrap();
    assert_eq!(p.what, "argus sim speed x=fast");
    assert!(!p.message.is_empty(), "{p:?}");
    h.step();
    h.get_by_label(&p.message);
    h.get_by_label("Dismiss").click();
    h.step();
    assert!(h.state().problem.is_none());

    // What the person did is only what the feed says they did: no row was
    // written for the lookups that drew the panels.
    let mine: Vec<Json> = h
        .state()
        .feed
        .iter()
        .filter(|r| r["author"] == "human")
        .map(|r| r["command"].clone())
        .collect();
    assert_eq!(
        mine,
        vec![
            json!("argus sim step n=1"),
            json!("argus sim speed x=10"),
            json!("argus sim play"),
            json!("argus sim pause")
        ]
    );
}

#[test]
fn a_daemon_that_goes_away_says_so_and_can_be_reconnected() {
    let mut h = Harness::builder().build_state(
        |ctx, ed: &mut Editor| ed.ui(ctx),
        Editor::new("argusd-no-such-socket.sock", "human"),
    );
    h.step();
    h.step();
    assert!(
        matches!(h.state().link_state(), argus_editor::link::State::Down(_)),
        "{:?}",
        h.state().link_state()
    );
    h.get_by_label("Reconnect");
    h.get_by_label_contains("disconnected");
}

/// Found by Kevin's hand check: the editor left minimized grew to 11 GB,
/// every frame the game sent written to the GPU and held there until the
/// window painted again. Minimized, it takes no frames; shown again, the
/// newest.
#[test]
fn a_minimized_editor_holds_no_frames() {
    let t = common::temp("editor-minimized");
    let name = daemon(t.path(), "minimized");
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .wgpu()
        .build_state(|ctx, ed: &mut Editor| ed.ui(ctx), {
            let mut ed = Editor::new(&name, "human");
            ed.audio = false;
            ed
        });
    until(&mut h, "frames", |ed| ed.viewport.frames_shown > 0);
    h.get_by_label("Play").click();
    until(&mut h, "playing", |ed| ed.status["playing"] == true);
    let minimized = |h: &mut Harness<'_, Editor>, on: bool| {
        h.input_mut()
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .minimized = Some(on);
    };
    minimized(&mut h, true);
    h.step();
    let before = h.state().viewport.frames_shown;
    let end = Instant::now() + Duration::from_millis(600);
    while Instant::now() < end {
        minimized(&mut h, true);
        h.step();
        std::thread::sleep(Duration::from_millis(16));
    }
    assert_eq!(
        h.state().viewport.frames_shown,
        before,
        "no frames while minimized"
    );
    minimized(&mut h, false);
    until(&mut h, "the newest frame once shown", |ed| {
        ed.viewport.frames_shown > before
    });
}
