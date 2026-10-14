//! Inactive entities through the daemon (issue 18): a script switches a
//! group of entities off and on (declaring the write); while off they are
//! out of q:each, drawing, and the semantic frame, found by q:inactive and
//! by the protocol's query; a seek puts back what was off; and an entity
//! file can be written inactive, its children with it.

mod common;

use common::*;
use serde_json::{json, Value as Json};
use std::path::Path;

fn rooms(root: &Path) {
    write(
        root,
        "argus.toml",
        "name = \"rooms\"\nclock = \"continuous@60\"\ndefault_scene = \"floor\"\nplugins = []\n",
    );
    write(
        root,
        "components/floor.luau",
        r#"component "Floor" {
  doc = "Which room is shown",
  shown = field.str { default = "a" },
  seen_b = field.i32 { default = 0 },
  parked_b = field.i32 { default = 0 },
}
component "room_a" { doc = "In room a" }
component "room_b" { doc = "In room b" }
"#,
    );
    write(
        root,
        "systems/rooms.luau",
        r#"system "sim.rooms" {
  stage = "sim",
  reads = { "Floor", "room_a", "room_b" },
  writes = { "Floor", "Inactive" },
  doc = "Shows one room at a time: the other is switched off.",
  run = function(q)
    for _, f in q:each("Floor") do
      if q.tick == 2 then f.shown = "b" end
      for e in q:each("room_a") do e:set_active(f.shown == "a") end
      for e in q:inactive("room_a") do e:set_active(f.shown == "a") end
      for e in q:each("room_b") do e:set_active(f.shown == "b") end
      for e in q:inactive("room_b") do e:set_active(f.shown == "b") end
      f.seen_b = q:count("room_b")
      local n = 0
      for _ in q:inactive("room_b") do n += 1 end
      f.parked_b = n
    end
  end,
}
"#,
    );
    write(
        root,
        "systems/sneaky.luau",
        r#"system "sim.sneaky" {
  stage = "sim", after = { "sim.rooms" },
  reads = { "room_b" },
  doc = "Switches an entity off without declaring the write.",
  run = function(q)
    if q.tick ~= 5 then return end
    for e in q:each("room_b") do e:set_active(false) end
  end,
}
"#,
    );
    write(
        root,
        "scenes/floor/scene.toml",
        "clock = \"continuous@60\"\nseed = 1\n",
    );
    write(
        root,
        "scenes/floor/camera.toml",
        "[Transform]\npos = [0.0, 0.0]\n\n[Camera]\nheight = 10.0\n",
    );
    write(root, "scenes/floor/floor.toml", "[Floor]\n");
    for (room, x) in [("a", -2.0), ("b", 2.0)] {
        write(
            root,
            &format!("scenes/floor/table_{room}.toml"),
            &format!(
                "tags = [\"room_{room}\"]\n\n[Transform]\npos = [{x}, 0.0]\n\n[Sprite]\nkind = \"rect\"\nsize = [1.0, 1.0]\n\n[Describe]\nname = \"table {room}\"\n"
            ),
        );
    }
    // A chest written inactive, with a coin inside it.
    write(
        root,
        "scenes/floor/chest.toml",
        "tags = [\"Inactive\"]\n\n[Transform]\npos = [0.0, 3.0]\n\n[Sprite]\nkind = \"rect\"\nsize = [1.0, 1.0]\n\n[Describe]\nname = \"chest\"\n",
    );
    write(
        root,
        "scenes/floor/coin.toml",
        "[Transform]\npos = [0.0, 0.0]\n\n[Parent]\nentity = \"@chest\"\n\n[Sprite]\nkind = \"circle\"\nsize = [0.4, 0.4]\n\n[Describe]\nname = \"coin\"\n",
    );
}

fn in_frame(d: &mut argus_daemon::Daemon) -> Vec<String> {
    let f = ok(d, "obs.frame", json!({ "format": "semantic" }));
    let mut ids: Vec<String> = f["entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r[0].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    ids
}

fn floor(d: &mut argus_daemon::Daemon, field: &str) -> Json {
    let q = ok(
        d,
        "world.query",
        json!({ "select": [format!("Floor.{field}")], "where": { "has": ["Floor"] } }),
    );
    q["rows"][0][0].clone()
}

#[test]
fn a_script_switches_a_room_off_and_on_and_the_frame_follows() {
    let t = temp("active");
    rooms(t.path());
    let mut d = open(t.path());
    assert_eq!(
        in_frame(&mut d),
        ["@table_a", "@table_b"],
        "the chest, written inactive, and the coin in it are not shown"
    );
    let coin = ok(
        &mut d,
        "world.query",
        json!({ "select": ["id"], "where": { "has": ["InactiveUnder"] } }),
    );
    assert_eq!(
        coin["rows"],
        json!([["@coin"]]),
        "the coin is under the inactive chest: {coin}"
    );

    ok(&mut d, "sim.step", json!({ "n": 2 }));
    assert_eq!(in_frame(&mut d), ["@table_a"], "room b switched off");
    assert_eq!(floor(&mut d, "seen_b"), 0, "q:count skips it");
    assert_eq!(floor(&mut d, "parked_b"), 1, "q:inactive finds it");
    // The protocol's query sees the world as it is.
    let all = ok(
        &mut d,
        "world.query",
        json!({ "select": ["id"], "where": { "has": ["room_b"] } }),
    );
    assert_eq!(all["total"], 1, "{all}");
    let active = ok(
        &mut d,
        "world.query",
        json!({ "select": ["id"], "where": { "has": ["room_b"], "not": ["Inactive", "InactiveUnder"] } }),
    );
    assert_eq!(active["total"], 0, "{active}");

    ok(&mut d, "sim.step", json!({ "n": 2 }));
    assert_eq!(in_frame(&mut d), ["@table_b"], "on tick 2 it swapped");
    // Back before the swap: a seek puts back which was off.
    ok(&mut d, "sim.seek", json!({ "frame": 2 }));
    assert_eq!(in_frame(&mut d), ["@table_a"]);
    ok(&mut d, "sim.seek", json!({ "frame": 4 }));
    assert_eq!(in_frame(&mut d), ["@table_b"]);

    // A system that did not declare the write is refused, saying so.
    ok(&mut d, "sim.step", json!({ "n": 2 }));
    let errs = ok(&mut d, "script.errors", json!({})).to_string();
    assert!(
        errs.contains("sim.sneaky") && errs.contains("Inactive"),
        "{errs}"
    );

    // Switched on by an edit: the chest and the coin in it are shown.
    commit(
        &mut d,
        "test",
        "open the chest",
        &[(
            "edit.remove",
            json!({ "entity": "@chest", "component": "Inactive" }),
        )],
    );
    let shown = in_frame(&mut d);
    assert!(
        shown.contains(&"@chest".to_string()) && shown.contains(&"@coin".to_string()),
        "{shown:?}"
    );
}
