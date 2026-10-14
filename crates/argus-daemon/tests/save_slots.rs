//! 2D staples item 4: save slots. A script lists the saves there are with
//! what each holds (`q:saves`), newest first, with the game's own summary
//! fields; deletes one (`q:delete_save`); and what a tick read is part of
//! the run, so a re-run of it reads the same list whatever the disk says
//! now. A release player lists the saves it is given.

mod common;

use common::*;
use serde_json::{json, Value as Json};
use std::path::Path;

fn project(root: &Path) {
    write(
        root,
        "argus.toml",
        "name = \"slots\"\nclock = \"continuous@60\"\ndefault_scene = \"main\"\nplugins = []\n",
    );
    write(
        root,
        "input.toml",
        "[actions.jump]\ntype = \"button\"\n[actions.jump.bindings]\nkeyboard = [\"space\"]\n\n[slots.1]\ndevices = [\"keyboard\"]\n",
    );
    write(
        root,
        "components/seen.luau",
        r#"component "Seen" {
  doc = "The saves the menu sees",
  names = field.list { of = "str" },
  level = field.i32 {},
  place = field.str {},
}
"#,
    );
    write(
        root,
        "systems/slots.luau",
        r#"system "sim.saver" {
  stage = "sim",
  doc = "Saves two slots and deletes the first.",
  run = function(q)
    if q.tick == 2 then q:save("slot1", { level = 3, place = "Cave" }) end
    if q.tick == 4 then q:save("slot2", { level = 4, place = "Hall" }) end
    if q.tick == 8 then q:delete_save("slot1") end
  end,
}

system "sim.menu" {
  stage = "sim",
  after = { "sim.saver" },
  writes = { "Seen" },
  doc = "Lists the saves, as a slot menu would.",
  run = function(q)
    local s = q:entity("@eye"):get("Seen")
    local names = {}
    for _, v in ipairs(q:saves()) do table.insert(names, v.name) end
    s.names = names
    local first = q:saves()[1]
    s.level = first and (first.summary.level or 0) or 0
    s.place = first and (first.summary.place or "") or ""
  end,
}
"#,
    );
    write(
        root,
        "scenes/main/scene.toml",
        "clock = \"continuous@60\"\nseed = 1\n",
    );
    write(root, "scenes/main/eye.toml", "[Seen]\n");
}

fn seen(d: &mut argus_daemon::Daemon, field: &str) -> Json {
    let q = ok(
        d,
        "world.query",
        json!({ "select": [format!("Seen.{field}")], "where": { "has": ["Seen"] } }),
    );
    q["rows"][0][0].clone()
}

fn names(d: &mut argus_daemon::Daemon) -> Vec<String> {
    let mut v: Vec<String> = seen(d, "names")
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_str().unwrap().to_string())
        .collect();
    v.sort();
    v
}

#[test]
fn a_menu_lists_the_saves_with_their_summaries_and_deletes_one() {
    let t = temp("save-slots");
    project(t.path());
    let mut d = open(t.path());
    let saves = t.path().join(".argus").join("saves");

    // Ticks 0 to 2: slot1 is written after the step that asked; tick 3
    // lists it.
    ok(&mut d, "sim.step", json!({ "n": 3 }));
    ok(&mut d, "sim.step", json!({ "n": 1 }));
    assert_eq!(names(&mut d), ["slot1"]);
    assert_eq!(seen(&mut d, "level"), 3);
    assert_eq!(seen(&mut d, "place"), "Cave");

    // Tick 4 asks for slot2; tick 5 lists both.
    ok(&mut d, "sim.step", json!({ "n": 1 }));
    ok(&mut d, "sim.step", json!({ "n": 1 }));
    assert_eq!(names(&mut d), ["slot1", "slot2"]);

    // Tick 8 deletes slot1, from the disk too; tick 9 lists slot2.
    ok(&mut d, "sim.step", json!({ "n": 3 }));
    ok(&mut d, "sim.step", json!({ "n": 1 }));
    assert_eq!(names(&mut d), ["slot2"]);
    assert_eq!(seen(&mut d, "level"), 4);
    assert!(!saves.join("slot1.save").exists());
    assert!(saves.join("slot2.save").exists());
    let ev = ok(&mut d, "obs.events", json!({ "name": "game.save_deleted" })).to_string();
    assert!(ev.contains("slot1"), "{ev}");

    // Tick 3 again: it reads what it read the first time, slot1 with its
    // summary, though slot1 is gone from the disk now.
    ok(&mut d, "sim.seek", json!({ "frame": 3 }));
    ok(&mut d, "sim.step", json!({ "n": 1 }));
    assert_eq!(names(&mut d), ["slot1"]);
    assert_eq!(seen(&mut d, "place"), "Cave");
    drop(d);

    // A release player lists what its host gives it.
    let mut p = argus_player::Player::open(t.path(), None, None).unwrap();
    p.list_saves(Some(&saves));
    p.step(1).unwrap();
    let eye = p
        .world
        .named()
        .find(|(n, _)| *n == "eye")
        .map(|(_, e)| e)
        .expect("@eye");
    assert_eq!(
        p.world.get_field(eye, "Seen.names").unwrap(),
        argus_ecs::Value::List(vec![argus_ecs::Value::Str("slot2".into())])
    );
}
