//! Phase 1 item 6 through the protocol: a room carved with edit.tiles, a
//! player moved by injected actions through its CharacterController,
//! contacts as events in the log and in a script's q:events, and a tile
//! edit that is ground (or not) on the next tick (§12, §8 L2).

mod common;

use common::*;
use serde_json::{json, Value as Json};
use std::path::Path;

fn platformer(root: &Path) {
    write(
        root,
        "argus.toml",
        "name = \"platformer\"\nclock = \"continuous@60\"\ndefault_scene = \"room\"\nplugins = []\n",
    );
    write(
        root,
        "input.toml",
        "[actions.move]\ntype = \"axis2\"\n[actions.move.bindings]\nkeyboard = [\"wasd\"]\n\n[actions.jump]\ntype = \"button\"\n[actions.jump.bindings]\nkeyboard = [\"space\"]\n\n[slots.1]\ndevices = [\"keyboard\"]\n",
    );
    argus_assets::encode_png(
        &root.join("assets/tiles/set.png"),
        12,
        4,
        &[120, 110, 100, 255].repeat(48),
    )
    .unwrap();
    write(
        root,
        "assets/tiles/set.toml",
        "grid = [3, 1]\n\n[[frames]]\nindex = 0\nproperties = { solid = true }\n\n[[frames]]\nindex = 1\nproperties = { one_way = true }\n\n[[frames]]\nindex = 2\nproperties = { slope = [0, 1] }\n",
    );
    write(
        root,
        "components/heard.luau",
        "component \"Heard\" {\n  doc = \"What the landing listener heard\",\n  landings = field.i32 { default = 0, doc = \"Landings seen\" },\n  speed = field.f32 { default = 0, doc = \"Speed of the last one\" },\n}\n",
    );
    write(
        root,
        "systems/listen.luau",
        r#"system "late.listen" {
  stage = "late",
  reads = { "Heard" },
  writes = { "Heard" },
  doc = "Counts the landings physics.move reported this tick.",
  run = function(q)
    for _, ev in ipairs(q:events("body.landed")) do
      local e = ev.entity
      if e:has("Heard") then
        local h = e:get("Heard")
        h.landings = h.landings + 1
        h.speed = ev.speed
      end
    end
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
        "scenes/room/player.toml",
        "[Transform]\npos = [3.5, 3.0]\n\n[Collider]\nsize = [0.8, 1.6]\n\n[KinematicBody]\n\n[CharacterController]\n\n[PlayerSlot]\nslot = 1\n\n[Actions]\n\n[Heard]\n",
    );
    write(
        root,
        "scenes/room/floor.toml",
        "[Transform]\npos = [0.0, 8.0]\n\n[Tilemap]\nmap = \"scenes/room/room.tilemap.toml\"\n",
    );
}

fn player(d: &mut argus_daemon::Daemon) -> Json {
    ok(d, "world.get", json!({ "entity": "@player" }))["components"].clone()
}

