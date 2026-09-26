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
