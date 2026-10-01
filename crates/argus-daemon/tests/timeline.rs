//! Timelines through the protocol (phase 3 item 5, §13): the table and
//! what is playing (timeline.list), a timeline scrubbed to a time
//! (timeline.seek, the editor's scrubber), its replay the same as the run,
//! and rows that do not read in script.errors.

mod common;

use common::*;
use serde_json::json;

fn show(root: &std::path::Path) {
    write(
        root,
        "scenes/arena/lamp.toml",
        "[Transform]\npos = [5.0, 0.0]\n\n[Light]\nintensity = 1.0\n",
    );
    write(
        root,
        "defs/timeline.toml",
        r#"# The lamp comes up over a second, then flares.
["show"]
length = 2.0

["show.glow"]
timeline = "timeline:show"
track = "lamp"
at = 0.5
kind = "tween"
who = "@lamp"
path = "Light.intensity"
to = [5.0, 0.0]
duration = 1.0

["show.flare"]
timeline = "timeline:show"
track = "lamp"
at = 1.5
kind = "event"
effects = "@lamp:Light.intensity = 9; emit flared"
"#,
    );
    // Played once, from half a second in.
    write(
        root,
        "systems/show.luau",
        r#"system "sim.show" {
  stage = "sim",
  writes = { "Timeline" },
  doc = "Plays the show once, at tick 30.",
  run = function(q)
    if q.tick == 30 then
      q:spawn({ "Timeline" }):get("Timeline").name = "timeline:show"
    end
  end,
}
"#,
    );
}

fn intensity(d: &mut argus_daemon::daemon::Daemon) -> f64 {
    let g = ok(d, "world.get", json!({ "entity": "@lamp" }));
    g["components"]["Light"]["intensity"].as_f64().unwrap()
}

