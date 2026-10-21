//! §16 "one schema generates the protocol, the CLI help, the MCP tool
//! descriptions, and the agent-facing docs" (phase 2 item 8): the schema
//! must say what the methods do. Every method it lists is answered, and each
//! one's required parameters are the truth: called with none, a method with
//! a required parameter (or a "one of" group) refuses, and a method without
//! one does not refuse for its parameters. The tools name only methods that
//! exist, and their parameters are unique.

mod common;

use argus_daemon::codes;
use argus_daemon::docs::{self, METHODS, TOOLS};
use common::*;
use serde_json::json;

/// Methods not called here: they open a window or a device, end the
/// session, or wait on the network, which this test must not do; or start
/// a match, which every later step would run (tests/netplay.rs has it).
const SKIP: &[&str] = &[
    "session.close",
    "session.shutdown",
    "render.window",
    "audio.device",
    "audio.play",
    "mark.pull",
    "net.sim",
];

#[test]
fn every_method_is_answered_and_its_required_params_are_the_truth() {
    let t = temp("schema");
    project(t.path());
    let mut d = open(t.path());
    let mut wrong = Vec::new();
    for m in METHODS {
        if SKIP.contains(&m.name) {
            continue;
        }
        let needs = m.params.iter().any(|p| p.required || p.one_of());
        let r = d.call(0, m.name, &json!({}));
        if let Err(e) = &r {
            assert_ne!(
                e.code,
                codes::UNKNOWN_METHOD,
                "{} is in the schema but not answered",
                m.name
            );
        }
        match (&r, needs) {
            (Ok(v), true) => wrong.push(format!(
                "{} lists a required parameter but answered without it: {}",
                m.name,
                v.to_string().chars().take(120).collect::<String>()
            )),
            (Err(e), false) if e.code == codes::BAD_PARAMS => wrong.push(format!(
                "{} lists no required parameter but refused: {}",
                m.name, e.message
            )),
            _ => {}
        }
    }
    assert!(
        wrong.is_empty(),
        "the schema is wrong about:\n{}",
        wrong.join("\n")
    );
    // Every parameter has a known type, and none is listed twice.
    let types = [
        docs::STRING,
        docs::INTEGER,
        docs::NUMBER,
        docs::BOOLEAN,
        docs::ARRAY,
        docs::OBJECT,
        docs::ANY,
    ];
    for (name, params) in METHODS
        .iter()
        .map(|m| (m.name, m.params))
        .chain(TOOLS.iter().map(|t| (t.name, t.params)))
    {
        for (i, p) in params.iter().enumerate() {
            assert!(types.contains(&p.ty), "{name}.{}: type {}", p.name, p.ty);
            assert!(
                !params[..i].iter().any(|q| q.name == p.name),
                "{name} lists {} twice",
                p.name
            );
        }
    }
}

#[test]
fn the_tools_call_methods_that_exist() {
    assert!(
        (10..=16).contains(&TOOLS.len()),
        "about a dozen tools, not a hundred"
    );
    for t in TOOLS {
        for m in t.calls {
            assert!(
                docs::find(m).is_some(),
                "tool {} calls {m}, which is not a method",
                t.name
            );
        }
        assert!(!t.doc.is_empty());
    }
}
