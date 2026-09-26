//! Phase 1 item 7 through the protocol: a camera entity follows the player
//! across two rooms, keeping to each room's bounds from room.toml, a script
//! shakes it on landing through the camera.shake event, and obs.frame says
//! which camera drew and the rect it showed (§12 "Camera", §4 rooms, §8).

mod common;

use common::*;
use serde_json::{json, Value as Json};
use std::path::Path;

fn hollow(root: &Path) {
    write(
        root,
        "argus.toml",
        "name = \"hollow\"\nclock = \"continuous@60\"\ndefault_scene = \"hollow\"\nplugins = []\n",
    );
    write(
        root,
        "input.toml",
        "[actions.move]\ntype = \"axis2\"\n[actions.move.bindings]\nkeyboard = [\"wasd\"]\n\n[actions.jump]\ntype = \"button\"\n[actions.jump.bindings]\nkeyboard = [\"space\"]\n\n[slots.1]\ndevices = [\"keyboard\"]\n",
    );
    argus_assets::encode_png(
        &root.join("assets/tiles/rock.png"),
        4,
        4,
        &[120, 110, 100, 255].repeat(16),
    )
    .unwrap();
    write(
        root,
        "assets/tiles/rock.toml",
        "grid = [1, 1]\n\n[[frames]]\nindex = 0\nproperties = { solid = true }\n",
    );
    write(
        root,
        "systems/thud.luau",
        r#"system "late.thud" {
  stage = "late",
  doc = "Shakes the camera when anything lands.",
  run = function(q)
    for _, ev in ipairs(q:events("body.landed")) do
      q:emit("camera.shake", { amount = 0.5 })
    end
  end,
}
"#,
    );
    write(
        root,
        "scenes/hollow/scene.toml",
        "clock = \"continuous@60\"\nseed = 1\n\n[rooms]\nstart = \"a\"\nradius = 1\n\n[rooms.graph]\n\"a\" = [\"b\"]\n",
    );
    write(
        root,
        "scenes/hollow/rooms/a/room.toml",
        "bounds = [0.0, 0.0, 24.0, 14.0]\nneighbors = [\"b\"]\n",
    );
    write(
        root,
        "scenes/hollow/rooms/b/room.toml",
        "bounds = [24.0, 0.0, 60.0, 14.0]\nneighbors = [\"a\"]\n",
    );
    write(
        root,
        "scenes/hollow/rooms/a/player.toml",
        "[Transform]\npos = [4.0, 4.0]\n\n[Collider]\nsize = [0.8, 1.6]\n\n[KinematicBody]\n\n[CharacterController]\n\n[PlayerSlot]\nslot = 1\n\n[Actions]\n",
    );
    write(
        root,
        "scenes/hollow/rooms/a/camera.toml",
        "[Transform]\npos = [0.0, 0.0]\n\n[Camera]\nheight = 12.0\ntarget = \"@player\"\noffset = [0.0, 1.0]\n\n[CameraShake]\n",
    );
    write(
        root,
        "scenes/hollow/rooms/a/floor.toml",
        "[Transform]\npos = [0.0, 14.0]\n\n[Tilemap]\nmap = \"scenes/hollow/floor.tilemap.toml\"\n",
    );
}

fn camera(d: &mut argus_daemon::Daemon) -> Json {
    ok(d, "world.get", json!({ "entity": "@a/camera" }))["components"].clone()
}

fn f(v: &Json) -> f64 {
    v.as_f64().unwrap()
}

