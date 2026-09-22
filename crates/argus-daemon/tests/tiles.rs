//! Phase 1 item 5 through the protocol: a tilemap created, carved with
//! terrain at the rect level in a transaction (author, feed, undo),
//! queried for cells and collision, and drawn by a Tilemap entity (§12).

mod common;

use common::*;
use serde_json::json;

const TILESET: &str = r#"grid = [4, 4]

# tile n is frame n - 1: the fully surrounded rock tile (16) is solid,
# and the top-left corner tile (7) one-way, to have both in one test.
[[frames]]
index = 15
properties = { solid = true }

[[frames]]
index = 6
properties = { one_way = true }

[autotile.rock]
mode = "4bit"
tiles = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
"#;

#[test]
fn a_room_is_carved_at_the_rect_level_undone_and_drawn() {
    let t = temp("tiles");
    project(t.path());
    argus_assets::encode_png(
        &t.path().join("assets/tiles/rock.png"),
        16,
        16,
        &[90, 80, 70, 255].repeat(256),
    )
    .unwrap();
    write(t.path(), "assets/tiles/rock.toml", TILESET);
    write(
        t.path(),
        "scenes/arena/camera.toml",
        "[Transform]\npos = [8.0, -4.0]\n\n[Camera]\nheight = 10.0\n",
    );
    let mut d = open(t.path());
    let file = "scenes/arena/room.tilemap.toml";
    let info = ok(
        &mut d,
        "scene.tiles.create",
        json!({
            "file": file, "width": 32, "height": 16, "chunk": 16,
            "layers": [{ "name": "ground", "tileset": "tiles/rock", "collision": true }]
        }),
    );
    assert_eq!(info["width"], 32);
    assert_eq!(info["layers"][0]["terrains"], json!(["rock"]));
    assert_eq!(info["layers"][0]["tileset_found"], true);
    assert_eq!(info["chunks"], 0);
    let e = err(
        &mut d,
        "scene.tiles.create",
        json!({ "file": file, "width": 1, "height": 1, "layers": [{ "name": "g" }] }),
    );
    assert!(e.message.contains("already exists"));

    // Carve: a 10x3 floor of rock across two chunks, in one transaction.
    let tx = ok(
        &mut d,
        "edit.begin",
        json!({ "author": "agent:claude", "note": "floor" }),
    )["tx"]
        .clone();
    ok(
        &mut d,
        "edit.tiles",
        json!({ "tx": tx, "file": file, "layer": "ground", "rect": { "x": 12, "y": 10, "w": 10, "h": 3 }, "terrain": "rock" }),
    );
    let c = ok(&mut d, "edit.commit", json!({ "tx": tx }));
    assert_eq!(
        c["ops"][0],
        json!(["terrain", file, "ground", [12, 10, 10, 3], "rock"])
    );
    assert_eq!(c["touched"], json!([format!("{file}#ground")]));
    let files = c["files"].as_array().unwrap();
    assert!(
        files
            .iter()
            .any(|f| f == "scenes/arena/room.chunks/ground/0_0.bin"),
        "{files:?}"
    );
    assert!(files
        .iter()
        .any(|f| f == "scenes/arena/room.chunks/ground/1_0.bin"));
    // Auto-tiled by N=1 E=2 S=4 W=8: the top-left corner has E and S (mask 6, tile 7),
    // the top edge E, S, W (14, tile 15), the interior everything (15, tile 16).
    let cells = ok(
        &mut d,
        "scene.tiles.get",
        json!({ "file": file, "rect": { "x": 12, "y": 10, "w": 3, "h": 2 } }),
    );
    assert_eq!(cells["cells"], json!([7, 15, 15, 8, 16, 16]));
    assert_eq!(cells["layers"], json!(["ground"]));
    // Collision as text: the top row is one-way (tile 7 is frame 6), the middle solid.
    let col = ok(
        &mut d,
        "scene.tiles.collision",
        json!({ "file": file, "rect": { "x": 11, "y": 10, "w": 4, "h": 2 } }),
    );
    assert_eq!(col["rows"], json!([".-..", "..##"]));

    // Raw cells and fills work the same way; every edit is undoable per author.
    let tx = ok(
        &mut d,
        "edit.begin",
        json!({ "author": "human:kevin", "note": "a crate" }),
    )["tx"]
        .clone();
    ok(
        &mut d,
        "edit.tiles",
        json!({ "tx": tx, "file": file, "layer": "ground", "rect": { "x": 0, "y": 0, "w": 2, "h": 1 }, "cells": [3, 4] }),
    );
    ok(&mut d, "edit.commit", json!({ "tx": tx }));
    ok(&mut d, "edit.undo", json!({ "author": "agent:claude" }));
    let after = ok(
        &mut d,
        "scene.tiles.get",
        json!({ "file": file, "rect": { "x": 12, "y": 10, "w": 3, "h": 1 } }),
    );
    assert_eq!(
        after["cells"],
        json!([0, 0, 0]),
        "the agent's floor is gone"
    );
    let kept = ok(
        &mut d,
        "scene.tiles.get",
        json!({ "file": file, "rect": { "x": 0, "y": 0, "w": 2, "h": 1 } }),
    );
    assert_eq!(
        kept["cells"],
        json!([3, 4]),
        "the human's edit is not the agent's to undo"
    );
    ok(&mut d, "edit.redo", json!({ "author": "agent:claude" }));
    let again = ok(
        &mut d,
        "scene.tiles.get",
        json!({ "file": file, "rect": { "x": 12, "y": 10, "w": 3, "h": 1 } }),
    );
    assert_eq!(again["cells"], json!([7, 15, 15]));
    // Bad edits are refused whole.
    let tx = ok(
        &mut d,
        "edit.begin",
        json!({ "author": "agent:claude", "note": "bad" }),
    )["tx"]
        .clone();
    ok(
        &mut d,
        "edit.tiles",
        json!({ "tx": tx, "file": file, "layer": "ground", "rect": { "x": 0, "y": 0, "w": 1, "h": 1 }, "terrain": "lava" }),
    );
    let e = err(&mut d, "edit.commit", json!({ "tx": tx }));
    assert_eq!(e.code, argus_tiles::codes::NO_TILESET);
    assert!(e.hint.contains("rock"), "{}", e.hint);
    let e = err(
        &mut d,
        "edit.tiles",
        json!({ "tx": 1, "file": file, "layer": "ground", "rect": { "x": 0, "y": 0, "w": 2, "h": 2 }, "cells": [1] }),
    );
    assert_eq!(e.code, argus_daemon::codes::BAD_PARAMS);

    // Placed in the world, the map draws: the camera sees the floor's left end.
    // "room" is the room document's name, so it is refused as an entity id.
    let tx = ok(
        &mut d,
        "edit.begin",
        json!({ "author": "agent:claude", "note": "place the map" }),
    )["tx"]
        .clone();
    let placed = json!({ "Transform": { "pos": [0.0, 0.0] }, "Tilemap": { "map": file } });
    ok(
        &mut d,
        "edit.spawn",
        json!({ "tx": tx, "id": "room", "components": placed }),
    );
    let e = err(&mut d, "edit.commit", json!({ "tx": tx }));
    assert!(e.message.contains("reserved"), "{}", e.message);
    let tx = ok(
        &mut d,
        "edit.begin",
        json!({ "author": "agent:claude", "note": "place the map" }),
    )["tx"]
        .clone();
    ok(
        &mut d,
        "edit.spawn",
        json!({ "tx": tx, "id": "floor", "components": placed }),
    );
    ok(&mut d, "edit.commit", json!({ "tx": tx }));
    let frame = |d: &mut argus_daemon::Daemon| {
        if let Ok(f) = d.call(0, "obs.frame", &json!({ "width": 160, "height": 80 })) {
            assert!(f["sprites"].as_u64().unwrap() > 0, "{f}");
            assert_eq!(f["missing_atlases"], json!([]), "{f}");
        }
    };
    frame(&mut d);
    // The map's index sits in the scene folder beside the entity files; a
    // reload skips it (it is data, not an entity) and draws the same.
    let s = ok(&mut d, "scene.load", json!({}));
    assert!(s.get("issues").is_none_or(|i| i == &json!([])), "{s}");
    let got = ok(&mut d, "world.get", json!({ "entity": "@floor" }));
    assert_eq!(got["components"]["Tilemap"]["map"], file, "{got}");
    frame(&mut d);
}