fn tx(
    d: &mut argus_daemon::daemon::Daemon,
    note: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, argus_ecs::ArgusError> {
    let t = d.call(1, "edit.begin", &json!({ "author": "human", "note": note }))?["tx"].clone();
    let mut p = params;
    p["tx"] = t.clone();
    if let Err(e) = d.call(1, method, &p) {
        let _ = d.call(1, "edit.abort", &json!({ "tx": t }));
        return Err(e);
    }
    d.call(1, "edit.commit", &json!({ "tx": t }))
}

fn cue_names(d: &mut argus_daemon::daemon::Daemon) -> Vec<String> {
    let l = ok(d, "timeline.list", json!({ "name": "show" }));
    let mut out = Vec::new();
    for t in l["timelines"][0]["tracks"].as_array().unwrap() {
        for c in t["cues"].as_array().unwrap() {
            out.push(c[1].as_str().unwrap().to_string());
        }
    }
    out
}

#[test]
fn a_timeline_is_edited_row_by_row_and_played_from_the_editor() {
    let t = temp("timeline-edit");
    project(t.path());
    show(t.path());
    // No script plays it: the editor does.
    std::fs::remove_file(t.path().join("systems/show.luau")).unwrap();
    let mut d = open(t.path());
    d.call(1, "session.identify", &json!({ "author": "human" }))
        .unwrap();
    let e = d
        .call(1, "timeline.play", &json!({ "name": "nothing" }))
        .unwrap_err();
    assert!(e.message.contains("no timeline"), "{e}");
    let p = d
        .call(1, "timeline.play", &json!({ "name": "show" }))
        .unwrap();
    assert_eq!(p["name"], "show");
    ok(&mut d, "sim.step", json!({ "n": 30 }));
    let l = ok(&mut d, "timeline.list", json!({}));
    assert_eq!(l["playing"][0]["name"], "show", "{l}");
    ok(&mut d, "sim.step", json!({ "n": 120 }));
    assert_eq!(intensity(&mut d), 9.0, "it flared");
    let feed = ok(&mut d, "edit.feed", json!({ "limit": 50 }));
    assert!(
        feed["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["command"] == "argus timeline play name=show"),
        "{feed}"
    );

    // A cue moved (a field of its row), and one added (a row).
    tx(
        &mut d,
        "flare sooner",
        "edit.def",
        json!({ "def": "timeline:show.flare", "path": "at", "value": 1.2 }),
    )
    .unwrap();
    let c = tx(
        &mut d,
        "then dim",
        "edit.def_row",
        json!({ "def": "timeline:show.dim", "fields": {
            "timeline": "timeline:show", "track": "lamp", "at": 1.8, "kind": "event",
            "effects": "@lamp:Light.intensity = 2; emit dimmed",
        } }),
    )
    .unwrap();
    assert_eq!(c["files"], json!(["defs/timeline.toml"]));
    assert!(
        c["command"].as_str().unwrap().contains("op: def_row"),
        "{}",
        c["command"]
    );
    let text = read(t.path(), "defs/timeline.toml");
    assert!(
        text.starts_with("# The lamp comes up over a second, then flares."),
        "{text}"
    );
    assert!(
        text.contains("[\"show.dim\"]") && text.contains("at = 1.8"),
        "{text}"
    );
    assert_eq!(cue_names(&mut d), ["show.glow", "show.flare", "show.dim"]);
    // Played again: the new cue fires.
    d.call(1, "timeline.play", &json!({ "name": "show" }))
        .unwrap();
    ok(&mut d, "sim.step", json!({ "n": 150 }));
    assert_eq!(intensity(&mut d), 2.0, "it dimmed");
    let ev = ok(&mut d, "obs.events", json!({ "name": "dimmed" }));
    assert_eq!(ev["total"], 1, "{ev}");

    // A cue of a kind there is not: nothing kept.
    let e = tx(
        &mut d,
        "blink",
        "edit.def_row",
        json!({ "def": "timeline:show.blink", "fields": { "timeline": "timeline:show", "kind": "blink" } }),
    )
    .unwrap_err();
    assert!(e.message.contains("would not read"), "{e}");
    assert_eq!(read(t.path(), "defs/timeline.toml"), text);
    let e = tx(
        &mut d,
        "no",
        "edit.def_row",
        json!({ "def": "timeline:show.x", "fields": { "nope": 1 } }),
    )
    .unwrap_err();
    assert!(e.message.contains("no field"), "{e}");
    // A field of a row the timeline cannot read: refused the same way.
    let e = tx(
        &mut d,
        "blink",
        "edit.def",
        json!({ "def": "timeline:show.dim", "path": "kind", "value": "blink" }),
    )
    .unwrap_err();
    assert!(e.message.contains("would not read"), "{e}");
    assert_eq!(read(t.path(), "defs/timeline.toml"), text);
    // The rows as the file writes them: an editor's fields.
    let l = ok(&mut d, "timeline.list", json!({ "name": "show" }));
    assert_eq!(l["timelines"][0]["fields"], json!({ "length": 2.0 }), "{l}");
    let cues = &l["timelines"][0]["tracks"][0]["cues"];
    assert_eq!(cues[2][1], "show.dim");
    assert_eq!(
        cues[2][9]["effects"], "@lamp:Light.intensity = 2; emit dimmed",
        "{cues}"
    );
    assert!(
        cues[0][9].get("from").is_none(),
        "the glow starts where the field is: {cues}"
    );
    assert!(
        l["easings"]
            .as_array()
            .unwrap()
            .contains(&json!("in_out_sine")),
        "{l}"
    );
    assert_eq!(l["kinds"].as_array().unwrap().len(), 5);

    // Taken out, then back with undo: the file's exact text.
    tx(
        &mut d,
        "no dim",
        "edit.def_row",
        json!({ "def": "timeline:show.dim", "remove": true }),
    )
    .unwrap();
    assert!(!read(t.path(), "defs/timeline.toml").contains("show.dim"));
    assert_eq!(cue_names(&mut d), ["show.glow", "show.flare"]);
    d.call(1, "edit.undo", &json!({ "author": "human" }))
        .unwrap();
    assert_eq!(read(t.path(), "defs/timeline.toml"), text);
    assert_eq!(cue_names(&mut d), ["show.glow", "show.flare", "show.dim"]);
    // Scrubbed: the latest play at a second and a half in.
    let s = d
        .call(1, "timeline.seek", &json!({ "name": "show", "time": 1.5 }))
        .unwrap();
    assert_eq!(s["reached"], true, "{s}");
    assert!(
        s["diverged"].is_null(),
        "from the keyframe its play made: {s}"
    );
    // The glow's tween runs to 1.5 over the flare at 1.2: its last value.
    assert_eq!(intensity(&mut d), 5.0, "{s}");
    let s = d
        .call(1, "timeline.seek", &json!({ "name": "show", "time": 1.9 }))
        .unwrap();
    assert_eq!(s["reached"], true, "{s}");
    assert_eq!(intensity(&mut d), 2.0, "past the dim at 1.8");
    // Two seeks in a row are one move: one row, the time let go at.
    let feed = ok(&mut d, "edit.feed", json!({ "limit": 50 }));
    let seeks: Vec<&serde_json::Value> = feed["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| {
            r["command"]
                .as_str()
                .unwrap_or("")
                .starts_with("argus timeline seek")
        })
        .collect();
    assert_eq!(seeks.len(), 1, "{feed}");
    assert!(
        seeks[0]["command"].as_str().unwrap().contains("time=1.9"),
        "{}",
        seeks[0]
    );
}

#[test]
fn a_timeline_is_listed_played_and_scrubbed() {
    let t = temp("timeline");
    project(t.path());
    show(t.path());
    let mut d = open(t.path());
    let l = ok(&mut d, "timeline.list", json!({}));
    assert_eq!(l["timelines"][0]["name"], "show");
    assert_eq!(l["timelines"][0]["length"], 2.0);
    let cues = &l["timelines"][0]["tracks"][0];
    assert_eq!(cues["track"], "lamp");
    assert_eq!(cues["cues"][0][1], "show.glow");
    assert_eq!(cues["cues"][0][5], "@lamp");
    assert_eq!(l["playing"], json!([]));
    assert_eq!(l["problems"], json!([]));
    let defs = ok(&mut d, "world.defs", json!({ "table": "timeline" }));
    assert_eq!(defs["total"], 3);

    // Played through: its cues at their times, the lamp's field moved.
    ok(
        &mut d,
        "sim.run_until",
        json!({ "event": "timeline.started" }),
    );
    let l = ok(&mut d, "timeline.list", json!({ "name": "show" }));
    assert_eq!(l["playing"][0]["name"], "show");
    let r = ok(
        &mut d,
        "sim.run_until",
        json!({ "event": "timeline.ended" }),
    );
    assert_eq!(r["hit"], true);
    assert_eq!(intensity(&mut d), 9.0);
    ok(&mut d, "sim.step", json!({ "n": 30 }));
    let ev = ok(&mut d, "obs.events", json!({ "name": "timeline.cue" }));
    assert_eq!(ev["total"], 2, "{ev}");

    // Scrubbed: a second in, the glow is half way; the replay matches.
    let s = ok(
        &mut d,
        "timeline.seek",
        json!({ "name": "show", "time": 1.0 }),
    );
    assert_eq!(s["reached"], true, "{s}");
    assert_eq!(s["diverged"], json!(null), "{s}");
    assert_eq!(s["ended"], false);
    let time = s["time"].as_f64().unwrap();
    assert!((0.999..1.02).contains(&time), "{s}");
    let x = intensity(&mut d);
    assert!(x > 2.5 && x < 3.5, "half way up: {x}");
    // Back before the glow: the lamp as it was.
    let tl = s["timeline"].as_str().unwrap().to_string();
    let s = ok(
        &mut d,
        "timeline.seek",
        json!({ "timeline": tl, "time": 0.25 }),
    );
    assert_eq!(s["diverged"], json!(null), "{s}");
    assert_eq!(intensity(&mut d), 1.0);
    // Past its end: the frame it ended.
    let s = ok(
        &mut d,
        "timeline.seek",
        json!({ "name": "show", "time": 30.0 }),
    );
    assert_eq!(s["ended"], true, "{s}");
    assert_eq!(intensity(&mut d), 9.0);
    let e = err(
        &mut d,
        "timeline.seek",
        json!({ "name": "other", "time": 1.0 }),
    );
    assert!(e.message.contains("no timeline \"other\""), "{e}");

    // A row that does not read is a script error, phase timeline.
    assert_eq!(ok(&mut d, "script.errors", json!({}))["total"], 0);
    write(
        t.path(),
        "defs/timeline.toml",
        "[\"show\"]\n\n[\"show.odd\"]\ntimeline = \"timeline:show\"\nkind = \"juggle\"\n",
    );
    ok(&mut d, "script.reload", json!({}));
    let errors = ok(&mut d, "script.errors", json!({}));
    let rows = errors["rows"].as_array().unwrap();
    let bad = rows
        .iter()
        .find(|r| r["phase"] == "timeline")
        .unwrap_or_else(|| panic!("{errors}"));
    assert_eq!(bad["file"], "defs/timeline.toml");
    assert!(
        bad["message"]
            .as_str()
            .unwrap()
            .contains("show.odd: kind \"juggle\""),
        "{bad}"
    );
}