#[test]
fn a_camera_follows_the_player_through_two_rooms_and_keeps_to_each() {
    let t = temp("camera");
    hollow(t.path());
    let mut d = open(t.path());
    let file = "scenes/hollow/floor.tilemap.toml";
    ok(
        &mut d,
        "scene.tiles.create",
        json!({ "file": file, "width": 60, "height": 14, "chunk": 16,
                "layers": [{ "name": "ground", "tileset": "tiles/rock", "collision": true }] }),
    );
    commit(
        &mut d,
        "agent:claude",
        "a floor through both rooms",
        &[(
            "edit.tiles",
            json!({ "file": file, "layer": "ground", "rect": { "x": 0, "y": 13, "w": 60, "h": 1 }, "tile": 1 }),
        )],
    );

    // It lands; the landing shook the camera the same tick; the camera keeps
    // to room a: the player is near its left wall, so the view's left edge
    // is the wall.
    ok(&mut d, "sim.step", json!({ "n": 30 }));
    let zone = ok(&mut d, "obs.events", json!({ "name": "camera.zone" }));
    assert_eq!(zone["total"], 1);
    assert_eq!(zone["rows"][0]["fields"]["zone"], "a");
    let shakes = ok(&mut d, "obs.events", json!({ "name": "camera.shake" }));
    assert_eq!(shakes["total"], 1, "the script asked for one shake");
    let c = camera(&mut d);
    let trauma = f(&c["CameraShake"]["trauma"]);
    assert!(
        trauma > 0.0 && trauma < 0.5,
        "raised then decaying: {trauma}"
    );
    let (min, max) = (&c["Camera"]["view_min"], &c["Camera"]["view_max"]);
    let off = &c["CameraShake"]["offset"];
    assert!(
        (f(&min[0]) - f(&off[0])).abs() < 1e-3,
        "the view's left edge is room a's wall, shake aside: {c}"
    );
    assert!((f(&max[1]) - f(&min[1]) - 12.0).abs() < 1e-3);

    // Run right into room b: the camera crosses over and keeps to b.
    ok(
        &mut d,
        "input.inject",
        json!({ "action": "move", "value": [1, 0], "frames": 240 }),
    );
    ok(&mut d, "sim.step", json!({ "n": 300 }));
    let px = f(
        &ok(&mut d, "world.get", json!({ "entity": "@a/player" }))["components"]["Transform"]
            ["pos"][0],
    );
    assert!(px > 30.0, "ran into room b: {px}");
    let zone = ok(&mut d, "obs.events", json!({ "name": "camera.zone" }));
    assert_eq!(zone["total"], 2);
    assert_eq!(zone["rows"][1]["fields"]["zone"], "b");
    let c = camera(&mut d);
    assert!(
        f(&c["Camera"]["view_min"][0]) >= 24.0 - 1e-3,
        "inside room b: {c}"
    );
    assert_eq!(c["Camera"]["zone"], "b");
    assert!(f(&c["CameraShake"]["trauma"]) == 0.0, "the shake is over");

    // The follow settings are tune fields, the camera is a system in late.
    let desc = ok(&mut d, "world.describe", json!({ "component": "Camera" }));
    let tuned: Vec<&str> = desc["rows"][0]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["tune"] == true)
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    for n in ["dead_zone", "lookahead", "smoothing", "offset", "height"] {
        assert!(tuned.contains(&n), "{n}: {tuned:?}");
    }
    let shake = ok(
        &mut d,
        "world.describe",
        json!({ "component": "CameraShake" }),
    );
    let tuned: Vec<&str> = shake["rows"][0]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["tune"] == true)
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    for n in ["decay", "curve", "max_offset", "frequency"] {
        assert!(tuned.contains(&n), "{n}: {tuned:?}");
    }
    // The timelines write it too: a camera cut makes a camera active.
    let w = ok(&mut d, "world.who_writes", json!({ "component": "Camera" }));
    assert_eq!(w["systems"], json!(["timeline.update", "camera.update"]));

    // The picture: which camera drew, and the rect it showed.
    if let Some(fr) = frame(&mut d, json!({ "width": 640, "height": 360 })) {
        assert_eq!(fr["camera"], "@a/camera", "{fr}");
        let rect = &fr["view"]["rect"];
        assert!(
            (f(&rect[0]) - f(&c["Camera"]["view_min"][0])).abs() < 1e-3,
            "{fr} {c}"
        );
        assert!((f(&rect[3]) - f(&c["Camera"]["view_max"][1])).abs() < 1e-3);
    }
}
