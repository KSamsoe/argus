//! Phase 4 item 4: the agent panel, driven headless over a real socket on
//! a copy of the fixture project. The agent asks a question with a clip,
//! proposes a change, and the person's mark waits for it; the person
//! scrubs the clip and answers with a key, answers another with a
//! sentence, tries the proposal (A and B at the same moment) and keeps B,
//! and edits and closes the mark. The agent reads the answers, and the
//! panel shows what it did.

#[path = "../../argus-daemon/tests/common/mod.rs"]
mod common;

use argus_client::Client;
use argus_daemon::{serve, socket_name_for, Transport};
use argus_editor::app::Tab;
use argus_editor::Editor;
use eframe::egui::{self, Key};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use serde_json::{json, Value as Json};
use std::path::Path;
use std::time::{Duration, Instant};

fn daemon(root: &Path) -> String {
    common::project(root);
    let name = format!("{}-agent", socket_name_for(root));
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
        "timed out waiting for {what}; problem {:?}; reviews {:?}; proposals {}; marks {:?}",
        h.state().problem,
        h.state().agent.reviews,
        h.state().agent.proposals,
        h.state().agent.marks
    );
}

fn result(c: &mut Client, method: &str, params: Json) -> Json {
    let r = c.call(method, params).unwrap();
    assert!(r.get("error").is_none(), "{method}: {r}");
    r["result"].clone()
}

fn save(h: &mut Harness<'_, Editor>, name: &str) {
    let img = h.render().expect("drawn");
    let (w, hh) = img.dimensions();
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let f = std::io::BufWriter::new(std::fs::File::create(&out).unwrap());
    let mut enc = png::Encoder::new(f, w, hh);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&img.into_raw())
        .unwrap();
}

