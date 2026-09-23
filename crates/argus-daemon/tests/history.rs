//! Phase 1 item 13 through the protocol: obs.diff (what changed since I
//! last looked), obs.trace (one entity's fields over a window), and obs.why
//! (the declared write sets walked back from an event), on a small fight
//! where three rats bite the hero (§8 L3 and its helpers).

mod common;

use argus_daemon::history::History;
use argus_ecs::{Transform, Vec2, World};
use common::*;
use serde_json::json;
use std::path::Path;

fn project(root: &Path) {
    write(
        root,
        "argus.toml",
        "name = \"fight\"\nclock = \"continuous@60\"\ndefault_scene = \"room\"\nplugins = []\n",
    );
    write(
        root,
        "components/game.luau",
        r#"component "Health" {
  doc = "Hit points",
  cur = field.i32 { default = 10, range = {0, 1000} },
}
component "Rat" { doc = "Bites", bite = field.i32 { default = 3 } }
component "Velocity" { doc = "Units a tick", v = field.vec2 {} }
"#,
    );
    write(
        root,
        "systems/spawn.luau",
        r#"system "sim.spawn" {
  stage = "sim",
  writes = { "Rat", "Health", "Transform", "Velocity" },
  doc = "Three rats at tick 5.",
  run = function(q)
    if q.tick == 5 then
      for i = 1, 3 do
        local r = q:spawn({ "Rat", "Health", "Transform", "Velocity" })
        r:get("Transform").pos = vector.create(2 + i, 0, 0)
        r:get("Velocity").v = vector.create(-0.05, 0, 0)
      end
    end
  end,
}
"#,
    );
    write(
        root,
        "systems/move.luau",
        r#"system "sim.move" {
  stage = "sim",
  after = { "sim.spawn" },
  reads = { "Velocity" },
  writes = { "Transform" },
  doc = "Things move by their velocity.",
  run = function(q)
    for e, v, t in q:each("Velocity", "Transform") do
      t.pos = t.pos + v.v
    end
  end,
}
"#,
    );
    write(
        root,
        "systems/bite.luau",
        r#"system "sim.bite" {
  stage = "sim",
  after = { "sim.move" },
  reads = { "Rat", "Transform" },
  writes = { "Health" },
  doc = "Every tenth tick each rat near the hero bites.",
  run = function(q)
    if q.tick % 10 ~= 0 then return end
    local hero = q:entity("@hero")
    local hp = hero:get("Health")
    for r, rat, t in q:each("Rat", "Transform") do
      if math.abs(t.pos.x) < 2.8 and hp.cur > 0 then
        hp.cur = math.max(0, hp.cur - rat.bite)
        q:emit("damage", { target = hero, rat = r, amount = rat.bite })
        if hp.cur == 0 then q:emit("hero.died", { entity = hero }) end
      end
    end
  end,
}
"#,
    );
    write(
        root,
        "systems/clear.luau",
        r#"system "sim.clear" {
  stage = "sim",
  doc = "The crate breaks at tick 20.",
  run = function(q)
    if q.tick == 20 then q:entity("@crate"):despawn() end
  end,
}
"#,
    );
    write(
        root,
        "scenes/room/scene.toml",
        "clock = \"continuous@60\"\nseed = 1\n",
    );
    write(
        root,
        "scenes/room/hero.toml",
        "[Transform]\npos = [0.0, 0.0]\n\n[Health]\ncur = 10\n",
    );
    write(
        root,
        "scenes/room/crate.toml",
        "[Transform]\npos = [-3.0, 0.0]\n\n[Health]\ncur = 1\n",
    );
}

