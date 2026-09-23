//! Phase 1 item 8 through the protocol, the gate's own words: the player has
//! a run, jump, and land animation with a footstep event that fires a
//! sound. The animator is an asset, its states follow the physics body's
//! fields, the footstep asks the mixer for sfx/step, the mixer answers, and
//! a broken animator file is an import failure with its line (§13, §8, §14).

mod common;

use common::*;
use serde_json::{json, Value as Json};
use std::path::Path;

const ANIMATOR: &str = r#"type = "animator"
initial = "idle"

[states.idle]
clip = "idle"

[[states.idle.transitions]]
to = "run"
when = "KinematicBody.grounded and abs(KinematicBody.velocity.x) > 0.5"

[states.run]
clip = "run"
events = [{ frame = 1, name = "footstep", sound = "sfx/step" }, { frame = 3, name = "footstep", sound = "sfx/step" }]

[[states.run.transitions]]
to = "idle"
when = "KinematicBody.grounded and abs(KinematicBody.velocity.x) <= 0.5"

[states.jump]
clip = "jump"

[[states.jump.transitions]]
to = "fall"
when = "KinematicBody.velocity.y <= 0"

[states.fall]
clip = "fall"

[[states.fall.transitions]]
to = "land"
when = "KinematicBody.grounded"

[states.land]
clip = "land"
loop = false

[[states.land.transitions]]
to = "idle"
when = "end"

[[any]]
to = "jump"
when = "not KinematicBody.grounded and KinematicBody.velocity.y > 0"

[[any]]
to = "fall"
when = "not KinematicBody.grounded and KinematicBody.velocity.y < -1"
"#;

fn project(root: &Path) {
    write(
        root,
        "argus.toml",
        "name = \"anim\"\nclock = \"continuous@60\"\ndefault_scene = \"room\"\nplugins = []\n",
    );
    write(
        root,
        "input.toml",
        "[actions.move]\ntype = \"axis2\"\n[actions.move.bindings]\nkeyboard = [\"wasd\"]\n\n[actions.jump]\ntype = \"button\"\n[actions.jump.bindings]\nkeyboard = [\"space\"]\n\n[slots.1]\ndevices = [\"keyboard\"]\n",
    );
    write(root, "audio.toml", "[buses]\nmaster = {}\nsfx = { parent = \"master\" }\n\n[sounds.\"sfx/step\"]\nvariations = [ { wave = \"noise\", ms = 60 } ]\n");
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
    // The hero: an 8-frame sheet with a clip per animation.
    argus_assets::encode_png(
        &root.join("assets/hero.png"),
        32,
        8,
        &[230, 110, 60, 255].repeat(256),
    )
    .unwrap();
    write(
        root,
        "assets/hero.toml",
        "grid = [8, 1]\n\n[clips.idle]\nframes = [0]\n\n[clips.run]\nfrom = 1\nto = 4\nfps = 12\nloop = true\n\n[clips.jump]\nframes = [5]\n\n[clips.fall]\nframes = [6]\n\n[clips.land]\nframes = [7, 0]\nfps = 12\nloop = false\n",
    );
    write(root, "assets/anim/hero.toml", ANIMATOR);
    write(
        root,
        "scenes/room/scene.toml",
        "clock = \"continuous@60\"\nseed = 1\n",
    );
    write(
        root,
        "scenes/room/player.toml",
        "[Transform]\npos = [4.0, 3.0]\n\n[Sprite]\natlas = \"hero\"\nsize = [0.8, 1.6]\n\n[Collider]\nsize = [0.8, 1.6]\n\n[KinematicBody]\n\n[CharacterController]\n\n[PlayerSlot]\nslot = 1\n\n[Actions]\n\n[Animator]\nmachine = \"anim/hero\"\n",
    );
    write(
        root,
        "scenes/room/floor.toml",
        "[Transform]\npos = [0.0, 8.0]\n\n[Tilemap]\nmap = \"scenes/room/room.tilemap.toml\"\n",
    );
}

