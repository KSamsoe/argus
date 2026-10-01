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

/// A tileset sprite with a sidecar, in both copies.
fn sprite(root: &Path) {
    argus_assets::encode_png(
        &root.join("assets/tiles/rock.png"),
        4,
        4,
        &[90, 80, 70, 255].repeat(16),
    )
    .unwrap();
    common::write(
        root,
        "assets/tiles/rock.toml",
        "# Rock.\ngrid = [1, 1]\n\n[[frames]]\nindex = 0\nproperties = { solid = true }\n",
    );
}

#[test]
fn a_def_and_an_asset_edit_are_commands_too() {
    // Found building phase 4 item 8: the edit tool took neither a def op
    // (a tuning session's commit, item 3) nor an asset op, so those feed
    // lines were not commands the agent could issue.
    let (ta, tb) = (common::temp("feed-da"), common::temp("feed-db"));
    common::project(ta.path());
    common::project(tb.path());
    sprite(ta.path());
    sprite(tb.path());
    let a = daemon(ta.path(), "feed-da");
    let mut h = Client::connect(&a).unwrap();
    call(&mut h, "session.identify", json!({ "author": "human" }));
    let tx = call(
        &mut h,
        "edit.begin",
        json!({ "author": "human", "note": "lighter keys, a one-way rock" }),
    )["tx"]
        .clone();
    call(
        &mut h,
        "edit.def",
        json!({ "tx": tx, "def": "item:key.brass", "path": "weight", "value": 0.05 }),
    );
    call(
        &mut h,
        "edit.asset",
        json!({ "tx": tx, "asset": "tiles/rock", "path": "frames[0].properties", "value": { "one_way": true } }),
    );
    call(&mut h, "edit.commit", json!({ "tx": tx }));
    let feed = call(&mut h, "edit.feed", json!({ "limit": 50 }));
    let line = feed["rows"].as_array().unwrap().last().unwrap()["command"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        line.contains("op: def") && line.contains("op: asset"),
        "{line}"
    );
    let b = daemon(tb.path(), "feed-db");
    let mut argv = vec!["--socket".to_string(), b.clone()];
    argv.extend(command::split(&line).unwrap().into_iter().skip(1));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run(&argv, &mut out, &mut err);
    assert_eq!(
        code,
        EXIT_OK,
        "{line}: {}{}",
        String::from_utf8_lossy(&out),
        String::from_utf8_lossy(&err)
    );
    for f in ["defs/items.toml", "assets/tiles/rock.toml"] {
        assert_eq!(
            common::read(ta.path(), f),
            common::read(tb.path(), f),
            "{f}"
        );
    }
    assert!(common::read(tb.path(), "assets/tiles/rock.toml").starts_with("# Rock."));
    assert!(common::read(tb.path(), "assets/tiles/rock.toml").contains("one_way = true"));
    // A def row can be fenced now too: the agent's edit of it is refused.
    call(
        &mut h,
        "edit.lock",
        json!({ "author": "human", "defs": ["item:key.brass"], "note": "mine" }),
    );
    let mut agent = Client::connect(&a).unwrap();
    let tx = call(
        &mut agent,
        "edit.begin",
        json!({ "author": "agent:claude", "note": "heavier" }),
    )["tx"]
        .clone();
    call(
        &mut agent,
        "edit.def",
        json!({ "tx": tx, "def": "item:key.brass", "path": "weight", "value": 1 }),
    );
    let r = agent.call("edit.commit", json!({ "tx": tx })).unwrap();
    assert!(
        r["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("mine"),
        "{r}"
    );
}

#[test]
fn a_timelines_rows_its_play_and_its_scrub_are_commands_too() {
    // Phase 4 item 9: the timeline editor's changes (a cue's field, a cue
    // added), its Play, and a drag along its ruler (one row), typed back
    // through the CLI on a fresh copy, make the same table and world.
    let (ta, tb) = (common::temp("feed-ta"), common::temp("feed-tb"));
    let table = "# A nudge.
[\"nudge\"]
length = 1.0

[\"nudge.hurt\"]
timeline = \"timeline:nudge\"
at = 0.25
kind = \"event\"
effects = \"actor:Health.cur = 4\"
";
    common::write(ta.path(), "defs/timeline.toml", table);
    common::write(tb.path(), "defs/timeline.toml", table);
    let a = daemon(ta.path(), "feed-ta");
    let b = daemon(tb.path(), "feed-tb");
    let mut h = Client::connect(&a).unwrap();
    call(&mut h, "session.identify", json!({ "author": "human" }));
    let mut cb = Client::connect(&b).unwrap();
    let tx = call(
        &mut h,
        "edit.begin",
        json!({ "author": "human", "note": "sooner, then heal" }),
    )["tx"]
        .clone();
    call(
        &mut h,
        "edit.def",
        json!({ "tx": tx, "def": "timeline:nudge.hurt", "path": "at", "value": 0.1 }),
    );
    call(
        &mut h,
        "edit.def_row",
        json!({ "tx": tx, "def": "timeline:nudge.heal", "fields": {
            "timeline": "timeline:nudge", "at": 0.5, "kind": "event", "effects": "actor:Health.cur = 9",
        } }),
    );
    call(&mut h, "edit.commit", json!({ "tx": tx }));
    call(
        &mut h,
        "timeline.play",
        json!({ "name": "nudge", "actor": "@player" }),
    );
    call(&mut h, "sim.step", json!({ "n": 60 }));
    for t in [0.3, 0.4] {
        call(
            &mut h,
            "timeline.seek",
            json!({ "name": "nudge", "time": t }),
        );
    }
    let feed = call(&mut h, "edit.feed", json!({ "limit": 50 }));
    let lines: Vec<String> = feed["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["author"] == "human")
        .map(|r| r["command"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(lines.len(), 4, "edit, play, step, one seek: {lines:#?}");
    assert!(lines[0].contains("op: def_row"), "{}", lines[0]);
    assert_eq!(lines[1], "argus timeline play actor=@player name=nudge");
    assert_eq!(lines[3], "argus timeline seek name=nudge time=0.4");
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
    assert_eq!(
        common::read(ta.path(), "defs/timeline.toml"),
        common::read(tb.path(), "defs/timeline.toml")
    );
    let wa = world(&mut h, ta.path());
    assert_eq!(wa, world(&mut cb, tb.path()), "the same world");
    let hp = wa["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| {
            r[0].as_str()
                .is_some_and(|i| i.trim_start_matches('@') == "player")
        })
        .unwrap_or_else(|| panic!("no player: {wa}"))[1]
        .clone();
    assert_eq!(hp, 4, "0.4 s in: hurt at 0.1, not yet healed at 0.5: {wa}");
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
