//! The phase 4 gate's second half (design §18): "A human tunes Outpost's
//! food economy at 10× with sliders; the agent sees the slider path as
//! data, sweeps the candidates, and reports which ones starve."
//!
//! The person is driven headless as in tests/tuning.rs: Outpost plays at
//! 10x while the colony's hunger is dragged three times, a pause between,
//! and kept. The agent is the agent's own tools (`argus_cli::tools`, what
//! `argus mcp` serves): it reads the path with `query {tuning}`, takes the
//! value the session began at and each value the person let go at, sweeps
//! them with `test {sweep}` over §9's scenario (twenty seeds of thirty
//! days each), and says which starve. A candidate is minutes of release
//! build, so this runs by hand at the gate:
//!
//!   cargo test -p argus-editor --release --test gate_outpost -- --ignored --nocapture

#[path = "../../argus-daemon/tests/common/mod.rs"]
mod common;

use argus_cli::tools;
use argus_client::Client;
use argus_daemon::{serve, socket_name_for, Transport};
use argus_editor::Editor;
use eframe::egui::{self, accesskit::Role, Event, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use serde_json::{json, Value as Json};
use std::path::Path;
use std::time::{Duration, Instant};

/// Where the person lets go, as a share of the slider (hunger 0 to 10).
const LET_GO: [f32; 3] = [0.13, 0.20, 0.16];
/// Frames between two rows of the path that make them two moves: a hand
/// let go between them (at 10x, half a second).
const LET_GO_GAP: u64 = 300;

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
    let deadline = Instant::now() + Duration::from_secs(30);
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

/// Draw frames for a while, the game running, as a person watches.
fn watch(h: &mut Harness<'_, Editor>, ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        h.step();
        std::thread::sleep(Duration::from_millis(16));
    }
}

/// Drag the hunger slider from where its knob is to a share of its track.
fn drag_to(h: &mut Harness<'_, Editor>, to: f32) {
    let b = h
        .get_all_by_label("hunger_per_day")
        .find(|n| n.role() == Role::Slider)
        .expect("the hunger slider")
        .bounding_box()
        .expect("placed");
    let now = h
        .state()
        .tuning
        .rows
        .iter()
        .find(|r| r["path"] == "Colony.hunger_per_day")
        .and_then(|r| r["value"].as_f64())
        .unwrap_or(1.0) as f32;
    // egui's track: the slider's rect, less the knob's radius at each end.
    let y = ((b.y0 + b.y1) / 2.0) as f32;
    let r = (b.y1 - b.y0) as f32 / 2.5;
    let x = |f: f32| b.x0 as f32 + r + ((b.x1 - b.x0) as f32 - 2.0 * r) * f;
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
    let from = (now / 10.0).clamp(0.0, 1.0);
    press(h, Pos2::new(x(from), y), true);
    for i in 1..=8 {
        let f = from + (to - from) * i as f32 / 8.0;
        h.input_mut()
            .events
            .push(Event::PointerMoved(Pos2::new(x(f), y)));
        h.step();
        std::thread::sleep(Duration::from_millis(15));
    }
    press(h, Pos2::new(x(to), y), false);
    let want = (to * 10.0) as f64;
    until(h, "the slider where it was let go", |ed| {
        ed.tuning
            .rows
            .iter()
            .find(|r| r["path"] == "Colony.hunger_per_day")
            .and_then(|r| r["value"].as_f64())
            .is_some_and(|v| (v - want).abs() < 0.15)
    });
}

fn call(c: &mut Client, tool: &str, args: Json) -> Json {
    match tools::run(c, tool, &args) {
        Ok(v) => v,
        Err(e) => panic!("{tool} {args}: {e}"),
    }
}

