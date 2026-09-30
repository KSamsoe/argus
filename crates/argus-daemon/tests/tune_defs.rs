//! Phase 4 item 3, the daemon's side of the tuning panel and the sweep.
//! §10: "In Outpost the tune fields are job priorities, consumption rates,
//! and growth curves": components of named entities, the job kinds made at
//! run time, and the def tables' rows (a meal's food). A slider moves any of
//! them live and logged; the session's end commits them, the def rows to
//! their table's file; `sim.set` puts a value in the live world with no
//! file and no log; and `test.run` tries candidates without writing them.

mod common;

use argus_daemon::Daemon;
use common::*;
use serde_json::{json, Value as Json};
use std::path::Path;

const ME: u64 = 2;

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let name = e.file_name();
        let n = name.to_string_lossy();
        if n.starts_with('.') || n == "dist" {
            continue;
        }
        let (a, b) = (e.path(), to.join(&name));
        if a.is_dir() {
            copy(&a, &b);
        } else {
            std::fs::copy(&a, &b).unwrap();
        }
    }
}

fn me(d: &mut Daemon, method: &str, params: Json) -> Json {
    d.call(ME, method, &params)
        .unwrap_or_else(|e| panic!("{method}: {} ({})", e.message, e.hint))
}

fn meal_food(d: &mut Daemon) -> f64 {
    let t = ok(
        d,
        "world.defs",
        json!({ "table": "items", "where": { "id": "meal" } }),
    );
    let i = t["columns"]
        .as_array()
        .unwrap()
        .iter()
        .position(|c| c == "food")
        .expect("a food column");
    t["rows"][0][i].as_f64().unwrap()
}

fn outpost(tag: &str) -> (Temp, Daemon) {
    let t = temp(tag);
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/outpost"),
        t.path(),
    );
    let mut d = open(t.path());
    me(&mut d, "session.identify", json!({ "author": "human" }));
    ok(&mut d, "sim.step", json!({ "n": 2 }));
    (t, d)
}

