//! Phase 2 item 6 through the protocol: a landing is a hit, and the
//! camera's PostFlash, PostShake, and PostHitStop answer it. The game stands
//! still while the flash and the shake play on, and a jump pressed during
//! the stop lands after it. The semantic frame lists what is on; every
//! setting is a tune field; and a script fades the screen with PostFade when
//! the camera changes room (§13 post stack, §8 effects, §12 room hook).

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
        "systems/juice.luau",
        r#"system "late.thud" {
  stage = "late",
  doc = "A landing is a hit.",
  run = function(q)
    for _, ev in ipairs(q:events("body.landed")) do
      q:emit("thud", { amount = 1 })
    end
  end,
}
return system "late.fade" {
  stage = "late",
  after = { "camera.update" },
  writes = { "Tween" },
  doc = "A new room fades out and back in.",
  run = function(q)
    for _, ev in ipairs(q:events("camera.zone")) do
      if ev.zone == "b" then
        q:tween(q:entity("@a/camera"), "PostFade.amount", { to = 1, duration = 0.1, yoyo = true, repeats = 1 })
      end
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
        "[Transform]\npos = [0.0, 0.0]\n\n[Camera]\nheight = 12.0\ntarget = \"@player\"\noffset = [0.0, 1.0]\n\n\
         [PostFlash]\non = \"thud\"\ncolor = \"#FF0000\"\n\n\
         [PostShake]\non = \"thud\"\ntrauma = 0.8\n\n\
         [PostHitStop]\non = \"thud\"\nframes = 5\n\n\
         [PostFade]\n",
    );
    write(
        root,
        "scenes/hollow/rooms/a/floor.toml",
        "[Transform]\npos = [0.0, 14.0]\n\n[Tilemap]\nmap = \"scenes/hollow/floor.tilemap.toml\"\n",
    );
}

fn effects(d: &mut argus_daemon::Daemon) -> Vec<Json> {
    ok(d, "obs.frame", json!({ "format": "semantic" }))["effects"]
        .as_array()
        .unwrap()
        .clone()
}

fn kind<'a>(fx: &'a [Json], k: &str) -> Option<&'a Json> {
    fx.iter().find(|e| e["kind"] == k)
}

fn player_y(d: &mut argus_daemon::Daemon) -> f64 {
    ok(d, "world.get", json!({ "entity": "@a/player" }))["components"]["Transform"]["pos"][1]
        .as_f64()
        .unwrap()
}

#[test]
fn a_hit_flashes_shakes_and_stops_and_a_new_room_fades() {
    let t = temp("post");
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
    assert!(effects(&mut d).is_empty(), "nothing on before a hit");

    // Fall until the landing: the thud is the hit, on that same tick.
    let mut landed = false;
    for _ in 0..60 {
        ok(&mut d, "sim.step", json!({ "n": 1 }));
        if ok(&mut d, "obs.events", json!({ "name": "thud" }))["total"] == 1 {
            landed = true;
            break;
        }
    }
    assert!(landed);
    let fx = effects(&mut d);
    assert_eq!(kind(&fx, "flash").unwrap()["amount"], 0.6, "{fx:?}");
    assert_eq!(kind(&fx, "flash").unwrap()["color"], "#FF0000");
    assert_eq!(kind(&fx, "screen shake").unwrap()["trauma"], 0.8);
    assert_eq!(kind(&fx, "hit-stop").unwrap()["ticks_left"], 5);

    // A jump tapped now: pressed and released within the stop.
    let y = player_y(&mut d);
    ok(
        &mut d,
        "input.inject",
        json!({ "action": "jump", "value": true, "frames": 2 }),
    );
    let mut flashes = Vec::new();
    for _ in 0..5 {
        ok(&mut d, "sim.step", json!({ "n": 1 }));
        assert_eq!(player_y(&mut d), y, "the game stands still");
        let fx = effects(&mut d);
        flashes.push(kind(&fx, "flash").unwrap()["amount"].as_f64().unwrap());
    }
    assert!(
        flashes.windows(2).all(|w| w[1] < w[0]),
        "the flash fades on through the stop: {flashes:?}"
    );
    assert!(kind(&effects(&mut d), "hit-stop").is_none(), "over");
    // The jump was kept, and lands on the first tick after.
    ok(&mut d, "sim.step", json!({ "n": 3 }));
    assert!(
        (player_y(&mut d) - y).abs() > 0.05,
        "jumped after the stop: {y} to {}",
        player_y(&mut d)
    );

    // Every setting of the trio is a tune field, adjustable together.
    let fields = ok(&mut d, "tune.fields", json!({ "entity": "@a/camera" }));
    let paths: Vec<&str> = fields["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["path"].as_str().unwrap())
        .collect();
    for p in [
        "PostFlash.peak",
        "PostFlash.decay",
        "PostShake.trauma",
        "PostShake.curve",
        "PostHitStop.frames",
        "PostHitStop.scale",
        "PostFade.amount",
    ] {
        assert!(paths.contains(&p), "{p} in {paths:?}");
    }

    // Run right into room b: the script's tween fades out and back in.
    ok(
        &mut d,
        "input.inject",
        json!({ "action": "move", "value": [1, 0], "frames": 300 }),
    );
    let mut faded = 0.0f64;
    for _ in 0..300 {
        ok(&mut d, "sim.step", json!({ "n": 1 }));
        if let Some(f) = kind(&effects(&mut d), "fade") {
            faded = faded.max(f["amount"].as_f64().unwrap());
        }
        if faded > 0.0 && kind(&effects(&mut d), "fade").is_none() {
            break;
        }
    }
    let zone = ok(&mut d, "obs.events", json!({ "name": "camera.zone" }));
    assert_eq!(zone["total"], 2);
    assert!(faded > 0.9, "all the way out: {faded}");
    let amount = ok(&mut d, "world.get", json!({ "entity": "@a/camera" }))["components"]
        ["PostFade"]["amount"]
        .clone();
    assert_eq!(amount, 0.0, "and back in");
}
