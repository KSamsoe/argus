//! Review requests (§10, phase 4 item 4) through the protocol: the agent
//! asks with a clip of exact frames, replayed from the ring without
//! touching the live run; the human answers once; both are events, rows in
//! the change feed with their authors, and notifications on `reviews`.

mod common;

use common::*;
use serde_json::{json, Value as Json};
use std::path::Path;

fn call(d: &mut argus_daemon::daemon::Daemon, conn: u64, method: &str, params: Json) -> Json {
    match d.call(conn, method, &params) {
        Ok(v) => v,
        Err(e) => panic!("{method} failed: {e}"),
    }
}

fn player_x(d: &mut argus_daemon::daemon::Daemon) -> Json {
    ok(d, "world.get", json!({ "entity": "@player" }))["components"]["Transform"]["pos"].clone()
}

#[test]
fn the_agent_asks_with_a_clip_and_the_human_answers() {
    let t = temp("reviews-ask");
    project(t.path());
    let mut d = open(t.path());
    let (agent, human) = (1, 2);
    call(
        &mut d,
        agent,
        "session.identify",
        json!({ "author": "agent:claude" }),
    );
    call(
        &mut d,
        human,
        "session.identify",
        json!({ "author": "human" }),
    );
    call(
        &mut d,
        human,
        "obs.subscribe",
        json!({ "topics": ["reviews"] }),
    );
    ok(
        &mut d,
        "input.inject",
        json!({ "action": "move", "value": [1, 0], "frames": 200 }),
    );
    ok(&mut d, "sim.step", json!({ "n": 400 }));
    let before = (
        ok(&mut d, "sim.status", json!({}))["tick"].clone(),
        player_x(&mut d),
    );
    d.outbox.clear();

    // The last two seconds, by default, with three choices.
    let r = call(
        &mut d,
        agent,
        "obs.review",
        json!({ "question": "does the walk read as a run?", "options": ["yes", "no", "too fast"] }),
    );
    assert_eq!(r["id"], 1);
    assert_eq!(r["status"], "open");
    assert_eq!(
        (r["from"].clone(), r["to"].clone()),
        (json!(280), json!(400))
    );
    assert_eq!(r["asked_by"], "agent:claude");
    assert_eq!(r["options"], json!(["yes", "no", "too fast"]));
    if r["clip_skipped"].is_null() {
        // Two seconds at 30 pictures a second, and the first.
        assert_eq!(r["frames"], 61, "{r}");
        let png = std::fs::read(r["clip"].as_str().unwrap()).unwrap();
        assert!(png.windows(4).any(|w| w == b"acTL"), "an animated PNG");
        assert!(Path::new(r["sheet"].as_str().unwrap()).is_file());
    } else {
        eprintln!(
            "no graphics adapter: the clip was skipped: {}",
            r["clip_skipped"]
        );
    }
    // The clip was replayed from the ring: the live run is where it was.
    let after = (
        ok(&mut d, "sim.status", json!({}))["tick"].clone(),
        player_x(&mut d),
    );
    assert_eq!(before, after);
    // The human's editor hears of it.
    let heard: Vec<&Json> = d
        .outbox
        .iter()
        .filter(|(c, n)| *c == human && n["method"] == "review.asked")
        .map(|(_, n)| n)
        .collect();
    assert_eq!(heard.len(), 1, "{:?}", d.outbox);
    assert_eq!(
        heard[0]["params"]["question"],
        "does the walk read as a run?"
    );

    // Exact frames; frames that do not run forward, or have not happened.
    let r2 = call(
        &mut d,
        agent,
        "obs.review",
        json!({ "question": "is the start snappy?", "frames": [100, 160], "note": "accel 40" }),
    );
    assert_eq!(
        (r2["from"].clone(), r2["to"].clone()),
        (json!(100), json!(160))
    );
    assert_eq!(r2["options"], json!(["yes", "no"]));
    assert_eq!(r2["note"], "accel 40");
    if r2["clip_skipped"].is_null() {
        assert_eq!(r2["frames"], 31);
    }
    for frames in [json!([300, 200]), json!([390, 500]), json!([1, 2, 3])] {
        let e = d
            .call(
                agent,
                "obs.review",
                &json!({ "question": "?", "frames": frames }),
            )
            .unwrap_err();
        assert_eq!(e.code, argus_daemon::codes::BAD_PARAMS, "{frames}: {e}");
    }
    let e = d
        .call(agent, "obs.review", &json!({ "question": "  " }))
        .unwrap_err();
    assert_eq!(e.code, argus_daemon::codes::BAD_PARAMS);

    let q = ok(&mut d, "review.list", json!({}));
    assert_eq!(q["open"], 2);
    assert_eq!(q["rows"][0]["id"], 1, "oldest first");

    // The human answers with a choice; once.
    d.outbox.clear();
    let a = call(
        &mut d,
        human,
        "review.answer",
        json!({ "id": 1, "answer": "too fast" }),
    );
    assert_eq!(a["status"], "answered");
    assert_eq!(a["answered_by"], "human");
    assert_eq!(a["answered_frame"], 400);
    assert!(d
        .outbox
        .iter()
        .any(|(c, n)| *c == human && n["method"] == "review.answered"));
    let e = d
        .call(human, "review.answer", &json!({ "id": 1, "answer": "no" }))
        .unwrap_err();
    assert_eq!(e.code, argus_daemon::codes::BAD_REVIEW);
    assert!(e.message.contains("too fast"), "{e}");
    // A sentence is an answer too.
    call(
        &mut d,
        human,
        "review.answer",
        json!({ "id": "#2", "answer": "snappy, but the first step hitches" }),
    );

    // The agent reads the answers.
    let q = ok(&mut d, "review.list", json!({ "all": true }));
    assert_eq!(
        (q["open"].clone(), q["answered"].clone()),
        (json!(0), json!(2))
    );
    assert_eq!(q["rows"][0]["answer"], "too fast");
    assert_eq!(q["rows"][1]["answer"], "snappy, but the first step hitches");
    assert_eq!(ok(&mut d, "review.list", json!({}))["rows"], json!([]));
    let g = ok(&mut d, "review.get", json!({ "id": 2 }));
    assert_eq!(g["scene"], "arena");
    assert_eq!(g["seed"], 7);
    assert_eq!(
        err(&mut d, "review.get", json!({ "id": 9 })).code,
        argus_daemon::codes::NO_REVIEW
    );

    // Events, and the change feed with who did what.
    let ev = ok(&mut d, "obs.events", json!({ "name": "review.asked" }));
    assert_eq!(ev["rows"].as_array().unwrap().len(), 2, "{ev}");
    let ev = ok(&mut d, "obs.events", json!({ "name": "review.answered" }));
    assert_eq!(ev["rows"].as_array().unwrap().len(), 2, "{ev}");
    let feed = ok(&mut d, "edit.feed", json!({ "limit": 100 }));
    let rows = feed["rows"].as_array().unwrap();
    let asked: Vec<&Json> = rows
        .iter()
        .filter(|r| r["method"] == "obs.review")
        .collect();
    assert_eq!(asked.len(), 2, "{feed}");
    assert_eq!(asked[0]["author"], "agent:claude");
    assert!(
        asked[0]["command"]
            .as_str()
            .unwrap()
            .starts_with("argus obs review "),
        "{}",
        asked[0]["command"]
    );
    let answered: Vec<&Json> = rows
        .iter()
        .filter(|r| r["method"] == "review.answer")
        .collect();
    assert_eq!(answered.len(), 2, "{feed}");
    assert_eq!(answered[0]["author"], "human");
    // Lookups are not changes.
    assert!(!rows.iter().any(|r| r["method"] == "review.list"));
}

#[test]
fn a_review_on_a_discrete_clock_is_turns() {
    let t = temp("reviews-discrete");
    project(t.path());
    let mut d = open(t.path());
    ok(&mut d, "scene.load", json!({ "scene": "delve" }));
    ok(&mut d, "sim.advance", json!({ "n": 12 }));
    let r = ok(
        &mut d,
        "obs.review",
        json!({ "question": "does the turn read?" }),
    );
    assert_eq!((r["from"].clone(), r["to"].clone()), (json!(4), json!(12)));
    if r["clip_skipped"].is_null() {
        assert_eq!(r["frames"], 9, "one picture a turn: {r}");
    }
}