#[test]
fn the_tuning_panel_reaches_def_rows_and_job_kinds() {
    let (t, mut d) = outpost("tune-defs");
    let f = ok(&mut d, "tune.fields", json!({}));
    let rows = f["rows"].as_array().unwrap().clone();
    let has = |pred: &dyn Fn(&Json) -> bool| rows.iter().any(pred);
    assert!(
        has(&|r| r["entity"] == "@colony" && r["path"] == "Colony.hunger_per_day"),
        "the colony's own"
    );
    assert!(
        has(&|r| r["def"] == "item:meal" && r["path"] == "food" && r["value"] == 1.0),
        "a meal's food"
    );
    assert!(
        has(&|r| r["def"] == "job:cook" && r["path"] == "priority"),
        "a job's priority in its table"
    );
    let kinds: Vec<&Json> = rows
        .iter()
        .filter(|r| r["runtime"] == true && r["path"] == "JobKind.priority")
        .collect();
    assert!(
        kinds.len() >= 5,
        "the job kinds made at run time, one by one: {kinds:?}"
    );
    assert!(
        kinds.iter().any(|r| r["label"] == "JobKind cook"),
        "{kinds:?}"
    );

    // A slider on a def row: live, logged, and the tables read again.
    me(
        &mut d,
        "tune.set",
        json!({ "def": "item:meal", "path": "food", "value": 0.8 }),
    );
    assert!((meal_food(&mut d) - 0.8).abs() < 1e-6);
    me(
        &mut d,
        "tune.set",
        json!({ "def": "item:meal", "path": "food", "value": 0.7 }),
    );
    let text = std::fs::read_to_string(t.path().join("defs/items.toml")).unwrap();
    assert!(
        text.contains("food = 1.0"),
        "live only until the session ends: {text}"
    );
    let log = ok(&mut d, "tune.log", json!({}));
    let story = log["story"].to_string();
    assert!(
        story.contains("item:meal food 1→0.8"),
        "the search path: {story}"
    );
    assert!(story.contains("→0.7"), "{story}");
    // A job kind's priority, the runtime entity itself.
    let cook = kinds.iter().find(|r| r["label"] == "JobKind cook").unwrap()["entity"].clone();
    me(
        &mut d,
        "tune.set",
        json!({ "entity": cook, "path": "JobKind.priority", "value": 9 }),
    );
    // Out of its range: refused, with the range.
    let e = d
        .call(
            ME,
            "tune.set",
            &json!({ "def": "item:meal", "path": "food", "value": 11 }),
        )
        .unwrap_err();
    assert!(e.message.contains("[0, 10]"), "{}", e.message);

    // The session's end: the def row into its file, comments and all.
    let end = me(&mut d, "tune.end", json!({}));
    assert!(end["commit"]["id"].is_u64(), "{end}");
    let text = std::fs::read_to_string(t.path().join("defs/items.toml")).unwrap();
    assert!(text.contains("food = 0.7"), "{text}");
    assert!(
        text.contains("# Goods (section 4's data tables)"),
        "the comments stay: {text}"
    );
    assert!(
        text.contains("food = 0.4"),
        "the other rows as they were: {text}"
    );
    let feed = ok(&mut d, "edit.feed", json!({ "limit": 50 }));
    let last = feed["rows"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["author"], "human");
    assert_eq!(last["command"], "argus tune end");
    // One undo: back to where the session started, in memory and the file.
    me(&mut d, "edit.undo", json!({ "author": "human" }));
    assert!((meal_food(&mut d) - 1.0).abs() < 1e-6);
    let text = std::fs::read_to_string(t.path().join("defs/items.toml")).unwrap();
    assert!(text.contains("food = 1.0"), "{text}");

    // edit.def: a def row in a transaction, as the agent's edit tool has it.
    let tx = me(
        &mut d,
        "edit.begin",
        json!({ "author": "human", "note": "cheaper cooking" }),
    )["tx"]
        .clone();
    me(
        &mut d,
        "edit.def",
        json!({ "tx": tx, "def": "recipe:meal", "path": "work", "value": 4 }),
    );
    me(&mut d, "edit.commit", json!({ "tx": tx }));
    let feed = ok(&mut d, "edit.feed", json!({ "limit": 50 }));
    let last = feed["rows"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(
        last["command"],
        "argus edit author=human note='cheaper cooking' ops='[{def: recipe:meal, op: def, path: work, value: 4}]'"
    );
    assert!(std::fs::read_to_string(t.path().join("defs/recipes.toml"))
        .unwrap()
        .contains("work = 4"));

    // sim.set: live, no file, no log; a command in the feed.
    let before = std::fs::read_to_string(t.path().join("defs/items.toml")).unwrap();
    let rows_before = ok(&mut d, "tune.log", json!({}))["total"].clone();
    me(
        &mut d,
        "sim.set",
        json!({ "def": "item:meal", "path": "food", "value": 0.5 }),
    );
    me(
        &mut d,
        "sim.set",
        json!({ "entity": "@colony", "path": "Colony.hunger_per_day", "value": 1.5 }),
    );
    assert!((meal_food(&mut d) - 0.5).abs() < 1e-6);
    assert_eq!(
        std::fs::read_to_string(t.path().join("defs/items.toml")).unwrap(),
        before
    );
    assert_eq!(
        ok(&mut d, "tune.log", json!({}))["total"],
        rows_before,
        "not logged"
    );
    let feed = ok(&mut d, "edit.feed", json!({ "limit": 50 }));
    let last = feed["rows"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(
        last["command"],
        "argus sim set entity=@colony path=Colony.hunger_per_day value=1.5"
    );
}

#[test]
fn a_sweep_tries_each_candidate_without_writing_it() {
    let (t, mut d) = outpost("sweep");
    // A scenario that holds as long as a colonist needs at most a meal
    // and a half a day, and a meal is worth at least half a meal.
    write(
        t.path(),
        "tests/zz_sweep.luau",
        r#"scenario "a day's food is within reach" {
  scene = "valley",
  seeds = { 1, 2 },
  run = function(t)
    local c = t:call("world.get", { entity = "@colony" })
    local need = c.components.Colony.hunger_per_day
    if need > 1.5 then error("a colonist needs " .. need .. " meals a day", 0) end
    local d = t:call("world.defs", { table = "items", where = { id = "meal" } })
    local food
    for i, col in ipairs(d.columns) do if col == "food" then food = d.rows[1][i] end end
    if food < 0.5 then error("a meal is worth " .. food, 0) end
    t:step(5)
  end,
}
"#,
    );
    let r = me(
        &mut d,
        "test.run",
        json!({
            "file": "tests/zz_sweep.luau",
            "sweep": { "entity": "@colony", "path": "Colony.hunger_per_day", "values": [1.0, 1.4, 2.0] },
        }),
    );
    let c = r["candidates"].as_array().unwrap();
    let verdicts: Vec<(f64, bool, u64)> = c
        .iter()
        .map(|x| {
            (
                x["value"].as_f64().unwrap(),
                x["ok"].as_bool().unwrap(),
                x["failed"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        verdicts,
        vec![(1.0, true, 0), (1.4, true, 0), (2.0, false, 2)],
        "{r}"
    );
    assert!(
        c[2]["failing"][0]["error"]
            .as_str()
            .unwrap()
            .contains("needs 2"),
        "{r}"
    );
    // A def row's candidates, with another override held for every run.
    let r = me(
        &mut d,
        "test.run",
        json!({
            "file": "tests/zz_sweep.luau",
            "set": [{ "entity": "@colony", "path": "Colony.hunger_per_day", "value": 1.2 }],
            "sweep": { "def": "item:meal", "path": "food", "values": [0.3, 1.0] },
        }),
    );
    let oks: Vec<bool> = r["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["ok"].as_bool().unwrap())
        .collect();
    assert_eq!(oks, vec![false, true], "{r}");
    // Nothing was written, and the session's world is as it was.
    assert!(std::fs::read_to_string(t.path().join("defs/items.toml"))
        .unwrap()
        .contains("food = 1.0"));
    assert!(
        (meal_food(&mut d) - 1.0).abs() < 1e-6,
        "the scene loaded again after the tests"
    );
    let c = ok(&mut d, "world.get", json!({ "entity": "@colony" }));
    assert_eq!(c["components"]["Colony"]["hunger_per_day"], 1.0);
    // A bad target is refused before anything runs.
    let e = d
        .call(
            ME,
            "test.run",
            &json!({ "sweep": { "path": "Colony.hunger_per_day", "values": [1] } }),
        )
        .unwrap_err();
    assert!(e.message.contains("entity"), "{}", e.message);
}
