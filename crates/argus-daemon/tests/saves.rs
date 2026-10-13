//! Save games in the daemon (§6, phase 3 item 0): `sim.save` and
//! `sim.load` through the protocol, a game asking for its own saves with
//! `q:save` and `q:load`, and a save written by one version of the game's
//! components loaded by the next through its migration.

mod common;

use argus_daemon::{codes, Daemon};
use common::*;
use serde_json::{json, Value as Json};
use std::path::Path;

const NEEDS_V1: &str = r#"return component "Needs" {
  doc = "What a colonist lacks",
  food = field.u8 { default = 50, doc = "Food left, 0 to 100" },
  mood = field.str { doc = "How they feel" },
}
"#;

/// Version 2: food is hunger now, a fraction that rises; mood is gone;
/// rest is new, and old saves start rested.
const NEEDS_V2: &str = r#"return component "Needs" {
  doc = "What a colonist lacks",
  version = 2,
  hunger = field.f32 { doc = "0 fed, 1 starving" },
  rest = field.f32 { doc = "0 tired, 1 rested" },
  migrate = {
    [2] = function(row)
      return { hunger = 1 - row.food / 100, rest = 1 }
    end,
  },
}
"#;

const COLONY_V1: &str = r#"system "sim.colony" {
  stage = "sim",
  on_load = true,
  reads = { "Needs", "Actions" },
  writes = { "Needs" },
  doc = "Founds a colony of five at the load; they eat a unit of food every ten ticks. The game saves itself at tick 30, and fire loads the manual save.",
  run = function(q)
    local n = 0
    for _ in q:each("Needs") do n += 1 end
    if n == 0 then
      for i = 1, 5 do
        local c = q:spawn({ "Needs" })
        local needs = c:get("Needs")
        needs.food = 40 + i
        needs.mood = "colonist " .. i
      end
      return
    end
    for _, needs in q:each("Needs") do
      if q.tick % 10 == 0 and needs.food > 0 then needs.food -= 1 end
    end
    if q.tick == 30 then q:save("auto") end
    for _, a in q:each("Actions") do
      if a.fire_pressed then q:load("manual") end
    end
  end,
}
"#;

const COLONY_V2: &str = r#"system "sim.colony" {
  stage = "sim",
  reads = { "Needs" },
  writes = { "Needs" },
  doc = "They grow hungrier every tick.",
  run = function(q)
    for _, needs in q:each("Needs") do needs.hunger += 0.001 end
  end,
}
"#;

fn colony(root: &Path) {
    project(root);
    write(root, "components/needs.luau", NEEDS_V1);
    write(root, "systems/colony.luau", COLONY_V1);
}

fn needs(d: &mut Daemon, field: &str) -> Vec<Json> {
    let q = ok(
        d,
        "world.query",
        json!({ "select": ["id", format!("Needs.{field}")], "where": { "has": ["Needs"] } }),
    );
    q["rows"].as_array().unwrap().clone()
}

fn tick(d: &mut Daemon) -> u64 {
    ok(d, "sim.status", json!({}))["tick"].as_u64().unwrap()
}

