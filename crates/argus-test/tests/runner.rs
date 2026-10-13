//! §9: scenarios load a scene, fix a seed, script input, run frames, and
//! assert on state and events; state goldens with field-level diffs; seed
//! sweeps; headless in tens of milliseconds; never flakes (§6).

#[path = "../../argus-daemon/tests/common/mod.rs"]
mod common;

use argus_daemon::Daemon;
use argus_test::{list, parse_seeds, run, Driver, Options};
use serde_json::{json, Value as Json};
use std::path::PathBuf;

struct Local(Daemon, PathBuf);

impl Driver for Local {
    fn call(&mut self, method: &str, params: &Json) -> Result<Json, Json> {
        self.0
            .call(0, method, params)
            .map_err(|e| argus_daemon::json::error_json(&e))
    }
    fn root(&self) -> PathBuf {
        self.1.clone()
    }
}

fn fixture(name: &str) -> (common::Temp, Local) {
    let t = common::temp(name);
    common::project(t.path());
    let d = common::open(t.path());
    let root = t.path().to_path_buf();
    (t, Local(d, root))
}

const WAVE: &str = r#"
return scenario "the bat dies of attrition" {
  scene = "arena", seed = 7,
  run = function(t)
    t.input:hold("move", {1, 0}, 3)
    t.input:tap("fire")
    t:run_until { event = "death", max_frames = 100 }
    t.expect:count({ has = {"Enemy"} }):eq(1)
    t.expect:field("@bat", "Health.cur"):eq(0)
    t.expect:field("@player", "Health.cur"):gte(1)
    t.expect:events("death", { cause = "attrition" }):count():eq(1)
    t.expect:events("shot"):exists()
    t.expect:field("@player", "Transform.pos"):eq({ 4, 0 })  -- two ticks of walking before the bat died
    t.expect:sum("Health.cur", { has = {"Enemy"} }):eq(0)
    t.expect:snapshot("after_death", { select = {"Transform.pos", "Health.cur"}, where = { has = {"Health"} } })
    t.expect:perf("sim"):p95_ms():lt(50.0)
    t.expect:perf("tick"):p95_ms():lt(50.0)
    t:log("bat down at tick " .. t:status().tick)
  end,
}
"#;

const SWEEP: &str = r#"
return scenario "health never goes negative" {
  scene = "arena", seeds = range(1, 5),
  run = function(t)
    t:run_until { frames = 10 }
    t.expect:field("@bat", "Health.cur"):gte(0)
    t.expect:count({ has = {"Enemy"}, within = { of = "@player", r = 100 } }):eq(1)
  end,
}
"#;

const DISCRETE: &str = r#"
return scenario "delve turns" {
  scene = "delve",
  run = function(t)
    t:advance { by = "turn", n = 2 }
    t.expect:count({ has = {"Health"} }):eq(1)
    t.expect:path("@hero", { has = {"Health"} }):exists()
    t.expect:count({ has = {"Enemy"} }):eq(0)
  end,
}
"#;