#[test]
fn a_person_answers_the_agent_tries_its_proposal_and_closes_a_mark() {
    let t = common::temp("editor-agent");
    let name = daemon(t.path());
    let mut agent = Client::connect(&name).unwrap();
    result(
        &mut agent,
        "session.identify",
        json!({ "author": "agent:claude" }),
    );
    // A run to look at, a mark of the person's, a question, a proposal.
    result(
        &mut agent,
        "input.inject",
        json!({ "action": "move", "value": [1, 0], "frames": 200 }),
    );
    result(&mut agent, "sim.step", json!({ "n": 400 }));
    result(
        &mut agent,
        "mark.drop",
        json!({ "note": "the shot went nowhere", "author": "human", "clip": false }),
    );
    let asked = result(
        &mut agent,
        "obs.review",
        json!({
            "question": "does the walk read as a run?",
            "options": ["yes", "no", "too fast"],
            "note": "walk speed 4",
        }),
    );
    let tx = result(
        &mut agent,
        "edit.begin",
        json!({ "author": "agent:claude", "note": "tougher player" }),
    )["tx"]
        .clone();
    result(
        &mut agent,
        "edit.set",
        json!({ "tx": tx, "entity": "@player", "path": "Health.max", "value": 5 }),
    );
    let proposed = result(
        &mut agent,
        "edit.propose",
        json!({ "tx": tx, "note": "a tougher player" }),
    );
    let pid = proposed["proposal"].as_u64().unwrap();

    let mut h = Harness::builder()
        .with_size(egui::vec2(1500.0, 1000.0))
        .wgpu()
        .build_state(|ctx, ed: &mut Editor| ed.ui(ctx), {
            let mut ed = Editor::new(&name, "human");
            ed.audio = false;
            ed
        });
    until(&mut h, "the bar", |ed| ed.status["project"] == "fixture");
    until(&mut h, "the agent's queue", |ed| ed.agent.waiting() == 2);
    h.step();
    // The bar says so, and takes the person there.
    h.get_by_label_contains("The agent asks: 1 question, 1 proposal")
        .click();
    h.step();
    assert_eq!(h.state().tab, Tab::Agent);
    h.get_by_label("Agent (2)");

    // The clip, read and scrubbed frame by frame.
    let frames = asked["frames"].as_u64().unwrap_or(0) as usize;
    if frames > 0 {
        until(&mut h, "the clip's frames", |ed| {
            ed.agent.clip_frames() == frames
        });
        h.step();
        h.get_by_label_contains("frame 280");
        h.state_mut().agent.scrub.at = 30;
        h.step();
        h.get_by_label_contains("frame 340");
        h.get_by_label("Play clip").click();
        h.step();
        std::thread::sleep(Duration::from_millis(150));
        h.step();
        assert!(h.state().agent.scrub.playing);
        assert_ne!(h.state().agent.scrub.at, 30, "it plays");
        h.get_by_label("Pause").click();
        h.step();
        save(&mut h, "editor-agent.png");
    } else {
        eprintln!("no graphics adapter: the clip was skipped");
    }
    // The question (and, in what the agent did, the command that asked it).
    assert!(h
        .get_all_by_label_contains("does the walk read as a run?")
        .next()
        .is_some());
    h.get_by_label("walk speed 4");

    // One key: the third choice.
    h.press_key(Key::Num3);
    until(&mut h, "the question answered", |ed| {
        ed.agent.reviews.is_empty()
    });
    let answers = result(&mut agent, "review.list", json!({ "all": true }));
    assert_eq!(answers["rows"][0]["answer"], "too fast", "{answers}");
    assert_eq!(answers["rows"][0]["answered_by"], "human");

    // A second question, answered in a sentence.
    result(
        &mut agent,
        "obs.review",
        json!({ "question": "is the start snappy?", "frames": [100, 160] }),
    );
    until(&mut h, "the second question", |ed| {
        ed.agent.reviews.len() == 1
    });
    h.step();
    h.state_mut().agent.answer = "snappy, but the first step hitches".into();
    h.step();
    h.get_by_label("Send").click();
    until(&mut h, "the sentence sent", |ed| {
        ed.agent.reviews.is_empty()
    });
    let answers = result(&mut agent, "review.list", json!({ "all": true }));
    assert_eq!(
        answers["rows"][1]["answer"], "snappy, but the first step hitches",
        "{answers}"
    );

    // The proposal: tried, switched, kept.
    h.get_by_label("Try it").click();
    until(&mut h, "the trial, showing B", |ed| {
        ed.agent.proposals["trial"]["proposal"] == pid
            && ed.agent.proposals["trial"]["showing"] == "B"
    });
    h.step();
    h.get_by_label("A: as it is").click();
    until(&mut h, "A showing", |ed| {
        ed.agent.proposals["trial"]["showing"] == "A"
    });
    h.step();
    h.get_by_label("B: tougher player").click();
    until(&mut h, "B showing again", |ed| {
        ed.agent.proposals["trial"]["showing"] == "B"
    });
    h.step();
    h.get_by_label("Keep B").click();
    until(&mut h, "the proposal kept", |ed| {
        ed.agent.proposals["pending"] == 0
    });
    let max = result(&mut agent, "world.get", json!({ "entity": "@player" }))["components"]
        ["Health"]["max"]
        .clone();
    assert_eq!(max, 5, "the pick is the commit");
    let feed = result(&mut agent, "edit.feed", json!({ "limit": 200 }));
    let picked = format!("proposal {pid} B picked by human");
    assert!(
        feed["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["author"] == "human"
                && r["note"].as_str().is_some_and(|n| n.starts_with(&picked))),
        "{feed}"
    );

    // The mark: its note set right, then closed.
    until(&mut h, "the mark", |ed| ed.agent.marks.len() == 1);
    h.step();
    h.state_mut()
        .agent
        .mark_notes
        .insert(1, "the shot never lands".into());
    h.step();
    h.get_by_label("Save note").click();
    until(&mut h, "the note saved", |ed| {
        ed.agent
            .marks
            .first()
            .is_some_and(|m| m["note"] == "the shot never lands")
    });
    h.get_by_label("Close").click();
    until(&mut h, "the mark closed", |ed| ed.agent.marks.is_empty());
    let marks = result(&mut agent, "mark.list", json!({ "all": true }));
    assert_eq!(marks["closed"], 1, "{marks}");
    assert_eq!(marks["rows"][0]["note"], "the shot never lands");

    // What the agent did, as notes, and as commands where it wrote none.
    h.step();
    assert!(h
        .get_all_by_label_contains("tougher player")
        .next()
        .is_some());
    assert!(h
        .get_all_by_label_contains("argus obs review")
        .next()
        .is_some());
    // The change feed, only the agent's rows.
    h.get_by_label("Change feed").click();
    h.state_mut().feed_author = Some("agent:claude".into());
    h.step();
    h.step();
    assert!(h
        .query_all_by_label_contains("argus review answer")
        .next()
        .is_none());
    assert!(h
        .get_all_by_label_contains("argus obs review")
        .next()
        .is_some());
    h.state_mut().feed_author = Some("human".into());
    h.step();
    h.step();
    assert!(h
        .query_all_by_label_contains("argus obs review")
        .next()
        .is_none());
    assert!(h
        .get_all_by_label_contains("argus review answer")
        .next()
        .is_some());
}