#[test]
fn diff_trace_and_why_answer_what_happened_to_the_hero() {
    let t = temp("history");
    project(t.path());
    let mut d = open(t.path());

    // Nothing to compare with yet: the first look is the baseline.
    let first = ok(&mut d, "obs.diff", json!({}));
    assert!(
        first["note"].as_str().unwrap().contains("first look"),
        "{first}"
    );

    ok(&mut d, "sim.step", json!({ "n": 45 }));
    // What changed since I last looked (frame 0).
    let diff = ok(&mut d, "obs.diff", json!({}));
    assert_eq!(
        (diff["from"].clone(), diff["to"].clone()),
        (json!(0), json!(45))
    );
    assert_eq!(diff["appeared"]["count"], 3);
    assert_eq!(diff["appeared"]["kinds"][0][1], 3);
    assert!(
        diff["appeared"]["kinds"][0][0]
            .as_str()
            .unwrap()
            .contains("Rat"),
        "{diff}"
    );
    assert_eq!(diff["vanished"]["count"], 1);
    assert_eq!(diff["vanished"]["kinds"][0][2], json!(["@crate"]));
    let hero = diff["changed"]["entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e[0] == "@hero")
        .expect("the hero changed");
    let hp = &hero[1]["Health.cur"];
    assert_eq!(hp[0], 10);
    let now_hp = hp[1].as_i64().unwrap();
    assert!(now_hp < 10, "bitten: {diff}");
    assert!(diff["bytes"].as_u64().unwrap() < 1500, "about 1 KB: {diff}");
    // Asked again at once, nothing changed since that look.
    let again = ok(&mut d, "obs.diff", json!({}));
    assert_eq!(again["changed"]["count"], 0);
    assert_eq!(again["appeared"]["count"], 0);
    // A frame that has not happened is refused.
    let bad = err(&mut d, "obs.diff", json!({ "since": 1_000_000 }));
    assert!(bad.message.contains("has not happened"), "{}", bad.message);
    // Since a frame between two looks: from the look before it.
    let mid = ok(&mut d, "obs.diff", json!({ "since": 20 }));
    assert_eq!(mid["from"], 0);

    // The events say who emitted them.
    let bites = ok(&mut d, "obs.events", json!({ "name": "damage" }));
    assert!(bites["total"].as_u64().unwrap() >= 1);
    assert_eq!(bites["rows"][0]["by"], "sim.bite");
    let bite_frames: Vec<u64> = bites["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["frame"].as_u64().unwrap())
        .collect();

    // The hero's trace: Health.cur fell on exactly the frames of the bites.
    let tr = ok(
        &mut d,
        "obs.trace",
        json!({ "entity": "@hero", "fields": ["Health"] }),
    );
    assert_eq!(tr["traced"], true);
    let f = &tr["fields"][0];
    assert_eq!(f["path"], "Health.cur");
    assert_eq!(f["start"], 10);
    assert_eq!(f["end"], now_hp);
    let mut change_frames: Vec<u64> = f["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p[0].as_u64().unwrap())
        .collect();
    change_frames.dedup();
    let mut unique_bites = bite_frames.clone();
    unique_bites.dedup();
    assert_eq!(change_frames, unique_bites, "{tr}");
    let events: Vec<&str> = tr["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e[2].as_str().unwrap())
        .collect();
    assert!(events.contains(&"damage"), "{tr}");
    assert!(tr["writers"]["Health"]
        .as_array()
        .unwrap()
        .contains(&json!("sim.bite")));
    // A window: the last 10 frames only.
    let recent = ok(
        &mut d,
        "obs.trace",
        json!({ "entity": "@hero", "frames": 10 }),
    );
    assert_eq!(recent["from"], 35);

    // A named entity that is gone is still found, with when it went.
    let cr = ok(&mut d, "obs.trace", json!({ "entity": "@crate" }));
    assert_eq!(cr["gone"], 20, "the frame its despawn event carries: {cr}");
    let last = cr["events"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(
        (last[1].clone(), last[2].clone(), last[3].clone()),
        (json!(20), json!("despawned"), json!("sim.clear"))
    );

    // An unnamed rat is watched from the first time it is asked for.
    let rat = diff["appeared"]["kinds"][0][2][0]
        .as_str()
        .unwrap()
        .to_string();
    let w = ok(&mut d, "obs.trace", json!({ "entity": rat }));
    assert_eq!(
        (w["traced"].clone(), w["watching"].clone()),
        (json!(false), json!(true))
    );
    ok(&mut d, "sim.step", json!({ "n": 5 }));
    let w = ok(
        &mut d,
        "obs.trace",
        json!({ "entity": rat, "fields": ["Transform.pos"] }),
    );
    assert_eq!(
        w["fields"][0]["changes"], 4,
        "moved on each of the frames after the first: {w}"
    );

    // Why did the hero take damage: the bite, and what feeds it.
    let seq = bites["rows"][0]["seq"].as_u64().unwrap();
    let why = ok(&mut d, "obs.why", json!({ "event": seq }));
    assert_eq!(why["event"]["by"], "sim.bite");
    let rows = why["rows"].as_array().unwrap();
    let names: Vec<&str> = rows.iter().map(|r| r[0].as_str().unwrap()).collect();
    assert_eq!(
        rows[names.iter().position(|n| *n == "sim.bite").unwrap()][2],
        0
    );
    let mv = &rows[names.iter().position(|n| *n == "sim.move").unwrap()];
    assert_eq!(
        (mv[2].clone(), mv[3].clone()),
        (json!(1), json!("writes Transform, which sim.bite reads"))
    );
    assert!(
        mv[4].as_str().unwrap().contains("systems/move.luau"),
        "{mv}"
    );
    assert!(names.contains(&"sim.spawn"), "it writes Rat: {names:?}");
    assert!(
        !names.contains(&"sim.clear"),
        "it declares nothing sim.bite reads: {names:?}"
    );
    // In stage order, as the scheduler runs them.
    let order = ok(&mut d, "script.systems", json!({}));
    let run: Vec<&str> = order["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .filter(|n| names.contains(n))
        .collect();
    assert_eq!(names, run);
    // What writes the entities the event names: the rat and the hero.
    let ents = why["entities"].as_array().unwrap();
    let hero = ents
        .iter()
        .find(|e| e["id"] == "@hero")
        .expect("the hero is named");
    assert!(hero["writers"]["Health"]
        .as_array()
        .unwrap()
        .contains(&json!("sim.bite")));
    let rat_w = ents
        .iter()
        .find(|e| e["id"] != "@hero")
        .expect("so is the rat");
    assert!(
        rat_w["writers"]["Transform"]
            .as_array()
            .unwrap()
            .contains(&json!("sim.move")),
        "{why}"
    );
    // By name: the latest such event.
    let by_name = ok(&mut d, "obs.why", json!({ "name": "damage" }));
    assert_eq!(by_name["event"]["name"], "damage");
}

/// The history keeps a window of frames; what falls out of it becomes the
/// starting point, so a trace still knows where a field began.
#[test]
fn the_history_keeps_a_window_and_folds_the_rest_into_its_start() {
    let mut w = World::new();
    argus_ecs::hierarchy::register(&mut w).unwrap();
    let e = w.spawn_authored("mover").unwrap();
    w.insert(e, Transform::at(0.0, 0.0)).unwrap();
    let still = w.spawn_authored("still").unwrap();
    w.insert(still, Transform::at(5.0, 0.0)).unwrap();
    let mut h = History::default();
    h.window = 10;
    for f in 0..30u64 {
        w.update::<Transform, _>(e, |t| t.pos = Vec2::new(f as f32, 0.0))
            .unwrap();
        h.record(&w, f);
    }
    let t = h.trace(e).unwrap();
    assert_eq!(t.since, 20, "the last 10 frames are kept");
    let s = t.story(&w, 20, 29, &|_, _| true);
    let pos = s.fields.iter().find(|f| f.path == "Transform.pos").unwrap();
    assert_eq!(
        pos.changes.len(),
        9,
        "frames 21 to 29; 20 is where it starts"
    );
    assert_eq!(
        pos.start,
        Some(argus_ecs::Value::Vec2(Vec2::new(20.0, 0.0)))
    );
    let s = h.trace(still).unwrap().story(&w, 20, 29, &|_, _| true);
    assert!(
        s.fields.is_empty() && s.steady > 0,
        "what held still costs nothing"
    );
}

/// Recording the named entities costs a comparison each when nothing
/// changed. Release only: `cargo test --release -p argus-daemon --test
/// history -- --ignored`.
#[test]
#[ignore]
fn perf_a_thousand_named_entities() {
    let mut w = World::new();
    argus_ecs::hierarchy::register(&mut w).unwrap();
    let es: Vec<_> = (0..1000)
        .map(|i| {
            let e = w.spawn_authored(&format!("n{i}")).unwrap();
            w.insert(e, Transform::at(i as f32, 0.0)).unwrap();
            e
        })
        .collect();
    let mut h = History::default();
    let mut total = std::time::Duration::ZERO;
    for f in 0..600u64 {
        for &e in es.iter().take(100) {
            w.update::<Transform, _>(e, |t| t.pos.y = f as f32).unwrap();
        }
        let s = std::time::Instant::now();
        h.record(&w, f);
        total += s.elapsed();
    }
    let ms = total.as_secs_f64() * 1e3 / 600.0;
    println!("1,000 named entities, 100 moving: {ms:.3} ms a tick");
    assert!(ms < 0.5, "{ms} ms");
}

/// An entity that gains and loses components and changes more than six
/// fields: the diff says so, past the six it lists.
#[test]
fn a_diff_lists_six_fields_and_counts_the_rest_and_the_components() {
    let t = temp("history-many");
    write(
        t.path(),
        "argus.toml",
        "name = \"many\"\nclock = \"continuous@60\"\ndefault_scene = \"room\"\nplugins = []\n",
    );
    write(
        t.path(),
        "components/game.luau",
        "component \"Stats\" {\n  doc = \"Eight numbers\",\n  a = field.i32 {}, b = field.i32 {}, c = field.i32 {}, d = field.i32 {},\n  e = field.i32 {}, f = field.i32 {}, g = field.i32 {}, h = field.i32 {},\n}\ncomponent \"Old\" { doc = \"Goes\" }\ncomponent \"New\" { doc = \"Comes\" }\n",
    );
    write(
        t.path(),
        "systems/change.luau",
        r#"system "sim.change" {
  stage = "sim",
  writes = { "Stats", "Old", "New" },
  doc = "At tick 2, every stat goes up, Old goes, and New comes.",
  run = function(q)
    if q.tick ~= 2 then return end
    local e = q:entity("@it")
    local s = e:get("Stats")
    s.a = 1; s.b = 2; s.c = 3; s.d = 4; s.e = 5; s.f = 6; s.g = 7; s.h = 8
    e:remove("Old")
    e:insert("New")
  end,
}
"#,
    );
    write(
        t.path(),
        "scenes/room/scene.toml",
        "clock = \"continuous@60\"\nseed = 1\n",
    );
    write(t.path(), "scenes/room/it.toml", "[Stats]\n\n[Old]\n");
    let mut d = open(t.path());
    ok(&mut d, "world.summary", json!({}));
    ok(&mut d, "sim.step", json!({ "n": 5 }));
    let diff = ok(&mut d, "obs.diff", json!({}));
    let it = &diff["changed"]["entities"][0];
    assert_eq!(it[0], "@it");
    assert_eq!(
        it[1].as_object().unwrap().len(),
        6,
        "six fields listed: {diff}"
    );
    assert_eq!(
        it[2],
        json!({ "more_fields": 2, "added": ["New"], "removed": ["Old"] })
    );
    assert_eq!(
        diff["changed"]["components"],
        json!({ "added": { "New": 1 }, "removed": { "Old": 1 } })
    );
    assert_eq!(diff["changed"]["fields"].as_array().unwrap().len(), 8);
    // The trace sees the components come and go on the frame it happened.
    let tr = ok(&mut d, "obs.trace", json!({ "entity": "@it" }));
    let comps: Vec<(u64, String, String)> = tr["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c[0].as_u64().unwrap(),
                c[1].as_str().unwrap().to_string(),
                c[2].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert!(comps.contains(&(2, "Old".into(), "removed".into())), "{tr}");
    assert!(comps.contains(&(2, "New".into(), "added".into())), "{tr}");
}