#[test]
#[ignore]
fn a_person_tunes_outpost_at_10x_and_the_agent_sweeps_the_path() {
    let t = common::temp("gate-outpost");
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/outpost"),
        t.path(),
    );
    let name = format!("{}-gate", socket_name_for(t.path()));
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
        .with_size(egui::vec2(1500.0, 950.0))
        .wgpu()
        .build_state(|ctx, ed: &mut Editor| ed.ui(ctx), {
            let mut ed = Editor::new(&name, "human");
            ed.audio = false;
            ed
        });

    // The person: the colony at 10x, its hunger moved three times while it
    // runs, then kept.
    until(&mut h, "the bar", |ed| ed.status["project"] == "outpost");
    h.get_by_label("Play").click();
    h.get_by_label("10x").click();
    until(&mut h, "playing at 10x", |ed| {
        ed.status["playing"] == true && ed.status["speed"] == 10.0
    });
    h.get_by_label("Tuning").click();
    until(&mut h, "the colony's hunger", |ed| {
        ed.tuning
            .rows
            .iter()
            .any(|r| r["path"] == "Colony.hunger_per_day")
    });
    h.state_mut().tuning.filter = "hunger".into();
    watch(&mut h, 300);
    for to in LET_GO {
        drag_to(&mut h, to);
        watch(&mut h, 1000);
    }
    h.state_mut().tuning.note = "hungrier colonists".into();
    h.step();
    h.get_by_label("Keep").click();
    until(&mut h, "kept", |ed| ed.tuning.session.is_null());
    h.get_by_label("Pause").click();
    until(&mut h, "paused", |ed| ed.status["paused"] == true);
    let kept = std::fs::read_to_string(t.path().join("scenes/valley/colony.toml")).unwrap();

    // The agent: the path as data.
    call(&mut agent, "summary", json!({}));
    let log = call(
        &mut agent,
        "query",
        json!({ "tuning": "Colony.hunger_per_day" }),
    );
    let story = log["story"].as_array().cloned().unwrap_or_default();
    println!("the path, as the agent reads it:");
    for s in &story {
        println!("  {}", s.as_str().unwrap_or(""));
    }
    let moves: Vec<&Json> = log["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["kind"] == "set" && r["path"] == "Colony.hunger_per_day")
        .collect();
    assert!(moves.len() >= 3, "{log}");
    // The candidates: where the session began, and each value the person
    // let go at (a row with a pause after it, and the last).
    let round = |v: f64| (v * 100.0).round() / 100.0;
    let mut candidates = vec![round(moves[0]["from"].as_f64().unwrap())];
    for (i, r) in moves.iter().enumerate() {
        let frame = r["frame"].as_u64().unwrap_or(0);
        let next = moves.get(i + 1).and_then(|n| n["frame"].as_u64());
        if next.is_none_or(|n| n.saturating_sub(frame) > LET_GO_GAP) {
            candidates.push(round(r["to"].as_f64().unwrap()));
        }
    }
    candidates.sort_by(|a, b| a.partial_cmp(b).unwrap());
    candidates.dedup();
    println!("candidates: {candidates:?} (where it began, and where the person let go)");
    assert!(candidates.len() >= 3, "{candidates:?} from {log}");
    let last = round(moves.last().unwrap()["to"].as_f64().unwrap());
    assert!(
        kept.contains(&format!("hunger_per_day = {last}"))
            || kept.contains(&format!("hunger_per_day = {}", moves.last().unwrap()["to"])),
        "the kept value is the path's last: {kept}"
    );

    // The sweep: §9's scenario, twenty seeds of thirty days, a candidate at
    // a time, tried without writing it.
    let started = Instant::now();
    let sweep = call(
        &mut agent,
        "test",
        json!({
            "name": "does not starve",
            "seeds": "1..20",
            "sweep": { "entity": "@colony", "path": "Colony.hunger_per_day", "values": candidates },
        }),
    );
    let mins = started.elapsed().as_secs_f64() / 60.0;
    // The report: which ones starve.
    println!("\nhunger a day | runs | starve | short of food only | seeds that starve");
    let mut starving = Vec::new();
    for c in sweep["candidates"].as_array().unwrap() {
        let failing = c["failing"].as_array().cloned().unwrap_or_default();
        let starved: Vec<u64> = failing
            .iter()
            .filter(|f| f["failed"].to_string().contains("starvation"))
            .filter_map(|f| f["seed"].as_u64())
            .collect();
        let short = failing.len() - starved.len();
        println!(
            "{} | {} | {} | {short} | {starved:?}",
            c["value"],
            c["runs"],
            starved.len()
        );
        assert_eq!(c["runs"], 20, "{c}");
        if !starved.is_empty() {
            starving.push(c["value"].as_f64().unwrap());
        }
    }
    println!("the sweep took {mins:.1} minutes");
    println!(
        "report: {} of {} candidates starve: {starving:?}",
        starving.len(),
        candidates.len()
    );
    let first = &sweep["candidates"][0];
    assert_eq!(
        first["failed"], 0,
        "the value the colony began at feeds it, as §9's scenario says: {first}"
    );
    // The sweep wrote nothing: the kept value is the file's.
    assert_eq!(
        std::fs::read_to_string(t.path().join("scenes/valley/colony.toml")).unwrap(),
        kept
    );
}