fn events(d: &mut Daemon, name: &str) -> Vec<Json> {
    ok(d, "obs.events", json!({ "name": name, "limit": 100 }))["rows"]
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn a_save_loads_back_to_the_same_world_and_it_plays_on_the_same() {
    let t = temp("saves-roundtrip");
    colony(t.path());
    let mut d = open(t.path());
    assert_eq!(needs(&mut d, "food").len(), 5, "founded at the load");

    // The game saves itself at tick 30: the world of that tick, written
    // once the step is over.
    ok(&mut d, "sim.step", json!({ "n": 40 }));
    let saved = events(&mut d, "game.saved");
    assert_eq!(saved.len(), 1, "{saved:?}");
    assert_eq!(saved[0]["fields"]["save"], "auto");
    assert_eq!(saved[0]["fields"]["tick"], 31);
    assert!(t.path().join(".argus/saves/auto.save").exists());

    // A save by hand, then on, then back.
    let s = ok(&mut d, "sim.save", json!({ "name": "manual" }));
    assert_eq!(
        (s["tick"].clone(), s["path"].clone()),
        (json!(40), json!(".argus/saves/manual.save"))
    );
    let at_40 = needs(&mut d, "food");
    ok(&mut d, "sim.step", json!({ "n": 25 }));
    let at_65 = needs(&mut d, "food");
    assert_ne!(at_40, at_65);
    let l = ok(&mut d, "sim.load", json!({ "name": "manual" }));
    assert_eq!(l["tick"], 40);
    assert_eq!(l["scene"], "arena");
    assert_eq!(l["migrated"], json!([]));
    assert_eq!(tick(&mut d), 40);
    assert_eq!(needs(&mut d, "food"), at_40, "the world of the save");
    ok(&mut d, "sim.step", json!({ "n": 25 }));
    assert_eq!(needs(&mut d, "food"), at_65, "and it plays on the same");
    assert_eq!(events(&mut d, "game.loaded").len(), 1);
    // The log's numbers keep counting over a load: a `since` cursor taken
    // before it sees what comes after.
    let seqs: Vec<u64> = ok(&mut d, "obs.events", json!({ "limit": 1000 }))["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["seq"].as_u64().unwrap())
        .collect();
    assert!(seqs[0] > 1, "{:?}", &seqs[..3]);
    let before = seqs[seqs.len() - 1];
    ok(&mut d, "sim.load", json!({ "name": "manual" }));
    let after = ok(&mut d, "obs.events", json!({ "since": before }));
    assert!(after["total"].as_u64().unwrap() >= 1, "{after}");

    // The autosave is the world of tick 30's end.
    ok(&mut d, "sim.load", json!({ "name": "auto" }));
    assert_eq!(tick(&mut d), 31);

    // Fire asks the game to load the manual save: done after the step.
    ok(&mut d, "sim.step", json!({ "n": 2 }));
    ok(
        &mut d,
        "input.inject",
        json!({ "action": "fire", "value": true, "frames": 1 }),
    );
    ok(&mut d, "sim.step", json!({ "n": 1 }));
    assert_eq!(tick(&mut d), 40, "the manual save, loaded");
    assert_eq!(needs(&mut d, "food"), at_40);

    // Loaded while playing, it plays on.
    ok(&mut d, "sim.play", json!({}));
    ok(&mut d, "sim.load", json!({ "name": "auto" }));
    assert!(d.wants_frames(), "playing after the load");
    d.frame().unwrap();
    assert_eq!(tick(&mut d), 32);
    ok(&mut d, "sim.pause", json!({}));

    // A save can go to a file of its own: a test's fixture.
    ok(
        &mut d,
        "sim.save",
        json!({ "name": "week1", "path": "tests/saves/week1.save" }),
    );
    assert!(t.path().join("tests/saves/week1.save").exists());

    let e = err(&mut d, "sim.load", json!({ "name": "nothing" }));
    assert_eq!(e.code, codes::BAD_SAVE);
}

#[test]
fn a_save_from_the_last_version_loads_through_the_migration() {
    let t = temp("saves-migrate");
    colony(t.path());
    let mut d = open(t.path());
    ok(&mut d, "sim.step", json!({ "n": 40 }));
    ok(&mut d, "sim.save", json!({ "name": "manual" }));
    let food: Vec<f64> = needs(&mut d, "food")
        .iter()
        .map(|r| r[1].as_f64().unwrap())
        .collect();

    // The next version of the game: Needs v2, a system that knows it.
    write(t.path(), "components/needs.luau", NEEDS_V2);
    write(t.path(), "systems/colony.luau", COLONY_V2);
    let l = ok(&mut d, "sim.load", json!({ "name": "manual" }));
    assert_eq!(
        l["migrated"],
        json!([{ "component": "Needs", "from": 1, "to": 2, "entities": 5, "declared": true, "left": [] }])
    );
    let ev = events(&mut d, "component.migrated");
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0]["fields"]["entities"], 5);
    let hunger: Vec<f64> = needs(&mut d, "hunger")
        .iter()
        .map(|r| r[1].as_f64().unwrap())
        .collect();
    for (f, h) in food.iter().zip(&hunger) {
        assert!((h - (1.0 - f / 100.0)).abs() < 1e-6, "{f} -> {h}");
    }
    assert!(needs(&mut d, "rest").iter().all(|r| r[1] == 1.0));
    // It plays on the new rules.
    ok(&mut d, "sim.step", json!({ "n": 10 }));
    let later = needs(&mut d, "hunger")[0][1].as_f64().unwrap();
    assert!((later - hunger[0] - 0.01).abs() < 1e-5, "{later}");

    // A save of v2 is refused by v1: a save does not migrate backwards.
    ok(&mut d, "sim.save", json!({ "name": "v2" }));
    write(t.path(), "components/needs.luau", NEEDS_V1);
    write(t.path(), "systems/colony.luau", COLONY_V1);
    let before = (tick(&mut d), needs(&mut d, "hunger"));
    let e = err(&mut d, "sim.load", json!({ "name": "v2" }));
    assert_eq!(e.code, argus_ecs::codes::BAD_STATE);
    assert!(
        e.message
            .contains("newer version of the game: Needs v2 (this build has v1)"),
        "{}",
        e.message
    );
    // A load that fails leaves the game that was running as it was.
    assert_eq!((tick(&mut d), needs(&mut d, "hunger")), before);
}
