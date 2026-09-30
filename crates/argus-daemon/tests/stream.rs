//! Phase 4 item 1, the daemon's side of the editor's viewport: frames for
//! a connection in a file both processes map (`render.stream`, §5's
//! out-of-band channel), and the viewport's keys and mouse down the
//! window's own path (`input.device`). What F8, Enter, Escape, F9, and F10
//! do there is in the change feed as the calls that do it, by the person;
//! the keys themselves are in the input log, as actions.

mod common;

use argus_client::frames::FrameReader;
use argus_daemon::Daemon;
use common::*;
use serde_json::{json, Value as Json};
use std::path::Path;
use std::time::{Duration, Instant};

const ME: u64 = 2;

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

fn me(d: &mut Daemon, method: &str, params: Json) -> Json {
    d.call(ME, method, &params)
        .unwrap_or_else(|e| panic!("{method}: {} ({})", e.message, e.hint))
}

/// Real-time frames for `ms`, the stream drawing as the server's loop has
/// it; the frames it announced.
fn run(d: &mut Daemon, ms: u64) -> Vec<Json> {
    let t0 = Instant::now();
    let mut out = Vec::new();
    while t0.elapsed() < Duration::from_millis(ms) {
        if d.wants_frames() {
            d.frame().unwrap();
        }
        d.stream_frame();
        for (c, n) in d.outbox.drain(..) {
            if c == ME && n["method"] == "render.frame" {
                out.push(n["params"].clone());
            }
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    out
}

fn key(d: &mut Daemon, name: &str) {
    me(
        d,
        "input.device",
        json!({ "events": [{ "key": name, "pressed": true }] }),
    );
    run(d, 60);
    me(
        d,
        "input.device",
        json!({ "events": [{ "key": name, "pressed": false }] }),
    );
    run(d, 60);
}

fn inspector(d: &mut Daemon) -> (bool, String) {
    let q = ok(
        d,
        "world.query",
        json!({ "select": ["UiRect.shown", "Inspector.page"], "where": { "has": ["Inspector"] } }),
    );
    (
        q["rows"][0][0].as_bool().unwrap_or(false),
        q["rows"][0][1].as_str().unwrap_or("").to_string(),
    )
}

fn human_rows(d: &mut Daemon) -> Vec<String> {
    let f = ok(d, "edit.feed", json!({ "limit": 500 }));
    f["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["author"] == "human")
        .map(|r| r["command"].as_str().unwrap_or("").to_string())
        .collect()
}

#[test]
fn a_viewport_gets_frames_and_plays_the_game_as_a_window_does() {
    let t = temp("stream");
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/outpost"),
        t.path(),
    );
    let mut d = open(t.path());
    me(&mut d, "session.identify", json!({ "author": "human" }));
    // The canvas is 960 x 540; frames of half that, so a pixel is two.
    let s = me(
        &mut d,
        "render.stream",
        json!({ "width": 480, "height": 270, "audio": false }),
    );
    let path = s["stream"]["path"].as_str().unwrap().to_string();
    assert_eq!(s["stream"]["slots"], 3);
    let frames = run(&mut d, 100);
    assert!(!frames.is_empty(), "the first frame at once");
    let reader = FrameReader::open(Path::new(&path)).unwrap();
    assert_eq!(reader.size(), (480, 270));
    let last = frames.last().unwrap();
    let mut px = Vec::new();
    let info = reader
        .read(
            last["slot"].as_u64().unwrap() as u32,
            last["seq"].as_u64().unwrap(),
            &mut px,
        )
        .expect("the frame named is whole");
    assert_eq!((info.width, info.height), (480, 270));
    let shot = Path::new(env!("CARGO_TARGET_TMPDIR")).join("stream-first.png");
    argus_render::Image {
        width: 480,
        height: 270,
        rgba: px.clone(),
    }
    .save_png(&shot)
    .unwrap();
    let colours: std::collections::HashSet<&[u8]> = px.chunks(4).collect();
    // Outpost is flat placeholder shapes: grass, trees, fields, the
    // stockpile, the colonists; a blank frame is one colour.
    assert!(
        colours.len() > 4,
        "the valley drawn: {} colours ({})",
        colours.len(),
        shot.display()
    );

    // Paused, it draws only what changed, and a few times a second.
    let idle = run(&mut d, 600);
    assert!(idle.len() <= 4, "paused and idle: {} frames", idle.len());
    // Playing, every frame.
    me(&mut d, "sim.play", json!({}));
    let playing = run(&mut d, 600);
    assert!(
        playing.len() >= 15,
        "playing: {} frames in 0.6 s",
        playing.len()
    );
    let seqs: Vec<u64> = playing.iter().map(|f| f["seq"].as_u64().unwrap()).collect();
    assert!(seqs.windows(2).all(|w| w[1] > w[0]), "in order: {seqs:?}");

    // Keys as the window sends them: Tab opens the inspector.
    key(&mut d, "Tab");
    assert_eq!(
        inspector(&mut d),
        (true, "colony".to_string()),
        "Tab opens it"
    );
    // The mouse, in the stream's pixels: over the Jobs tab, and a click.
    let b = ok(
        &mut d,
        "world.query",
        json!({ "select": ["UiRect.pos", "UiRect.size", "UiButton.event"], "where": { "has": ["UiButton"] } }),
    );
    let jobs = b["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r[2] == "inspect.tab.jobs")
        .expect("a Jobs tab")
        .clone();
    let cx = (jobs[0][0].as_f64().unwrap() + jobs[1][0].as_f64().unwrap() / 2.0) / 2.0;
    let cy = (jobs[0][1].as_f64().unwrap() + jobs[1][1].as_f64().unwrap() / 2.0) / 2.0;
    me(
        &mut d,
        "input.device",
        json!({ "events": [{ "pointer": [cx, cy] }, { "mouse": "left", "pressed": true }] }),
    );
    run(&mut d, 60);
    me(
        &mut d,
        "input.device",
        json!({ "events": [{ "mouse": "left", "pressed": false }] }),
    );
    run(&mut d, 80);
    assert_eq!(inspector(&mut d).1, "jobs", "a click on the Jobs tab");
    key(&mut d, "Tab");
    assert!(!inspector(&mut d).0, "Tab closes it");
    // The input log has actions, not keys.
    let log = ok(&mut d, "input.log.get", json!({}));
    let text = log.to_string();
    assert!(text.contains("inspect"), "{text}");
    assert!(!text.contains("\"Tab\""), "no keys in the log: {text}");

    // F8 in the viewport: a mark, the prompt in the frame's bar, a note
    // typed, Enter.
    me(
        &mut d,
        "input.device",
        json!({ "events": [{ "key": "F8", "pressed": true }] }),
    );
    assert_eq!(
        ok(&mut d, "sim.status", json!({}))["paused"],
        true,
        "F8 pauses"
    );
    me(
        &mut d,
        "input.device",
        json!({ "events": [{ "key": "F8", "pressed": false }], "text": ["feels", " late"] }),
    );
    run(&mut d, 60);
    let marks = ok(&mut d, "mark.list", json!({}));
    assert_eq!(marks["open"], 1, "{marks}");
    let id = marks["rows"][0]["id"].as_u64().unwrap();
    me(
        &mut d,
        "input.device",
        json!({ "events": [{ "key": "Enter", "pressed": true }, { "key": "Enter", "pressed": false }] }),
    );
    assert_eq!(
        ok(&mut d, "sim.status", json!({}))["paused"],
        false,
        "play goes on"
    );
    let m = ok(&mut d, "mark.get", json!({ "id": id }));
    assert_eq!(m["note"], "feels late", "{m}");
    // F8 again, and Escape takes it back.
    me(
        &mut d,
        "input.device",
        json!({ "events": [{ "key": "F8", "pressed": true }, { "key": "F8", "pressed": false }] }),
    );
    assert_eq!(ok(&mut d, "mark.list", json!({}))["open"], 2);
    me(
        &mut d,
        "input.device",
        json!({ "events": [{ "key": "Escape", "pressed": true }, { "key": "Escape", "pressed": false }] }),
    );
    assert_eq!(ok(&mut d, "mark.list", json!({}))["open"], 1, "cancelled");

    // F9 and F10 on the agent's proposal.
    let tx = ok(
        &mut d,
        "edit.begin",
        json!({ "author": "agent:claude", "note": "richer fields" }),
    )["tx"]
        .clone();
    ok(
        &mut d,
        "edit.set",
        json!({ "tx": tx, "entity": "@colony", "path": "Colony.crop_yield", "value": 6 }),
    );
    let p = ok(&mut d, "edit.propose", json!({ "tx": tx }));
    let pid = p["proposal"].as_u64().unwrap();
    key(&mut d, "F9");
    assert_eq!(
        ok(&mut d, "proposal.list", json!({}))["trial"]["showing"],
        "B",
        "F9 tries it, on B"
    );
    key(&mut d, "F10");
    let got = ok(&mut d, "world.get", json!({ "entity": "@colony" }));
    assert_eq!(
        got["components"]["Colony"]["crop_yield"], 6.0,
        "F10 keeps B"
    );

    // Everything the keys did, as the calls that do it, by the person.
    let rows = human_rows(&mut d);
    let want = [
        "argus sim play".to_string(),
        "argus mark drop author=human".to_string(),
        format!("argus mark note id={id} note='feels late'"),
        "argus mark drop author=human".to_string(),
        format!("argus mark cancel id={}", id + 1),
        format!("argus proposal try id={pid}"),
        "argus proposal show variant=B".to_string(),
        format!("argus proposal pick author=human id={pid} variant=B"),
    ];
    assert_eq!(rows, want, "the feed");

    // Another client cannot take the stream; closing it removes the file.
    let e = d
        .call(3, "render.stream", &json!({ "width": 64, "height": 64 }))
        .unwrap_err();
    assert!(e.message.contains("another client"), "{}", e.message);
    me(&mut d, "render.stream", json!({ "open": false }));
    drop(reader);
    assert!(!Path::new(&path).exists(), "the file goes with the stream");
    assert!(!d.has_stream());
}

/// Kevin's play in the editor: "the music plays before play has been hit".
/// The viewport opens the sound with its stream, and a device pulls the
/// mixer in real time whatever the clock does. Now the sound is held
/// while the game is stopped or paused, and the music goes on from where
/// it was. (With the master bus muted, so the test is silent; skipped on a
/// machine with no audio output.)
#[test]
fn the_music_waits_for_play() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/hollow")
        .canonicalize()
        .unwrap();
    let mut d = open(&root);
    let s = me(
        &mut d,
        "render.stream",
        json!({ "width": 320, "height": 180, "audio": true }),
    );
    if s["audio"].is_null() || s["audio"].get("error").is_some() {
        println!("no audio output here ({}); skipped", s["audio"]);
        return;
    }
    me(
        &mut d,
        "audio.bus",
        json!({ "bus": "master", "mute": true }),
    );
    let frames = |d: &mut Daemon, ms: u64| {
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_millis(ms) {
            if d.wants_frames() {
                d.frame().unwrap();
            }
            d.stream_frame();
            d.sync_audio();
            d.outbox.clear();
            std::thread::sleep(Duration::from_millis(16));
        }
    };
    let music = |d: &mut Daemon| {
        let a = ok(d, "audio.status", json!({}));
        (
            a["held"].as_bool().unwrap(),
            a["music"]["seconds"].as_f64().unwrap_or(0.0),
        )
    };
    frames(&mut d, 300);
    let (held, before) = music(&mut d);
    assert!(held, "not playing: held");
    frames(&mut d, 500);
    assert_eq!(music(&mut d), (true, before), "before Play the music waits");
    me(&mut d, "sim.play", json!({}));
    frames(&mut d, 700);
    let (held, playing) = music(&mut d);
    assert!(!held, "playing: not held");
    assert!(
        playing > before + 0.3,
        "the music goes on: {before} to {playing}"
    );
    me(&mut d, "sim.pause", json!({}));
    frames(&mut d, 100);
    let (held, paused) = music(&mut d);
    assert!(held, "paused: held");
    frames(&mut d, 500);
    assert_eq!(music(&mut d).1, paused, "paused, it waits where it was");
}
