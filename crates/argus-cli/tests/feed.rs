//! Phase 4 item 0: every change is in the change feed as the `argus`
//! command that makes it, with its author, and the commands are ones the
//! agent could have issued: run through the CLI against a fresh copy of the
//! project, they make the same world.

#[path = "../../argus-daemon/tests/common/mod.rs"]
mod common;

use argus_cli::{run, EXIT_OK};
use argus_client::{command, Client};
use argus_daemon::{serve, socket_name_for, Transport};
use serde_json::{json, Value as Json};
use std::path::Path;

/// A daemon on its own socket over a fresh copy of the fixture project.
fn daemon(root: &Path, tag: &str) -> String {
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
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("daemon {tag} did not come up");
}

fn call(c: &mut Client, method: &str, params: Json) -> Json {
    let r = c.call(method, params).unwrap();
    assert!(r.get("error").is_none(), "{method}: {r}");
    r["result"].clone()
}

/// What the two worlds are compared on: the clock, the fields the changes
/// touched, and the entity files the commits wrote.
fn world(c: &mut Client, root: &Path) -> Json {
    let st = call(c, "sim.status", json!({}));
    let q = call(
        c,
        "world.query",
        json!({ "select": ["id", "Health.cur", "Health.max", "Light.intensity"], "where": { "has": ["Health"] }, "limit": 100 }),
    );
    let mut files = serde_json::Map::new();
    for e in std::fs::read_dir(root.join("scenes/arena"))
        .unwrap()
        .flatten()
    {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "toml") {
            files.insert(
                p.file_name().unwrap().to_string_lossy().into(),
                json!(std::fs::read_to_string(&p).unwrap().replace("\r\n", "\n")),
            );
        }
    }
    json!({ "tick": st["tick"], "frame": st["frame"], "speed": st["speed"], "rows": q["rows"], "files": files })
}

#[test]
fn every_change_is_a_command_the_agent_could_have_issued() {
    let (ta, tb) = (common::temp("feed-a"), common::temp("feed-b"));
    let a = daemon(ta.path(), "feed-a");
    let mut h = Client::connect(&a).unwrap();
    assert_eq!(
        call(&mut h, "session.identify", json!({ "author": "human" }))["author"],
        "human"
    );

    // What a person does in the editor, as the calls it makes.
    call(&mut h, "sim.step", json!({ "n": 30 }));
    let tx = call(
        &mut h,
        "edit.begin",
        json!({ "author": "human", "note": "a light on the player" }),
    )["tx"]
        .clone();
    call(
        &mut h,
        "edit.add",
        json!({ "tx": tx, "entity": "@player", "component": "Light" }),
    );
    call(
        &mut h,
        "edit.set",
        json!({ "tx": tx, "entity": "@player", "path": "Health.max", "value": 7 }),
    );
    call(&mut h, "edit.commit", json!({ "tx": tx }));
    call(&mut h, "world.get", json!({ "entity": "@player" }));
    call(
        &mut h,
        "tune.set",
        json!({ "entity": "@player", "path": "Light.intensity", "value": 2.5, "author": "human" }),
    );
    call(
        &mut h,
        "tune.set",
        json!({ "entity": "@player", "path": "Light.intensity", "value": 1.5, "author": "human" }),
    );
    call(&mut h, "tune.end", json!({}));
    let tx = call(
        &mut h,
        "edit.begin",
        json!({ "author": "human", "note": "too low" }),
    )["tx"]
        .clone();
    call(
        &mut h,
        "edit.set",
        json!({ "tx": tx, "entity": "@player", "path": "Health.cur", "value": 1 }),
    );
    call(&mut h, "edit.commit", json!({ "tx": tx }));
    call(&mut h, "edit.undo", json!({ "author": "human" }));
    call(&mut h, "sim.speed", json!({ "x": 3 }));
    call(&mut h, "sim.step", json!({ "n": 12 }));
    // Another client that says nothing is `cli`.
    let mut other = Client::connect(&a).unwrap();
    call(&mut other, "sim.step", json!({ "n": 1 }));

    let feed = call(&mut h, "edit.feed", json!({ "limit": 500 }));
    let rows = feed["rows"].as_array().unwrap().clone();
    let summary: Vec<(String, String, String)> = rows
        .iter()
        .map(|r| {
            (
                r["kind"].to_string(),
                r["author"].as_str().unwrap_or("").to_string(),
                r["command"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    let lines: Vec<&str> = summary.iter().map(|s| s.2.as_str()).collect();
    assert_eq!(
        lines,
        vec![
            "argus sim step n=30",
            "argus edit author=human note='a light on the player' ops='[{component: Light, entity: @player, op: add}, {entity: @player, op: set, path: Health.max, value: 7}]'",
            "argus tune set author=human entity=@player path=Light.intensity value=2.5",
            "argus tune set author=human entity=@player path=Light.intensity value=1.5",
            "argus tune end",
            "argus edit author=human note='too low' ops='[{entity: @player, op: set, path: Health.cur, value: 1}]'",
            "argus edit undo author=human",
            "argus sim speed x=3",
            "argus sim step n=12",
            "argus sim step n=1",
        ],
        "the feed, one row per change and none for the lookups: {summary:#?}"
    );
    let authors: Vec<&str> = summary.iter().map(|s| s.1.as_str()).collect();
    assert_eq!(authors[..9], ["human"; 9], "{summary:#?}");
    assert_eq!(authors[9], "cli");
    // A command row holds its call, and its line reads back to it.
    for r in rows.iter().filter(|r| r["kind"] == "command") {
        let (m, p) = command::read_line(r["command"].as_str().unwrap()).unwrap();
        assert_eq!(
            (json!(m), p),
            (r["method"].clone(), r["params"].clone()),
            "{r}"
        );
    }

    // The same commands, typed into the CLI against a fresh copy.
    let b = daemon(tb.path(), "feed-b");
    for l in &lines {
        let mut argv = vec!["--socket".to_string(), b.clone()];
        argv.extend(command::split(l).unwrap().into_iter().skip(1));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&argv, &mut out, &mut err);
        assert_eq!(
            code,
            EXIT_OK,
            "{l}: {}{}",
            String::from_utf8_lossy(&out),
            String::from_utf8_lossy(&err)
        );
    }
    let mut cb = Client::connect(&b).unwrap();
    let wa = world(&mut h, ta.path());
    let wb = world(&mut cb, tb.path());
    assert_eq!(wa, wb, "the same world from the feed's commands");
    assert_eq!(wa["tick"], 43, "{wa}");
    assert_eq!(wa["speed"], 3.0, "the speed the person set: {wa}");
    assert!(
        wa["files"]["player.toml"]
            .as_str()
            .unwrap()
            .contains("[Light]"),
        "the commit wrote the file: {wa}"
    );
}
