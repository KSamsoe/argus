//! §17's targets beside what is measured (phase 4 item 6): every row is
//! there with its gate; the rows for every game are measured on any
//! project; a reference game's own rows are measured when it is open; a
//! row over its bound names the system to look at (§5: "a perf regression
//! is a query result with a system name on it"); and the protocol's calls
//! are timed.

mod common;

use argus_daemon::Daemon;
use common::*;
use serde_json::{json, Value as Json};

fn row<'a>(t: &'a Json, measure: &str) -> &'a Json {
    t["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["measure"] == measure)
        .unwrap_or_else(|| panic!("no row {measure}"))
}

fn call(d: &mut Daemon, method: &str, params: Json) -> Json {
    match d.call(1, method, &params) {
        Ok(v) => v,
        Err(e) => panic!("{method} failed: {e}"),
    }
}

#[test]
fn every_rows_target_is_there_and_the_rows_for_every_game_are_measured() {
    let t = temp("prof-targets-all");
    project(t.path());
    let mut d = open(t.path());
    let before = ok(&mut d, "prof.targets", json!({}));
    assert_eq!(before["rows"].as_array().unwrap().len(), 23);
    assert!(before["game"].is_null(), "the fixture is no reference game");
    assert_eq!(
        before["build"],
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    // A game's own row is not measured on another game.
    let tick = row(&before, "Sim tick, p95");
    assert_eq!(
        (tick["game"].clone(), tick["live"].clone()),
        (json!("swarm"), json!(false))
    );
    assert!(tick["value"].is_null());
    // Gated on Swarm itself since its chase is a Rust plugin (phase 5 item 6).
    assert!(
        tick["gate"]
            .as_str()
            .unwrap()
            .starts_with("argus-daemon perf_swarm_stress_tick_under_6_ms"),
        "{}",
        tick["gate"]
    );
    // A row only a test measures says which.
    let post = row(&before, "Post stack, 5 passes at 1080p");
    assert_eq!(post["live"], false);
    assert_eq!(
        post["gate"],
        "argus-render perf_post_stack_five_passes_at_1080p"
    );
    let seek = row(&before, "Seek to any point in last 5 min");
    assert_eq!(seek["live"], true);
    assert!(seek["value"].is_null(), "no seek yet: {seek}");
    assert_eq!(seek["text"], "no sim.seek yet");

    call(&mut d, "sim.step", json!({ "n": 180 }));
    call(&mut d, "sim.seek", json!({ "frame": 70 }));
    call(&mut d, "script.reload", json!({}));
    call(
        &mut d,
        "world.query",
        json!({ "where": { "has": ["Health"] } }),
    );
    let after = ok(&mut d, "prof.targets", json!({}));
    for (measure, bound) in [
        ("Seek to any point in last 5 min", 500.0),
        ("Luau hot reload", 100.0),
        ("world.query over 100k entities", 2.0),
        ("Full snapshot at 100k entities", 5.0),
    ] {
        let r = row(&after, measure);
        let v = r["value"]
            .as_f64()
            .unwrap_or_else(|| panic!("{measure}: {r}"));
        assert_eq!(r["ok"], v < bound, "{r}");
        assert_eq!(r["unit"], "ms");
    }
    let seek = row(&after, "Seek to any point in last 5 min");
    assert!(
        seek["text"].as_str().unwrap().contains("the last sim.seek"),
        "{seek}"
    );
    let snap = row(&after, "Full snapshot at 100k entities");
    assert!(
        snap["context"].as_str().unwrap().ends_with(" entities"),
        "{snap}"
    );

    // The protocol's calls, timed, the latest first.
    let calls = ok(&mut d, "prof.calls", json!({}));
    let rows = calls["rows"].as_array().unwrap();
    assert_eq!(rows[0][0], "prof.targets");
    let seek = rows.iter().find(|r| r[0] == "sim.seek").unwrap();
    assert_eq!(seek[1], 1);
    // The tick series, oldest first, for drawing.
    let s = ok(
        &mut d,
        "prof.systems",
        json!({ "last": 50, "series": true }),
    );
    let ticks = s["ticks"].as_array().unwrap();
    assert_eq!(ticks.len(), 50);
    assert!(ticks[0][0].as_u64() < ticks[49][0].as_u64());
    assert!(ok(&mut d, "prof.systems", json!({}))["ticks"].is_null());
    assert!(ok(&mut d, "sim.ring", json!({}))["capture_ms"]["last"].is_number());
}

#[test]
fn a_reference_games_rows_are_measured_and_one_over_names_its_system() {
    let t = temp("prof-targets-outpost");
    project(t.path());
    // The fixture, named as Outpost, with a system too slow for its tick.
    let toml = read(t.path(), "argus.toml").replace("name = \"fixture\"", "name = \"outpost\"");
    write(t.path(), "argus.toml", &toml);
    write(
        t.path(),
        "systems/slow.luau",
        r#"system "sim.slow" {
  stage = "sim",
  doc = "Counts to a large number every tick.",
  run = function(q)
    local x = 0
    for i = 1, 3000000 do x = x + i % 7 end
  end,
}
"#,
    );
    let mut d = open(t.path());
    call(&mut d, "sim.step", json!({ "n": 20 }));
    let r = ok(&mut d, "prof.targets", json!({ "last": 20 }));
    assert_eq!(r["game"], "outpost");
    let tick = row(&r, "Sim tick at 10× speed, 200 colonists, 20k items");
    assert_eq!(tick["live"], true);
    assert!(tick["value"].as_f64().unwrap() > 1.6, "{tick}");
    assert_eq!(tick["ok"], false);
    assert_eq!(tick["system"], "sim.slow", "the system to look at: {tick}");
    assert_eq!(r["heaviest"]["name"], "sim.slow");
    assert!(r["over"].as_u64().unwrap() >= 1);
    // Another game's rows stay unmeasured.
    assert_eq!(row(&r, "Room stream-in")["live"], false);
    // Outpost's other rows, as they are measured.
    // The layout runs with no UI too: measured, and well under.
    let ui = row(&r, "UI tree, 2,000 nodes with text");
    assert_eq!(ui["system"], "ui.layout");
    assert!(
        ui["text"].as_str().unwrap().contains(" p95 over 20 ticks"),
        "{ui}"
    );
    assert_eq!(ui["ok"], true);
    assert_eq!(ui["context"], "0 with UiNode");
    assert_eq!(
        row(&r, "Headless fast-forward")["text"],
        "no sim.run_until yet"
    );
    call(&mut d, "sim.run_until", json!({ "ticks": 5 }));
    call(&mut d, "sim.save", json!({ "name": "t" }));
    call(&mut d, "sim.load", json!({ "name": "t" }));
    let r = ok(&mut d, "prof.targets", json!({}));
    let ff = row(&r, "Headless fast-forward");
    assert_eq!(ff["unit"], "ticks/s");
    assert!(ff["value"].as_f64().unwrap() > 0.0, "{ff}");
    assert_eq!(ff["ok"], ff["value"].as_f64().unwrap() > 2000.0);
    let sl = row(&r, "Save / load at 8 h of play");
    let text = sl["text"].as_str().unwrap();
    assert!(
        text.starts_with("save ") && text.contains(", load "),
        "{sl}"
    );
    assert!(sl["ok"].is_boolean());
}
