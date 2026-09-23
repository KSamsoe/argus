//! Phase 1 item 9 through the protocol: a script squashes the player when
//! it lands and nudges the camera with q:tween, what is animating is a
//! world.query, the tweens end with their events, and who_writes names the
//! tween system for what it touched (§13 "Tweens", §3 write sets).

mod common;

use common::*;
use serde_json::json;
use std::path::Path;

fn project(root: &Path) {
    write(
        root,
        "argus.toml",
        "name = \"tweens\"\nclock = \"continuous@60\"\ndefault_scene = \"room\"\nplugins = []\n",
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
        "systems/juice.luau",
        r#"system "late.juice" {
  stage = "late",
  writes = { "Tween" },
  doc = "Squash on landing, and a camera nudge.",
  run = function(q)
    for _, ev in ipairs(q:events("body.landed")) do
      q:tween(ev.entity, "Transform.scale", {
        from = vector.create(1.4, 0.6, 0), to = vector.create(1, 1, 0),
        duration = 0.2, easing = "out_back", on_complete = "squash.done",
      })
      q:tween(q:entity("@camera"), "Camera.offset.y", { to = -0.5, duration = 0.1, yoyo = true, repeats = 1 })
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
    write(root, "scenes/room/player.toml", "[Transform]\npos = [4.0, 3.0]\n\n[Collider]\nsize = [0.8, 1.6]\n\n[KinematicBody]\n\n[CharacterController]\n");
    write(
        root,
        "scenes/room/camera.toml",
        "[Transform]\npos = [4.0, 3.0]\n\n[Camera]\nheight = 10.0\n",
    );
    write(
        root,
        "scenes/room/floor.toml",
        "[Transform]\npos = [0.0, 8.0]\n\n[Tilemap]\nmap = \"scenes/room/room.tilemap.toml\"\n",
    );
}

#[test]
fn a_landing_squashes_the_player_and_nudges_the_camera() {
    let t = temp("tweens");
    project(t.path());
    let mut d = open(t.path());
    let file = "scenes/room/room.tilemap.toml";
    ok(
        &mut d,
        "scene.tiles.create",
        json!({ "file": file, "width": 12, "height": 8, "layers": [{ "name": "ground", "tileset": "tiles/rock", "collision": true }] }),
    );
    commit(
        &mut d,
        "agent:claude",
        "floor",
        &[(
            "edit.tiles",
            json!({ "file": file, "layer": "ground", "rect": { "x": 0, "y": 7, "w": 12, "h": 1 }, "tile": 1 }),
        )],
    );
    // Fall until it lands, then one tick more for the tweens to start.
    let mut landed = false;
    for _ in 0..60 {
        ok(&mut d, "sim.step", json!({ "n": 1 }));
        if ok(&mut d, "obs.events", json!({ "name": "body.landed" }))["total"] == 1 {
            landed = true;
            break;
        }
    }
    assert!(landed);
    ok(&mut d, "sim.step", json!({ "n": 1 }));

    // What is animating right now is a query.
    let q = ok(
        &mut d,
        "world.query",
        json!({ "select": ["Tween.target", "Tween.path", "Tween.time"], "where": { "has": ["Tween"] } }),
    );
    assert_eq!(q["total"], 2, "{q}");
    let paths: Vec<&str> = q["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r[1].as_str().unwrap())
        .collect();
    assert!(
        paths.contains(&"Transform.scale") && paths.contains(&"Camera.offset.y"),
        "{paths:?}"
    );
    let scale = ok(&mut d, "world.get", json!({ "entity": "@player" }))["components"]["Transform"]
        ["scale"]
        .clone();
    assert!(scale[0].as_f64().unwrap() > 1.2, "squashed wide: {scale}");

    // Both end: the squash with its event, the nudge there and back.
    ok(&mut d, "sim.step", json!({ "n": 20 }));
    let done = ok(&mut d, "obs.events", json!({ "name": "squash.done" }));
    assert_eq!(done["total"], 1);
    assert_eq!(done["rows"][0]["fields"]["entity"], "@player");
    let scale = ok(&mut d, "world.get", json!({ "entity": "@player" }))["components"]["Transform"]
        ["scale"]
        .clone();
    assert_eq!(scale, json!([1.0, 1.0]));
    let cam = ok(&mut d, "world.get", json!({ "entity": "@camera" }))["components"]["Camera"]
        ["offset"]
        .clone();
    assert!(
        cam[1].as_f64().unwrap().abs() < 1e-4,
        "nudged and back: {cam}"
    );
    let q = ok(
        &mut d,
        "world.query",
        json!({ "where": { "has": ["Tween"] } }),
    );
    assert_eq!(q["total"], 0, "nothing animating");

    // The write sets tell the truth about what the tweens touched.
    let w = ok(&mut d, "world.who_writes", json!({ "component": "Camera" }));
    assert!(
        w["systems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s == "tween.update"),
        "{w}"
    );
    let w = ok(
        &mut d,
        "world.who_writes",
        json!({ "component": "Transform" }),
    );
    assert!(
        w["systems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s == "tween.update"),
        "{w}"
    );
    // Duration and delay are tune fields.
    let desc = ok(&mut d, "world.describe", json!({ "component": "Tween" }));
    let tuned: Vec<&str> = desc["rows"][0]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["tune"] == true)
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(tuned, ["duration", "delay"]);
}
