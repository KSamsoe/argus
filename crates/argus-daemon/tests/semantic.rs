//! Phase 1 item 12 through the protocol: the semantic frame of a small
//! platformer room (the camera's rect, the HUD, the player and a sign one
//! by one, the coins as a group, what the tiles in view collide with) in a
//! few KB and without a picture; and the annotated picture, whose marks
//! are the same ids and bounds (§8 L4 and L5, §12 accessibility).

mod common;

use common::*;
use serde_json::json;
use std::path::Path;

fn project(root: &Path) {
    write(
        root,
        "argus.toml",
        "name = \"frames\"\nclock = \"continuous@60\"\ndefault_scene = \"room\"\nplugins = []\n",
    );
    // A two-tile set: solid rock, then a one-way plank.
    let mut px = [120u8, 110, 100, 255].repeat(16);
    px.extend([170u8, 120, 60, 255].repeat(16));
    let mut rgba = Vec::new();
    for row in 0..4 {
        rgba.extend_from_slice(&px[row * 16..row * 16 + 16]);
        rgba.extend_from_slice(&px[64 + row * 16..64 + row * 16 + 16]);
    }
    argus_assets::encode_png(&root.join("assets/tiles/set.png"), 8, 4, &rgba).unwrap();
    write(
        root,
        "assets/tiles/set.toml",
        "grid = [2, 1]\n\n[[frames]]\nindex = 0\nproperties = { solid = true }\n\n[[frames]]\nindex = 1\nproperties = { one_way = true }\n",
    );
    write(
        root,
        "components/game.luau",
        "component \"Wallet\" {\n  doc = \"Coins picked up\",\n  coins = field.u32 { default = 3 },\n}\n\ncomponent \"Coin\" {\n  doc = \"A coin to pick up\",\n  value = field.u32 { default = 1 },\n}\n",
    );
    write(
        root,
        "scenes/room/scene.toml",
        "clock = \"continuous@60\"\nseed = 1\n",
    );
    write(
        root,
        "scenes/room/camera.toml",
        "[Transform]\npos = [6.0, 4.0]\n\n[Camera]\nheight = 8.0\n",
    );
    write(
        root,
        "scenes/room/player.toml",
        "[Transform]\npos = [6.0, 3.0]\n\n[Sprite]\nkind = \"rect\"\nsize = [1.0, 1.0]\ncolor = \"#F2E6C8\"\nz = 5.0\n\n[Wallet]\n\n[Describe]\nname = \"player\"\ntext = \"{Wallet.coins} coins\"\n",
    );
    write(
        root,
        "scenes/room/hud.toml",
        "[Describe]\nrole = \"hud\"\ntext = \"Coins {@player:Wallet.coins}. Room 1.\"\n",
    );
    write(
        root,
        "scenes/room/sign.toml",
        "[Transform]\npos = [9.0, 3.0]\n\n[Sprite]\nkind = \"rect\"\nsize = [0.8, 1.0]\ncolor = \"#8A6A40\"\n\n[Describe]\nname = \"sign\"\ntext = \"Beware of the pit\"\n",
    );
    for i in 0..5 {
        write(
            root,
            &format!("scenes/room/coin{i}.toml"),
            &format!("[Transform]\npos = [{}.5, 5.0]\n\n[Sprite]\nkind = \"circle\"\nsize = [0.4, 0.4]\ncolor = \"#FFD866\"\n\n[Coin]\n", 3 + i),
        );
    }
    write(
        root,
        "scenes/room/floor.toml",
        "[Transform]\npos = [0.0, 8.0]\n\n[Tilemap]\nmap = \"scenes/room/room.tilemap.toml\"\n",
    );
}