#[test]
fn a_player_runs_jumps_and_lands_on_carved_tiles() {
    let t = temp("physics");
    platformer(t.path());
    let mut d = open(t.path());
    let file = "scenes/room/room.tilemap.toml";
    ok(
        &mut d,
        "scene.tiles.create",
        json!({ "file": file, "width": 16, "height": 8, "chunk": 8,
                "layers": [{ "name": "ground", "tileset": "tiles/set", "collision": true }] }),
    );
    // Floor along the bottom, a one-way ledge, a slope up to a step.
    commit(
        &mut d,
        "agent:claude",
        "carve the room",
        &[
            (
                "edit.tiles",
                json!({ "file": file, "layer": "ground", "rect": { "x": 0, "y": 7, "w": 16, "h": 1 }, "tile": 1 }),
            ),
            (
                "edit.tiles",
                json!({ "file": file, "layer": "ground", "rect": { "x": 6, "y": 4, "w": 3, "h": 1 }, "tile": 2 }),
            ),
            (
                "edit.tiles",
                json!({ "file": file, "layer": "ground", "rect": { "x": 11, "y": 6, "w": 1, "h": 1 }, "tile": 3 }),
            ),
            (
                "edit.tiles",
                json!({ "file": file, "layer": "ground", "rect": { "x": 12, "y": 6, "w": 4, "h": 1 }, "tile": 1 }),
            ),
        ],
    );
    let col = ok(
        &mut d,
        "scene.tiles.collision",
        json!({ "file": file, "rect": { "x": 5, "y": 4, "w": 11, "h": 4 } }),
    );
    assert_eq!(
        col["rows"],
        json!([".---.......", "...........", "....../####", "###########"])
    );

    // It falls onto the floor: grounded, the event logged, the script heard it.
    ok(&mut d, "sim.step", json!({ "n": 40 }));
    let p = player(&mut d);
    assert_eq!(p["KinematicBody"]["grounded"], true, "{p}");
    let feet = p["Transform"]["pos"][1].as_f64().unwrap() - 0.8;
    assert!((feet - 1.0).abs() < 1e-3, "{feet}");
    let landed = ok(
        &mut d,
        "obs.events",
        json!({ "name": "body.landed", "entity": "@player" }),
    );
    assert_eq!(landed["total"], 1);
    assert_eq!(
        p["Heard"]["landings"], 1,
        "the late-stage script read the event the same tick"
    );
    assert!(p["Heard"]["speed"].as_f64().unwrap() > 1.0);

    // Actions in, movement out: run right, jump onto the one-way ledge.
    ok(
        &mut d,
        "input.inject",
        json!({ "action": "move", "value": [1, 0], "frames": 20 }),
    );
    ok(&mut d, "sim.step", json!({ "n": 20 }));
    let x = player(&mut d)["Transform"]["pos"][0].as_f64().unwrap();
    assert!(x > 3.5 + 1.5, "ran right: {x}");
    ok(
        &mut d,
        "input.inject",
        json!({ "action": "jump", "value": true, "frames": 30 }),
    );
    ok(&mut d, "sim.step", json!({ "n": 90 }));
    let jumped = ok(
        &mut d,
        "obs.events",
        json!({ "name": "body.jumped", "entity": "@player" }),
    );
    assert_eq!(jumped["total"], 1);
    let p = player(&mut d);
    let feet = p["Transform"]["pos"][1].as_f64().unwrap() - 0.8;
    assert!((feet - 4.0).abs() < 1e-3, "on the one-way ledge: {feet}");
    assert_eq!(p["KinematicBody"]["on_one_way"], true);
    assert_eq!(p["Heard"]["landings"], 2);

    // Erase the ledge under it: the next tick it falls, because the physics
    // step reads the same tiles the edit wrote.
    commit(
        &mut d,
        "human:kevin",
        "remove the ledge",
        &[(
            "edit.tiles",
            json!({ "file": file, "layer": "ground", "rect": { "x": 6, "y": 4, "w": 3, "h": 1 }, "tile": 0 }),
        )],
    );
    ok(&mut d, "sim.step", json!({ "n": 1 }));
    assert_eq!(player(&mut d)["KinematicBody"]["grounded"], false);
    ok(&mut d, "sim.step", json!({ "n": 60 }));
    let p = player(&mut d);
    let feet = p["Transform"]["pos"][1].as_f64().unwrap() - 0.8;
    assert!((feet - 1.0).abs() < 1e-3, "back on the floor: {feet}");
    assert_eq!(p["Heard"]["landings"], 3);

    // The systems are listed where they run, with what they write.
    let sys = ok(&mut d, "script.systems", json!({}));
    let names: Vec<&str> = sys["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "post.hitstop",
            "input.actions",
            "ui.input",
            "physics.control",
            "input.rebind",
            "physics.move",
            "physics.overlap",
            "nav.agents",
            "anim.update",
            "tween.update",
            "late.listen",
            "camera.update",
            "post.update",
            "particles.update",
            "ui.layout"
        ]
    );
    let w = ok(
        &mut d,
        "world.who_writes",
        json!({ "component": "KinematicBody" }),
    );
    assert_eq!(
        w["systems"],
        json!(["physics.control", "physics.move"]),
        "{w}"
    );
    // The feel parameters are tune fields.
    let desc = ok(
        &mut d,
        "world.describe",
        json!({ "component": "CharacterController" }),
    );
    let tuned: Vec<&str> = desc["rows"][0]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["tune"] == true)
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    for f in [
        "coyote_time",
        "jump_buffer",
        "corner_correction",
        "step_height",
        "jump_height",
        "run_speed",
    ] {
        assert!(tuned.contains(&f), "{f} is a tune field: {tuned:?}");
    }
}
