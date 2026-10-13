//! List fields through the daemon (issue 22): declared in a component and
//! a def table, written in an entity file and def rows, read and assigned
//! by a script, carried by an event, read and set over the protocol and
//! written back to the file, kept through a seek; a misspelt ref in a
//! list is a load error at its line, and a mod adds to a row's list or
//! takes from it.

mod common;

use argus_daemon::Daemon;
use common::*;
use serde_json::{json, Value as Json};
use std::path::Path;

fn lists(root: &Path) {
    project(root);
    write(
        root,
        "defs/monsters.luau",
        "def \"monsters\" { key = \"id\", prefix = \"monster\", id = field.str {}, hp = field.i32 { default = 1 } }\n",
    );
    write(
        root,
        "defs/monsters.toml",
        "[rat]\nhp = 1\n\n[bat]\nhp = 2\n",
    );
    write(
        root,
        "defs/rooms.luau",
        r#"def "rooms" {
  key = "id", prefix = "room",
  id = field.str {},
  pool = field.list { of = "ref", doc = "What may be met there" },
  tags = field.list { of = "str" },
}
"#,
    );
    write(
        root,
        "defs/rooms.toml",
        "[cellar]\npool = [\"monster:rat\", \"monster:bat\"]\ntags = [\"damp\", \"dark\"]\n",
    );
    write(
        root,
        "components/tile.luau",
        r#"component "Tile" {
  doc = "A floor tile",
  on = field.list { of = "str", default = { "dry" }, doc = "What is on it, in the order it came" },
  floors = field.list { of = "u8" },
}
"#,
    );
    write(
        root,
        "systems/burn.luau",
        r#"system "sim.burn" {
  stage = "sim",
  reads = { "Tile" },
  writes = { "Tile" },
  doc = "On the second tick the oiled tile catches fire.",
  run = function(q)
    if q.tick ~= 1 then return end
    for e, t in q:each("Tile") do
      local o = table.clone(t.on)
      table.insert(o, "burning")
      t.on = o
      local wrote = pcall(function() t.on[1] = "wet" end)
      assert(not wrote, "a list read is read-only")
      local room = q:def("room:cellar")
      q:emit("pool", { pool = room.pool, n = #room.pool, first = room.pool[1], on = t.on })
    end
  end,
}
"#,
    );
    write(
        root,
        "scenes/arena/tile.toml",
        "[Tile]\non = [\"oiled\"]\nfloors = [1, 2]\n",
    );
}

fn on(d: &mut Daemon) -> Json {
    let q = ok(
        d,
        "world.query",
        json!({ "select": ["Tile.on"], "where": { "has": ["Tile"] } }),
    );
    q["rows"][0][0].clone()
}

#[test]
fn a_list_field_through_files_scripts_and_the_protocol() {
    let t = temp("lists");
    lists(t.path());
    let mut d = open(t.path());
    let r = ok(&mut d, "scene.load", json!({}));
    assert!(r.get("issues").is_none(), "{r}");
    let desc = ok(&mut d, "world.describe", json!({ "component": "Tile" })).to_string();
    assert!(
        desc.contains("list of str") && desc.contains("list of u8"),
        "{desc}"
    );
    assert_eq!(on(&mut d), json!(["oiled"]));

    ok(&mut d, "sim.step", json!({ "n": 3 }));
    assert_eq!(
        on(&mut d),
        json!(["oiled", "burning"]),
        "assigned by the script"
    );
    let ev = ok(&mut d, "obs.events", json!({ "name": "pool" }));
    let f = &ev["rows"][0]["fields"];
    assert_eq!(f["pool"], json!(["monster:rat", "monster:bat"]), "{ev}");
    assert_eq!(f["n"], 2);
    assert_eq!(f["first"], "monster:rat");
    assert_eq!(f["on"], json!(["oiled", "burning"]));
    let defs = ok(&mut d, "world.defs", json!({ "table": "rooms" }));
    assert!(defs.to_string().contains("[\"damp\",\"dark\"]"), "{defs}");

    // Back to before the fire, and forward again.
    ok(&mut d, "sim.seek", json!({ "frame": 1 }));
    assert_eq!(on(&mut d), json!(["oiled"]));
    ok(&mut d, "sim.seek", json!({ "frame": 3 }));
    assert_eq!(on(&mut d), json!(["oiled", "burning"]));

    // Set over the protocol: the live world, then the entity's file.
    commit(
        &mut d,
        "test",
        "wet it",
        &[(
            "edit.set",
            json!({ "entity": "@tile", "path": "Tile.on", "value": ["wet", "cold"] }),
        )],
    );
    assert_eq!(on(&mut d), json!(["wet", "cold"]));
    let file = read(t.path(), "scenes/arena/tile.toml");
    assert!(file.contains("on = [\"wet\", \"cold\"]"), "{file}");
    let tx = ok(
        &mut d,
        "edit.begin",
        json!({ "author": "test", "note": "too big" }),
    )["tx"]
        .clone();
    ok(
        &mut d,
        "edit.set",
        json!({ "tx": tx, "entity": "@tile", "path": "Tile.floors", "value": [1, 300] }),
    );
    let bad = err(&mut d, "edit.commit", json!({ "tx": tx }));
    assert!(bad.message.contains("element 2"), "{}", bad.message);
}

#[test]
fn a_misspelt_ref_in_a_list_is_refused_at_its_line() {
    let t = temp("lists-ref");
    lists(t.path());
    let mut d = open(t.path());
    write(
        t.path(),
        "defs/rooms.toml",
        "[cellar]\ntags = [\"damp\"]\npool = [\"monster:rat\", \"monster:rta\"]\n",
    );
    let r = ok(&mut d, "scene.load", json!({}));
    let issues = r["issues"].as_array().cloned().unwrap_or_default();
    let it = issues
        .iter()
        .find(|i| i["message"].as_str().unwrap_or("").contains("monster:rta"))
        .unwrap_or_else(|| panic!("{r}"));
    let msg = it["message"].as_str().unwrap();
    assert!(msg.contains("rooms.cellar.pool, element 2"), "{msg}");
    assert_eq!(it["data"]["ref"], "defs/rooms.toml:3", "{it}");
}

#[test]
fn a_mod_adds_to_a_rows_list_and_takes_from_it() {
    let t = temp("lists-mod");
    lists(t.path());
    write(
        t.path(),
        "mods/more/mod.toml",
        "id = \"more\"\nname = \"More\"\n",
    );
    write(
        t.path(),
        "mods/more/defs/rooms.toml",
        "[cellar]\n\"+pool\" = [\"monster:bat\"]\n-tags = [\"damp\"]\n",
    );
    let mut d = open(t.path());
    let r = ok(&mut d, "scene.load", json!({ "mods": ["more"] }));
    assert!(r.get("issues").is_none(), "{r}");
    let defs = ok(&mut d, "world.defs", json!({ "table": "rooms" }));
    let text = defs.to_string();
    assert!(
        text.contains("[\"monster:rat\",\"monster:bat\",\"monster:bat\"]"),
        "the mod's bat added at the end: {defs}"
    );
    assert!(text.contains("[\"dark\"]"), "damp taken out: {defs}");

    // Both a whole list and a change to it: refused, saying which.
    write(
        t.path(),
        "mods/more/defs/rooms.toml",
        "[cellar]\npool = [\"monster:rat\"]\n\"+pool\" = [\"monster:bat\"]\n",
    );
    let r = ok(&mut d, "scene.load", json!({ "mods": ["more"] }));
    assert!(
        r.to_string()
            .contains("sets pool and adds to or takes from it too"),
        "{r}"
    );
}