#[test]
fn the_semantic_frame_says_what_the_player_sees_and_the_picture_is_marked() {
    let t = temp("frames");
    project(t.path());
    let mut d = open(t.path());
    // Ground along the bottom and a plank to stand on.
    let file = "scenes/room/room.tilemap.toml";
    ok(
        &mut d,
        "scene.tiles.create",
        json!({ "file": file, "width": 12, "height": 8, "layers": [{ "name": "ground", "tileset": "tiles/set", "collision": true }] }),
    );
    commit(
        &mut d,
        "agent:claude",
        "floor and plank",
        &[
            (
                "edit.tiles",
                json!({ "file": file, "layer": "ground", "rect": { "x": 0, "y": 7, "w": 12, "h": 1 }, "tile": 1 }),
            ),
            (
                "edit.tiles",
                json!({ "file": file, "layer": "ground", "rect": { "x": 4, "y": 4, "w": 3, "h": 1 }, "tile": 2 }),
            ),
        ],
    );

    // The camera 8 units tall over 640 x 360: 45 px a unit, centred on (6, 4).
    let f = ok(&mut d, "obs.frame", json!({ "format": "semantic" }));
    assert_eq!(f["format"], "semantic");
    assert_eq!(f["camera"]["id"], "@camera");
    assert_eq!(f["camera"]["rect"], json!([-1.11, 0.0, 13.11, 8.0]));
    assert_eq!(
        f["hud"],
        json!([{ "id": "@hud", "role": "hud", "text": "Coins 3. Room 1." }])
    );
    let rows = f["entities"].as_array().unwrap();
    let ids: Vec<&str> = rows.iter().map(|r| r[0].as_str().unwrap()).collect();
    assert_eq!(
        ids,
        ["@player", "@sign"],
        "named ones, in reading order: {f}"
    );
    assert_eq!(rows[0][2], "3 coins");
    // (6, 3) is 45 px below the centre: a 45 px square around (320, 225).
    assert_eq!(rows[0][3], json!([297, 202, 343, 248]));
    assert_eq!(rows[1][1], "sign");
    let coins = &f["groups"][0];
    assert_eq!(
        (coins["what"].clone(), coins["count"].clone()),
        (json!("Coin"), json!(5))
    );
    // The tiles in view: the plank in row 4, the ground in row 7.
    let map = &f["tiles"]["maps"][0];
    assert_eq!(map["entity"], "@floor");
    let trows: Vec<&str> = map["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap())
        .collect();
    assert_eq!(trows[4], "....---.....", "{trows:?}");
    assert_eq!(trows[7], "############");
    assert_eq!(map["cell"], json!([0, 0]));
    let bytes = f["bytes"].as_u64().unwrap();
    assert!(bytes < 3072, "about 3 KB: {bytes}");
    assert!(
        (bytes as i64 - f.to_string().len() as i64).abs() < 64,
        "bytes is honest"
    );

    // After a step and a pickup, the frame follows the world.
    commit(
        &mut d,
        "agent:claude",
        "a coin, and a step right",
        &[
            (
                "edit.set",
                json!({ "entity": "@player", "path": "Wallet.coins", "value": 4 }),
            ),
            (
                "edit.set",
                json!({ "entity": "@player", "path": "Transform.pos", "value": [8.0, 3.0] }),
            ),
        ],
    );
    ok(&mut d, "sim.step", json!({ "n": 1 }));
    let f = ok(
        &mut d,
        "obs.frame",
        json!({ "format": "semantic", "width": 320, "height": 180 }),
    );
    assert_eq!(f["hud"][0]["text"], "Coins 4. Room 1.");
    // 22.5 px a unit now: (8, 3) is 45 px right of the centre and 22.5 below.
    assert_eq!(f["entities"][0][3], json!([193, 101, 217, 124]), "{f}");

    // The picture, marked: the same ids and bounds, drawn over it.
    if let Some(p) = frame(&mut d, json!({ "annotate": true, "path": "shot.png" })) {
        assert_eq!(p["annotated"], true);
        let marks = p["marks"].as_array().unwrap();
        let labels: Vec<&str> = marks.iter().map(|m| m[0].as_str().unwrap()).collect();
        assert_eq!(labels, ["Coin x5", "@player", "@sign"], "{p}");
        assert!(marks.iter().all(|m| m[2] == true), "every label fits: {p}");
        let img = argus_assets::decode_png(&t.path().join("shot.png")).unwrap();
        let b = marks[1][1]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u32)
            .collect::<Vec<_>>();
        let (w, rgba) = (img.0, img.2);
        let i = (((b[1] + 10) * w + b[0]) * 4) as usize;
        assert_eq!(
            &rgba[i..i + 3],
            &[255, 220, 40],
            "a yellow box edge at the player's bounds"
        );
        // obs.annotate is the same call.
        let q = ok(&mut d, "obs.annotate", json!({ "path": "shot2.png" }));
        assert_eq!(q["marks"], p["marks"]);
    }
}