#[test]
fn a_scenario_runs_headless_and_every_expectation_kind_reports() {
    let (t, mut d) = fixture("runner");
    common::write(t.path(), "tests/wave.luau", WAVE);
    common::write(t.path(), "tests/sweep.luau", SWEEP);
    common::write(t.path(), "tests/delve.luau", DISCRETE);

    let l = list(t.path(), None).unwrap();
    assert_eq!(l["total"], 3);
    assert_eq!(l["rows"][0]["file"], "tests/delve.luau");
    assert_eq!(l["rows"][1]["seeds"], json!({ "from": 1, "to": 5 }));
    assert_eq!(l["rows"][1]["runs"], 5);
    assert_eq!(l["rows"][2]["name"], "the bat dies of attrition");
    assert_eq!(l["rows"][2]["seeds"], json!([7]));

    let r = run(&mut d, &Options::default()).unwrap();
    assert_eq!(
        r["ok"],
        true,
        "{}",
        serde_json::to_string_pretty(&r).unwrap()
    );
    assert_eq!(r["total_runs"], 7);
    assert_eq!(r["passed_runs"], 7);
    assert!(r["ms"].as_f64().unwrap() < 5000.0);
    let wave = &r["scenarios"][2];
    assert_eq!(wave["name"], "the bat dies of attrition");
    let seed7 = &wave["seeds"][0];
    assert_eq!(seed7["seed"], 7);
    assert_eq!(seed7["ok"], true);
    assert!(
        seed7["ms"].as_f64().unwrap() < 1000.0,
        "tens of milliseconds: {}",
        seed7["ms"]
    );
    let kinds: Vec<&str> = seed7["expectations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        vec![
            "count", "field", "field", "events", "events", "field", "sum", "snapshot", "perf",
            "perf"
        ]
    );
    let snap = &seed7["expectations"][7];
    assert_eq!(snap["recorded"], true, "first run records the golden");
    let golden = t
        .path()
        .join("tests/goldens/the_bat_dies_of_attrition/after_death.seed7.json");
    assert!(golden.exists(), "{golden:?}");
    let g: Json = serde_json::from_str(&std::fs::read_to_string(&golden).unwrap()).unwrap();
    assert_eq!(g["select"], json!(["id", "Transform.pos", "Health.cur"]));
    assert_eq!(g["rows"].as_array().unwrap().len(), 2, "player and bat");
    assert!(seed7["expectations"][8]["message"]
        .as_str()
        .unwrap()
        .contains("x1.5 margin"));
    assert_eq!(seed7["log"][0].as_str().unwrap(), "bat down at tick 2");
    // The delve scenario had no seed: the scene's own is reported.
    assert_eq!(r["scenarios"][0]["seeds"][0]["seed"], 1);
    assert_eq!(r["scenarios"][0]["seeds"][0]["expectations"][1]["ok"], true);

    // Second run: the golden matches, bit for bit (§6).
    let r2 = run(
        &mut d,
        &Options {
            name: Some("bat dies".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r2["total_runs"], 1);
    let snap = &r2["scenarios"][0]["seeds"][0]["expectations"][7];
    assert_eq!(snap["ok"], true);
    assert!(snap.get("recorded").is_none());
    assert!(snap["message"].as_str().unwrap().contains("matches"));

    // A drifted golden fails with a field-level diff.
    let mut g2 = g.clone();
    g2["rows"][0][2] = json!(99);
    g2["rows"]
        .as_array_mut()
        .unwrap()
        .push(json!(["@ghost", [0, 0], 1]));
    std::fs::write(&golden, serde_json::to_string_pretty(&g2).unwrap()).unwrap();
    let r3 = run(
        &mut d,
        &Options {
            file: Some("tests/wave.luau".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r3["ok"], false);
    let snap = &r3["scenarios"][0]["seeds"][0]["expectations"][7];
    assert_eq!(snap["ok"], false);
    assert_eq!(snap["diff"]["changed"][0]["field"], "Health.cur");
    assert_eq!(snap["diff"]["changed"][0]["golden"], 99);
    assert_eq!(snap["diff"]["removed"], json!(["@ghost"]));
    assert!(snap["message"]
        .as_str()
        .unwrap()
        .contains("Health.cur: golden 99, now"));
    // --update rewrites it.
    let r4 = run(
        &mut d,
        &Options {
            file: Some("tests/wave.luau".into()),
            update_goldens: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r4["ok"], true);
    assert!(r4["scenarios"][0]["seeds"][0]["expectations"][7]["message"]
        .as_str()
        .unwrap()
        .contains("updated"));

    // A seed sweep override, and a failing expectation names itself.
    let r5 = run(
        &mut d,
        &Options {
            name: Some("health never".into()),
            seeds: Some("1..3".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r5["total_runs"], 3);
    assert_eq!(r5["scenarios"][0]["seeds"][2]["seed"], 3);
    common::write(
        t.path(),
        "tests/bad.luau",
        r#"return scenario "wrong" { scene = "arena", seed = 1, run = function(t)
          t:run_until { frames = 5 }
          t.expect:count({ has = {"Enemy"} }):eq(0)
          t.expect:field("@player", "Health.cur"):between(4, 9)
        end }"#,
    );
    let r6 = run(
        &mut d,
        &Options {
            name: Some("wrong".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r6["ok"], false);
    let ex = &r6["scenarios"][0]["seeds"][0]["expectations"];
    assert_eq!(ex[0]["ok"], false);
    assert_eq!(ex[0]["message"], "count {has = [\"Enemy\"]}: 1 == 0 failed");
    assert_eq!(ex[1]["message"], "@player Health.cur: 3 in [4, 9] failed");

    // A scenario that errors is a failed run with the file:line, not a crash;
    // a file that does not load is reported and fails the run.
    common::write(
        t.path(),
        "tests/boom.luau",
        "return scenario \"boom\" { scene = \"arena\", seed = 1, run = function(t)\n  t:call(\"world.get\", {})\nend }",
    );
    common::write(
        t.path(),
        "tests/syntax.luau",
        "return scenario \"x\" { scene = ",
    );
    let r7 = run(
        &mut d,
        &Options {
            name: Some("boom".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r7["ok"], false);
    let e = r7["scenarios"][0]["seeds"][0]["error"].as_str().unwrap();
    assert!(e.contains("tests/boom.luau:2"), "{e}");
    assert!(e.contains("world.get: error 4704"), "{e}");
    assert_eq!(r7["load_errors"][0]["file"], "tests/syntax.luau");
}

#[test]
fn a_series_declared_after_the_run_is_sampled_by_rerunning() {
    let (t, mut d) = fixture("series");
    common::write(
        t.path(),
        "tests/food.luau",
        r#"return scenario "enemy health falls" {
  scene = "arena", seed = 7,
  run = function(t)
    t:run_until { sim_time = seconds(0.1) }
    t.expect:series("Health.cur", { every = seconds(0.05), where = { has = {"Enemy"} } }):trend():lte(0)
    t.expect:sum("Health.cur", { has = {"Enemy"} }):eq(0)
  end,
}"#,
    );
    let r = run(&mut d, &Options::default()).unwrap();
    assert_eq!(
        r["ok"],
        true,
        "{}",
        serde_json::to_string_pretty(&r).unwrap()
    );
    let s = &r["scenarios"][0]["seeds"][0];
    assert_eq!(
        s["reran"], true,
        "deterministic rerun with the series in place"
    );
    let e = &s["expectations"][0];
    assert_eq!(e["kind"], "series");
    assert!(e["actual"].as_f64().unwrap() < 0.0, "{e}");
    assert_eq!(s["tick"], 6);

    // Declared before the run, it samples during it and no rerun happens.
    common::write(
        t.path(),
        "tests/food.luau",
        r#"return scenario "enemy health falls" {
  scene = "arena", seed = 7,
  run = function(t)
    local hp = t.expect:series("Health.cur", { every = seconds(0.05), where = { has = {"Enemy"} } })
    t:run_until { frames = 6 }
    hp:trend():lt(0)
    hp:last():eq(0)
    hp:max():eq(2)
  end,
}"#,
    );
    let r = run(&mut d, &Options::default()).unwrap();
    assert_eq!(
        r["ok"],
        true,
        "{}",
        serde_json::to_string_pretty(&r).unwrap()
    );
    let s = &r["scenarios"][0]["seeds"][0];
    assert!(s.get("reran").is_none());
    assert_eq!(s["expectations"].as_array().unwrap().len(), 3);
}

/// Found building Hollow: `release` sent an inject with no value, which is the
/// action's active value, so it pressed instead of letting go.
#[test]
fn a_release_lets_go_and_a_field_path_reads_one_axis() {
    let (t, mut d) = fixture("release");
    common::write(
        t.path(),
        "tests/release.luau",
        r#"return scenario "a released hold lets go" {
  scene = "arena", seed = 7,
  run = function(t)
    t.input:hold("move", {1, 0})
    t:step(2)
    t.expect:field("@player", "Transform.pos.x"):eq(4)
    t.input:release("move")
    t:step(3)
    t.expect:field("@player", "Actions.move"):eq({ 0, 0 })
    t.expect:field("@player", "Transform.pos.x"):eq(4)
    t.expect:field("@player", "Transform.pos.y"):eq(0)
  end,
}"#,
    );
    let r = run(&mut d, &Options::default()).unwrap();
    assert_eq!(
        r["ok"],
        true,
        "{}",
        serde_json::to_string_pretty(&r).unwrap()
    );
}

#[test]
fn seed_specs_parse() {
    assert_eq!(parse_seeds("7").unwrap(), vec![7]);
    assert_eq!(parse_seeds("1..4").unwrap(), vec![1, 2, 3, 4]);
    assert_eq!(parse_seeds("1, 9,3").unwrap(), vec![1, 9, 3]);
    assert!(parse_seeds("5..1").is_err());
    assert!(parse_seeds("x").is_err());
}

/// §9 (phase 2 item 3): a failing run saves a replay the moment it fails,
/// which reproduces the failure in the daemon; an error saves where the
/// script stopped; a passing run and `replays: false` save nothing.
#[test]
fn a_failing_run_saves_a_replay_of_the_moment_it_failed() {
    let (t, mut d) = fixture("runner-replays");
    common::write(
        t.path(),
        "tests/fail.luau",
        r#"
return scenario "the player walks too far" {
  scene = "arena", seeds = range(1, 2),
  run = function(t)
    t.input:hold("move", {1, 0}, 100)
    t:run_until { frames = 30 }
    t.expect:field("@player", "Transform.pos.x"):lt(10)
    t:run_until { frames = 30 }
  end,
}
"#,
    );
    common::write(
        t.path(),
        "tests/stops.luau",
        r#"
return scenario "the script stops" {
  scene = "arena", seed = 7,
  run = function(t)
    t:run_until { frames = 12 }
    error("gave up here")
  end,
}
"#,
    );
    common::write(t.path(), "tests/pass.luau", SWEEP);
    let r = run(
        &mut d,
        &Options {
            replays: true,
            software: true,
            ..Default::default()
        },
    )
    .unwrap();
    let scenario = |name: &str| {
        r["scenarios"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == name)
            .unwrap()
            .clone()
    };
    let walked = scenario("the player walks too far");
    for s in walked["seeds"].as_array().unwrap() {
        assert_eq!(s["ok"], false);
        let rep = &s["replay"];
        assert_eq!(
            rep["frame"], 30,
            "saved when the expectation failed, not at the end: {rep}"
        );
        assert_eq!(rep["note"], "@player Transform.pos.x: 60 < 10 failed");
        assert_eq!(rep["about"]["scenario"], "the player walks too far");
        let dir = std::path::Path::new(rep["dir"].as_str().unwrap());
        assert!(dir.ends_with(format!("the_player_walks_too_far/seed{}", s["seed"])));
        assert!(dir.join("replay.bin").is_file() && dir.join("replay.json").is_file());
        if !rep["picture"].is_null() {
            assert!(
                rep["clip"].is_string(),
                "a clip of the seconds before it, drawn in software: {rep}"
            );
        }
    }
    let stopped = scenario("the script stops");
    let rep = &stopped["seeds"][0]["replay"];
    assert_eq!(rep["frame"], 12, "{rep}");
    assert!(rep["note"].as_str().unwrap().contains("gave up here"));
    for s in scenario("health never goes negative")["seeds"]
        .as_array()
        .unwrap()
    {
        assert!(s["replay"].is_null(), "a passing run saves nothing");
    }

    // The replay reproduces the failure in the daemon.
    let dir = walked["seeds"][0]["replay"]["dir"]
        .as_str()
        .unwrap()
        .to_string();
    let open = d.call("replay.open", &json!({ "path": dir })).unwrap();
    assert_eq!(open["frame"], 30);
    assert_eq!(open["digest"]["same"], true, "{open}");
    assert_eq!(open["against_mark"]["same"], true);
    let x = d
        .call("world.get", &json!({ "entity": "@player" }))
        .unwrap()["components"]["Transform"]["pos"][0]
        .clone();
    assert_eq!(x, 60.0);

    // Asked not to, it saves none.
    std::fs::remove_dir_all(t.path().join(".argus/replays")).unwrap();
    let r = run(
        &mut d,
        &Options {
            name: Some("walks".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(r["scenarios"][0]["seeds"][0]["replay"].is_null());
    assert!(!t.path().join(".argus/replays").exists());
}

/// Phase 3 (decided with Kevin): `days()` and `hours()` count in the
/// project's day (`[time] day`), 86,400 sim seconds when it says nothing.
#[test]
fn days_and_hours_are_the_projects() {
    let (t, mut d) = fixture("runner-days");
    common::write(
        t.path(),
        "tests/days.luau",
        r#"return scenario "a day" {
  scene = "arena", seed = 7,
  run = function(t)
    t:log(string.format("%g %g", days(1), hours(6)))
    t:step(1)
    t.expect:field("@player", "Health.cur"):gte(0)
  end,
}
"#,
    );
    let run_with = |d: &mut Local, day: Option<f64>| {
        run(
            d,
            &Options {
                day_seconds: day,
                ..Options::default()
            },
        )
        .unwrap()
    };
    let r = run_with(&mut d, Some(60.0));
    assert_eq!(
        r["ok"],
        true,
        "{}",
        serde_json::to_string_pretty(&r).unwrap()
    );
    let log = r["scenarios"][0]["seeds"][0]["log"].to_string();
    assert!(log.contains("60 15"), "{log}");
    let r = run_with(&mut d, None);
    let log = r["scenarios"][0]["seeds"][0]["log"].to_string();
    assert!(log.contains("86400 21600"), "{log}");
}

/// Phase 5 item 0: an image golden (§9, "an opt-in for rendering work,
/// with a perceptual threshold"). Recorded the first time, matched the
/// next, and a change a person would see fails with how much differs and
/// a picture of where. Drawn with the software rasteriser, so the golden is
/// the same on any machine.
#[test]
fn an_image_golden_is_recorded_matched_and_fails_with_a_picture() {
    let (t, d) = fixture("runner-image");
    common::write(
        t.path(),
        "scenes/arena/lamp.toml",
        "[Transform]\npos = [0.0, 0.0]\n\n[Sprite]\nkind = \"rect\"\nsize = [3.0, 3.0]\ncolor = \"#E8703F\"\n",
    );
    common::write(
        t.path(),
        "tests/look.luau",
        r#"
return scenario "the lamp looks as it did" {
  scene = "arena", seed = 7,
  run = function(t)
    t:run_until { frames = 2 }
    t.expect:image("lamp", { width = 160, height = 90 })
  end,
}
"#,
    );
    drop(d);
    let mut d = Local(common::open(t.path()), t.path().to_path_buf());
    let image = |r: &Json| r["scenarios"][0]["seeds"][0]["expectations"][0].clone();
    let golden = t.path().join(format!(
        "tests/goldens/the_lamp_looks_as_it_did/lamp.seed7.{}.png",
        std::env::consts::OS
    ));
    let r = run(&mut d, &Options::default()).unwrap();
    assert!(image(&r)["recorded"] == true, "{}", image(&r));
    assert!(golden.is_file());
    let r = run(&mut d, &Options::default()).unwrap();
    let e = image(&r);
    assert_eq!(e["ok"], true, "{e}");
    assert_eq!(e["differing"], 0, "the same picture: {e}");
    // The lamp another colour: a person would see it.
    let r = run(
        &mut d,
        &Options {
            set: vec![json!({ "entity": "@lamp", "path": "Sprite.color", "value": "#3FA0E8" })],
            ..Default::default()
        },
    )
    .unwrap();
    let e = image(&r);
    assert_eq!(e["ok"], false, "{e}");
    let msg = e["message"].as_str().unwrap();
    assert!(
        msg.contains("of its pixels differ from tests/goldens/"),
        "{msg}"
    );
    assert!(
        std::path::Path::new(e["diff"].as_str().unwrap()).is_file(),
        "{e}"
    );
    assert!(
        std::path::Path::new(e["actual"].as_str().unwrap()).is_file(),
        "{e}"
    );
    // And update records the new look.
    let r = run(
        &mut d,
        &Options {
            update_goldens: true,
            set: vec![json!({ "entity": "@lamp", "path": "Sprite.color", "value": "#3FA0E8" })],
            ..Default::default()
        },
    )
    .unwrap();
    assert!(image(&r)["recorded"] == true, "{}", image(&r));
}

/// Each system keeps its own picture golden: with only another system's,
/// the picture is held to the share rasterisers differ by at edges
/// (`other_max`, 1%) and says whose it was, and an update records this
/// system's own (Todd's Mac test: a Mac's GPU against Windows' WARP).
#[test]
fn an_image_golden_from_another_system_is_held_to_the_share_between_systems() {
    let (t, d) = fixture("runner-image-system");
    common::write(
        t.path(),
        "scenes/arena/lamp.toml",
        "[Transform]\npos = [0.0, 0.0]\n\n[Sprite]\nkind = \"rect\"\nsize = [3.0, 3.0]\ncolor = \"#E8703F\"\n",
    );
    common::write(
        t.path(),
        "tests/look.luau",
        r#"
return scenario "the lamp looks as it did" {
  scene = "arena", seed = 7,
  run = function(t)
    t:run_until { frames = 2 }
    t.expect:image("lamp", { width = 160, height = 90 })
  end,
}
"#,
    );
    drop(d);
    let mut d = Local(common::open(t.path()), t.path().to_path_buf());
    let image = |r: &Json| r["scenarios"][0]["seeds"][0]["expectations"][0].clone();
    let dir = t.path().join("tests/goldens/the_lamp_looks_as_it_did");
    let own = dir.join(format!("lamp.seed7.{}.png", std::env::consts::OS));
    let other = if std::env::consts::OS == "linux" {
        "macos"
    } else {
        "linux"
    };
    // This system's picture, moved to look like another system's.
    let r = run(&mut d, &Options::default()).unwrap();
    assert!(image(&r)["recorded"] == true, "{}", image(&r));
    std::fs::rename(&own, dir.join(format!("lamp.seed7.{other}.png"))).unwrap();
    let r = run(&mut d, &Options::default()).unwrap();
    let e = image(&r);
    assert_eq!(e["ok"], true, "{e}");
    assert_eq!(e["system"], other, "{e}");
    let msg = e["message"].as_str().unwrap();
    assert!(
        msg.contains(&format!(
            "recorded on {other}, 1.00% allowed between systems"
        )),
        "{msg}"
    );
    assert!(!own.exists(), "a run without update writes no golden");
    // Update records this system's own, and the next run holds to it.
    let r = run(
        &mut d,
        &Options {
            update_goldens: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(image(&r)["recorded"] == true, "{}", image(&r));
    assert!(own.is_file());
    let r = run(&mut d, &Options::default()).unwrap();
    let e = image(&r);
    assert_eq!(e["ok"], true, "{e}");
    assert!(e["system"].is_null(), "{e}");
}

/// A key `t:run_until` does not take is an error that names it: a misspelt
/// `max_frames` would run the default 100,000 ticks. `speed = "max"` (the
/// phase 0 design's, in Outpost's food scenario) is taken.
#[test]
fn run_until_refuses_a_key_it_does_not_take() {
    let (t, mut d) = fixture("runner-until-keys");
    common::write(
        t.path(),
        "tests/keys.luau",
        r#"return scenario "keys" {
  scene = "arena", seed = 7,
  run = function(t)
    t:run_until { frames = 2, speed = "max" }
    t:run_until { event = "never", max_frame = 5 }
  end,
}
"#,
    );
    let r = run(&mut d, &Options::default()).unwrap();
    assert_eq!(r["ok"], false, "{r}");
    let seed = &r["scenarios"][0]["seeds"][0];
    assert!(
        seed.to_string().contains("run_until has no key max_frame"),
        "{seed}"
    );
    assert_eq!(
        seed["tick"], 2,
        "the first call ran, the second did not: {seed}"
    );
}

/// The host channel (item 0 and 1): a scenario sends text as the page would,
/// a script answers with q:send, and the answer is an event a scenario can
/// check. The Bring Your Own Game Master game's scripted GM works this way.
#[test]
fn a_scenario_sends_a_message_and_checks_the_scripts_answer() {
    let (t, mut d) = fixture("message");
    common::write(
        t.path(),
        "systems/echo.luau",
        r#"return system "sim.echo" {
  stage = "sim",
  doc = "Sends each host message back on the echo channel, longer.",
  run = function(q)
    for _, ev in ipairs(q:events("host.message")) do
      q:send("echo", ev.channel .. ":" .. ev.text .. "!")
    end
  end,
}"#,
    );
    common::write(
        t.path(),
        "tests/message.luau",
        r#"return scenario "a message comes back" {
  scene = "arena", seed = 7,
  run = function(t)
    t.input:message("gm.ruling", "tie the door")
    t:step(1)
    t.expect:events("host.message", { channel = "gm.ruling", text = "tie the door" }):count():eq(1)
    t.expect:events("host.send", { channel = "echo", text = "gm.ruling:tie the door!" }):count():eq(1)
  end,
}"#,
    );
    let r = run(&mut d, &Options::default()).unwrap();
    assert_eq!(
        r["ok"],
        true,
        "{}",
        serde_json::to_string_pretty(&r).unwrap()
    );
}

/// Issue 12: a game system a fault switched off fails the run, naming it,
/// unless the scenario says it expects one (`faults = "allow"`). Before, a
/// run passed while the game played on with its turn system off.
#[test]
fn a_fault_fails_the_run_unless_the_scenario_allows_it() {
    let (t, mut d) = fixture("runner-faults");
    common::write(
        t.path(),
        "systems/boom.luau",
        r#"return system "sim.boom" {
  stage = "sim",
  doc = "Fails on its third tick.",
  run = function(q)
    if q.tick == 3 then error("the third tick") end
  end,
}"#,
    );
    for (file, allow) in [("fails.luau", ""), ("allows.luau", "faults = \"allow\",")] {
        common::write(
            t.path(),
            &format!("tests/{file}"),
            &format!(
                "return scenario \"{file}\" {{\n  scene = \"arena\", seed = 7, {allow}\n  run = function(t)\n    t:step(10)\n  end,\n}}"
            ),
        );
    }
    let r = run(&mut d, &Options::default()).unwrap();
    let by = |file: &str| {
        r["scenarios"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == file)
            .unwrap_or_else(|| panic!("{file}: {r}"))["seeds"][0]
            .clone()
    };
    let failed = by("fails.luau");
    assert_eq!(failed["ok"], false, "{failed}");
    let said = failed["error"].as_str().unwrap();
    assert!(
        said.contains("sim.boom faulted and was switched off") && said.contains("the third tick"),
        "{said}"
    );
    assert!(
        said.contains("systems/boom.luau"),
        "it names the file: {said}"
    );
    assert_eq!(by("allows.luau")["ok"], true, "{r}");
}

/// Issue 25: `:last()` and `:all()` on a run's events, and a scenario that
/// starts from a replay (here the one a failing scenario saved) where
/// `run` takes over, at its frame with its seed.
#[test]
fn events_have_last_and_all_and_a_scenario_starts_from_a_replay() {
    let (t, mut d) = fixture("runner-replay");
    common::write(
        t.path(),
        "systems/beat.luau",
        r#"return system "sim.beat" {
  stage = "sim",
  doc = "A beat each tick, with its number.",
  run = function(q) q:emit("beat", { n = q.tick }) end,
}"#,
    );
    common::write(
        t.path(),
        "tests/beats.luau",
        r#"return {
  scenario "beats are counted" {
    scene = "arena", seed = 7,
    run = function(t)
      t:step(5)
      local ev = t.expect:events("beat")
      assert(#ev:all() == 5, "all: " .. #ev:all())
      assert(ev:last().fields.n == 4, "last: " .. tostring(ev:last().fields.n))
      assert(ev:first().fields.n == 0)
    end,
  },
  scenario "stops on beat nine" {
    scene = "arena", seed = 7,
    run = function(t)
      t:step(10)
      assert(false, "stops here") -- fails, so a replay is saved here
    end,
  },
}"#,
    );
    let r = run(
        &mut d,
        &Options {
            replays: true,
            ..Default::default()
        },
    )
    .unwrap();
    let by = |r: &Json, name: &str| {
        r["scenarios"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == name)
            .unwrap()["seeds"][0]
            .clone()
    };
    assert_eq!(by(&r, "beats are counted")["ok"], true, "{r}");
    assert_eq!(by(&r, "stops on beat nine")["ok"], false);
    let saved = t
        .path()
        .join(".argus/replays/stops_on_beat_nine/seed7/replay.bin");
    assert!(saved.exists(), "{saved:?}");

    common::write(
        t.path(),
        "tests/from_replay.luau",
        r#"return scenario "from the replay" {
  replay = ".argus/replays/stops_on_beat_nine/seed7",
  run = function(t)
    assert(t:status().tick == 10, "at the replay's frame: " .. t:status().tick)
    t:step(2)
    local n = t.expect:events("beat"):last().fields.n
    assert(n == 11, "two more beats: " .. tostring(n))
  end,
}"#,
    );
    let r = run(
        &mut d,
        &Options {
            name: Some("from the replay".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let s = by(&r, "from the replay");
    assert_eq!(s["ok"], true, "{s}");
    assert_eq!(s["seed"], 7, "the replay's seed: {s}");
}

/// Issue 29: a --file the runner does not read, or a name no scenario has,
/// is an error naming what there is; it reported a pass of nothing.
#[test]
fn a_file_or_name_that_selects_nothing_is_an_error() {
    let (t, mut d) = fixture("runner-select");
    common::write(
        t.path(),
        "tests/walk.luau",
        "return scenario \"walks\" { scene = \"arena\", seed = 7, run = function(t) t:step(1) end }",
    );
    common::write(
        t.path(),
        "docs/slow.luau",
        "return scenario \"slow\" { scene = \"arena\", seed = 7, run = function(t) t:step(1) end }",
    );
    let pick = |file: Option<&str>, name: Option<&str>| Options {
        file: file.map(str::to_string),
        name: name.map(str::to_string),
        ..Default::default()
    };
    let e = run(&mut d, &pick(Some("docs/slow.luau"), None)).unwrap_err();
    assert!(
        e.contains("only .luau files directly in tests/") && e.contains("is not one"),
        "{e}"
    );
    assert!(e.contains("tests/walk.luau"), "it names what there is: {e}");
    let e = run(&mut d, &pick(Some("tests/gone.luau"), None)).unwrap_err();
    assert!(e.contains("does not exist"), "{e}");
    let e = run(&mut d, &pick(None, Some("nothing like it"))).unwrap_err();
    assert!(e.contains("no scenario has \"nothing like it\""), "{e}");
    // The file by its short name, or as a path under the project, runs.
    for f in ["walk.luau", "./tests/walk.luau"] {
        let r = run(&mut d, &pick(Some(f), None)).unwrap();
        assert_eq!(r["total_runs"], 1, "{f}: {r}");
    }
    let abs = t.path().join("tests").join("walk.luau");
    let r = run(&mut d, &pick(Some(&abs.to_string_lossy()), None)).unwrap();
    assert_eq!(r["total_runs"], 1, "{abs:?}: {r}");
}

/// Issue 28: a scenario's own code that never ends is cut off where it is,
/// past its max_ms or when the daemon is asked to stop, and the runs after
/// it still run (or, stopped, none do); a pcall does not keep it going.
#[test]
fn a_scenario_that_never_ends_is_cut_off_by_max_ms_or_a_stop() {
    let (t, mut d) = fixture("runner-forever");
    common::write(
        t.path(),
        "tests/forever.luau",
        r#"return {
  scenario "a loop that never ends" {
    scene = "arena", seed = 7, max_ms = 300,
    run = function(t)
      t:step(1)
      while true do pcall(function() local n = 0 for i = 1, 100 do n += i end end) end
    end,
  },
  scenario "after it" {
    scene = "arena", seed = 7,
    run = function(t) t:step(1) end,
  },
}"#,
    );
    let started = std::time::Instant::now();
    let r = run(&mut d, &Options::default()).unwrap();
    assert!(
        started.elapsed().as_secs() < 30,
        "cut off promptly: {:?}",
        started.elapsed()
    );
    let runs: Vec<&Json> = r["scenarios"].as_array().unwrap().iter().collect();
    let forever = &runs
        .iter()
        .find(|s| s["name"] == "a loop that never ends")
        .unwrap()["seeds"][0];
    assert_eq!(forever["ok"], false, "{forever}");
    let said = forever["error"].as_str().unwrap();
    assert!(
        said.contains("cut off") && said.contains("max_ms, 300 ms"),
        "{said}"
    );
    assert!(
        said.contains("tests/forever.luau:6"),
        "where it was: {said}"
    );
    assert_eq!(
        runs.iter().find(|s| s["name"] == "after it").unwrap()["ok"],
        true,
        "{r}"
    );
    assert_eq!(r["cut_off"][0]["scenario"], "a loop that never ends", "{r}");

    // The daemon asked to stop, from another thread, while a run loops with
    // no max_ms of its own: it stops, and nothing after it runs.
    common::write(
        t.path(),
        "tests/forever.luau",
        r#"return {
  scenario "a loop that never ends" {
    scene = "arena", seed = 7,
    run = function(t) while true do end end,
  },
  scenario "after it" {
    scene = "arena", seed = 7,
    run = function(t) t:step(1) end,
  },
}"#,
    );
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let s2 = stop.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        s2.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let r = run(
        &mut d,
        &Options {
            stop: Some(stop),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r["stopped"], true, "{r}");
    assert_eq!(r["ok"], false);
    assert_eq!(r["total_runs"], 1, "nothing ran after the stop: {r}");
    assert!(
        r["cut_off"][0]["why"]
            .as_str()
            .unwrap()
            .contains("daemon stop"),
        "{r}"
    );
}

/// Issue 24: a scenario's `require` looks in `tests/lib/` first, then in
/// the game's `lib/`; a module is evaluated once and frozen, and the files
/// in `tests/lib/` are not scenarios.
#[test]
fn a_scenario_requires_its_helpers_and_the_games_modules() {
    let (t, mut d) = fixture("runner-require");
    common::write(
        t.path(),
        "lib/rules.luau",
        "return { answer = 42, names = { \"bat\" } }",
    );
    common::write(t.path(), "lib/which.luau", "return { from = \"lib\" }");
    common::write(
        t.path(),
        "lib/spin.luau",
        "return { spin = function() while true do pcall(function() end) end end }",
    );
    common::write(
        t.path(),
        "tests/lib/which.luau",
        "return { from = \"tests/lib\" }",
    );
    common::write(
        t.path(),
        "tests/lib/helpers.luau",
        r#"local rules = require("rules")
__made = (__made or 0) + 1
return {
  answer = rules.answer,
  steps = function(t, n) t:step(n) return t:status().tick end,
}"#,
    );
    common::write(
        t.path(),
        "tests/lib/a.luau",
        "return { b = require(\"b\") }",
    );
    common::write(
        t.path(),
        "tests/lib/b.luau",
        "return { a = require(\"a\") }",
    );
    common::write(
        t.path(),
        "tests/uses.luau",
        r#"local helpers = require("helpers")
local rules = require("rules")
return {
  scenario "helpers from tests/lib, rules from the game's lib" {
    scene = "arena", seed = 7,
    run = function(t)
      assert(helpers.answer == 42 and rules.names[1] == "bat")
      assert(require("which").from == "tests/lib", "tests/lib/ first")
      assert(require("helpers") == helpers and __made == 1, "evaluated once")
      assert(helpers.steps(t, 3) == 3)
      assert(not pcall(function() rules.names[1] = "rat" end), "frozen")
      local ok, why = pcall(require, "nope")
      assert(not ok and string.find(tostring(why), "there is no tests/lib/nope.luau or lib/nope.luau", 1, true), tostring(why))
      local ok2, why2 = pcall(require, "a")
      assert(not ok2 and string.find(tostring(why2), "in a circle: tests/lib/a.luau, which requires tests/lib/b.luau, which requires tests/lib/a.luau", 1, true), tostring(why2))
    end,
  },
  scenario "cut off in a game module" {
    scene = "arena", seed = 7, max_ms = 300,
    run = function(t) require("spin").spin() end,
  },
}"#,
    );
    let listed = list(t.path(), None).unwrap();
    assert_eq!(listed["load_errors"], json!([]), "{listed}");
    let files: Vec<&str> = listed["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["file"].as_str().unwrap())
        .collect();
    assert!(files.iter().all(|f| *f == "tests/uses.luau"), "{listed}");
    let r = run(&mut d, &Options::default()).unwrap();
    let runs = r["scenarios"].as_array().unwrap();
    let first = runs
        .iter()
        .find(|s| s["name"] == "helpers from tests/lib, rules from the game's lib")
        .unwrap();
    assert_eq!(first["ok"], true, "{first}");
    let spun = &runs
        .iter()
        .find(|s| s["name"] == "cut off in a game module")
        .unwrap()["seeds"][0];
    let said = spun["error"].as_str().unwrap();
    assert!(
        said.contains("cut off") && said.contains("lib/spin.luau:1"),
        "{said}"
    );
}