/// The states the player entered, in order.
fn entered(d: &mut argus_daemon::Daemon) -> Vec<String> {
    ok(
        d,
        "obs.events",
        json!({ "name": "anim.entered", "entity": "@player", "limit": 200 }),
    )["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["fields"]["state"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn the_player_runs_jumps_and_lands_and_its_footsteps_fire_a_sound() {
    let t = temp("animation");
    project(t.path());
    let mut d = open(t.path());
    let assets = ok(&mut d, "asset.list", json!({ "kind": "animator" }));
    assert_eq!(assets["total"], 1, "{assets}");
    assert_eq!(assets["rows"][0][0], "anim/hero");
    let file = "scenes/room/room.tilemap.toml";
    ok(
        &mut d,
        "scene.tiles.create",
        json!({ "file": file, "width": 30, "height": 8, "chunk": 8,
                "layers": [{ "name": "ground", "tileset": "tiles/rock", "collision": true }] }),
    );
    commit(
        &mut d,
        "agent:claude",
        "floor",
        &[(
            "edit.tiles",
            json!({ "file": file, "layer": "ground", "rect": { "x": 0, "y": 7, "w": 30, "h": 1 }, "tile": 1 }),
        )],
    );

    // It drops onto the floor: fall, land, back to idle.
    ok(&mut d, "sim.step", json!({ "n": 60 }));
    assert_eq!(entered(&mut d), ["idle", "fall", "land", "idle"]);
    let p = ok(&mut d, "world.get", json!({ "entity": "@player" }))["components"].clone();
    assert_eq!(p["Animator"]["state"], "idle");
    assert_eq!(p["Sprite"]["frame"], 0);

    // Run: the run clip plays and each footstep asks for sfx/step, which the
    // mixer answers in the log.
    ok(
        &mut d,
        "input.inject",
        json!({ "action": "move", "value": [1, 0], "frames": 60 }),
    );
    ok(&mut d, "sim.step", json!({ "n": 40 }));
    let p = ok(&mut d, "world.get", json!({ "entity": "@player" }))["components"].clone();
    assert_eq!(p["Animator"]["state"], "run", "{p}");
    let f = p["Sprite"]["frame"].as_u64().unwrap();
    assert!((1..=4).contains(&f), "a run frame: {f}");
    let steps = ok(
        &mut d,
        "obs.events",
        json!({ "name": "footstep", "entity": "@player" }),
    );
    assert!(steps["total"].as_u64().unwrap() >= 3, "{steps}");
    assert_eq!(steps["rows"][0]["fields"]["state"], "run");
    let asked = ok(
        &mut d,
        "obs.events",
        json!({ "name": "sound.play", "entity": "@player" }),
    );
    assert_eq!(asked["total"], steps["total"], "one sound per footstep");
    assert_eq!(asked["rows"][0]["fields"]["sound"], "sfx/step");
    let played = ok(
        &mut d,
        "obs.events",
        json!({ "name": "sound.played", "entity": "@player" }),
    );
    assert_eq!(
        played["total"], steps["total"],
        "the mixer answered each: {played}"
    );

    // Jump while running: jump, fall, land.
    ok(
        &mut d,
        "input.inject",
        json!({ "action": "jump", "value": true, "frames": 30 }),
    );
    ok(&mut d, "sim.step", json!({ "n": 100 }));
    let seq = entered(&mut d);
    let tail: Vec<&str> = seq[4..].iter().map(String::as_str).collect();
    assert_eq!(&tail[..4], ["run", "jump", "fall", "land"], "{seq:?}");
    assert!(ok(&mut d, "obs.events", json!({ "name": "anim.error" }))["total"] == 0);

    // A broken animator file fails its import with the line; the last good
    // version keeps playing, so a typo does not freeze the player.
    write(
        t.path(),
        "assets/anim/hero.toml",
        &ANIMATOR.replace("when = \"end\"", "when = \"end or time > 1\""),
    );
    let r = ok(&mut d, "asset.import", json!({}));
    assert_eq!(r["failed"].as_array().unwrap().len(), 1, "{r}");
    assert_eq!(r["failed"][0]["name"], "anim/hero");
    assert!(
        r["failed"][0]["message"]
            .as_str()
            .unwrap()
            .contains("join with and"),
        "{r}"
    );
    let at = r["failed"][0]["at"].as_str().unwrap();
    assert!(at.starts_with("assets/anim/hero.toml:"), "{at}");
    ok(&mut d, "sim.step", json!({ "n": 60 }));
    ok(
        &mut d,
        "input.inject",
        json!({ "action": "move", "value": [-1, 0], "frames": 30 }),
    );
    ok(&mut d, "sim.step", json!({ "n": 20 }));
    let p = ok(&mut d, "world.get", json!({ "entity": "@player" }))["components"].clone();
    assert_eq!(
        p["Animator"]["state"], "run",
        "still animating on the last good file: {p}"
    );

    // The rate a human would tune is a tune field on the Animator.
    let desc = ok(&mut d, "world.describe", json!({ "component": "Animator" }));
    let speed = desc["rows"][0]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "speed")
        .unwrap()
        .clone();
    assert_eq!(speed["tune"], true);
    let w: Json = ok(&mut d, "world.who_writes", json!({ "component": "Sprite" }));
    assert_eq!(w["systems"], json!(["anim.update"]));
}
