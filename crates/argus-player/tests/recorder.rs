//! §15 the bounded recorder (phase 2 item 7): however long Hollow runs, the
//! recorder holds two keyframes and the input since the older one; a report
//! reaches back thirty to sixty seconds and replays in a fresh player to the
//! world it saved; a crash report replays to its tick without a saved world.

use argus_player::Player;
use serde_json::json;
use std::path::Path;

fn hollow() -> Player {
    Player::open(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hollow"),
        None,
        None,
    )
    .unwrap()
}

/// Two minutes of play: running back and forth, jumping every second.
fn play(p: &mut Player, seconds: u64) {
    for s in 0..seconds {
        let dir = if (s / 5) % 2 == 0 { 1 } else { -1 };
        p.inject("move", Some(&json!([dir, 0])), Some(60), false)
            .unwrap();
        p.inject("jump", Some(&json!(true)), Some(10), false)
            .unwrap();
        p.step(60).unwrap();
    }
}

fn journal_len(p: &Player) -> usize {
    p.sources.input.as_ref().unwrap().borrow().journal().len()
}

#[test]
fn the_recorder_stays_bounded_and_a_report_replays_to_the_same_world() {
    let mut p = hollow();
    p.record();
    play(&mut p, 20);
    let early = journal_len(&p);
    assert_eq!(
        p.recorder.as_ref().unwrap().from(),
        0,
        "early on, all of it"
    );
    play(&mut p, 100);
    let r = p.recorder.as_ref().unwrap();
    assert_eq!(r.len(), 2);
    let now = p.tick();
    assert_eq!(now, 120 * 60);
    let back = now - r.from();
    assert!(
        (1800..=3600).contains(&back),
        "reaches back 30 to 60 s: {back} ticks"
    );
    // Two commands a second: the journal holds the window, not the run.
    let held = journal_len(&p);
    assert!(
        held <= 2 * 60 + 4 && held >= early.min(2 * 30),
        "{held} commands held"
    );
    let log = p.sources.input.as_ref().unwrap().borrow().log().len();
    assert!(log < 4 * 70, "the value log is bounded too: {log}");

    // Two keyframes and the world at the end: its size follows the live
    // world, which streaming keeps to a handful of rooms however large the
    // game (Hollow at 201 rooms keeps six live at the start, phase 3 item 6).
    let bytes = p.report("the jump felt floaty", "report", "test").unwrap();
    assert!(
        bytes.len() < 128 * 1024,
        "a report is small: {} bytes",
        bytes.len()
    );
    let mut fresh = hollow();
    let out = fresh.play_report(&bytes).unwrap();
    println!("{out:#}");
    assert_eq!(out["tick"], now);
    assert_eq!(out["same"], true, "{out}");
    assert_eq!(out["meta"]["note"], "the jump felt floaty");
    assert_eq!(out["meta"]["kind"], "report");
    assert_eq!(fresh.digest(), p.digest());

    // A crash report has no world at the end, and still replays to its tick.
    let crash = p.report("panicked at ...", "crash", "test").unwrap();
    let mut fresh = hollow();
    let out = fresh.play_report(&crash).unwrap();
    assert_eq!(out["saved"], serde_json::Value::Null);
    assert_eq!(out["tick"], now);
    assert_eq!(fresh.digest(), p.digest());
}

fn delve() -> Player {
    Player::open(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/delve"),
        None,
        None,
    )
    .unwrap()
}

fn key(p: &mut Player, name: &str) {
    let k = |pressed| argus_input::DeviceInput::Key {
        name: name.into(),
        pressed,
    };
    p.devices(&[k(true)]);
    p.devices(&[k(false)]);
}

#[test]
fn on_a_turn_based_clock_a_report_reaches_back_by_turns_not_ticks() {
    // Issue 36: a player reads back through the log with the wheel before
    // reporting. Every notch is a tick of its own, frozen, which takes no
    // turn; the report's window counts turns, so the turns before the
    // reading are still in it.
    let mut p = delve();
    p.record();
    for _ in 0..5 {
        key(&mut p, "Period");
    }
    let waited = p.sched.turns_back(5).expect("five turns");
    let before = p.tick();
    for i in 0..200 {
        p.wheel(if i % 2 == 0 { 1.0 } else { -1.0 });
    }
    assert_eq!(p.tick(), before + 200, "each notch is a tick");
    assert_eq!(p.sched.turns_since(before), 0, "and none is a turn");
    assert!(
        p.recorder.as_ref().unwrap().from() <= waited,
        "the report starts before the turns: from {} > {waited}",
        p.recorder.as_ref().unwrap().from()
    );
    let bytes = p
        .report("that wait looked wrong", "report", "test")
        .unwrap();
    let mut fresh = delve();
    let out = fresh.play_report(&bytes).unwrap();
    assert_eq!(out["same"], true, "{out}");
    assert_eq!(fresh.digest(), p.digest());
}
