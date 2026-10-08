//! Phase 5 item 4 through the protocol: a `Tilemap` naming a Tiled map
//! (`.tmx`) is the engine's map. A body lands on its solid tiles, it is
//! drawn (its hidden layer not), it is in the semantic frame's `tiles`,
//! `scene.tiles.info` and `asset.get` show what the file holds, an edit
//! is refused with the hint to edit it in Tiled, and a save in Tiled is
//! read again (§12, §14).

mod common;

use common::*;
use serde_json::{json, Value as Json};
use std::path::Path;

const RED: [u8; 4] = [200, 40, 40, 255];
const GREEN: [u8; 4] = [40, 200, 40, 255];
const BLUE: [u8; 4] = [40, 40, 200, 255];
const YELLOW: [u8; 4] = [230, 210, 30, 255];

/// Rock (solid), a plank (one-way), a moss tile (nothing), and a hidden
/// blocker (solid): 8-pixel tiles in a row.
const SET: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" name="cave" tilewidth="8" tileheight="8" tilecount="4" columns="4">
 <image source="cave.png" width="32" height="8"/>
 <tile id="0"><properties><property name="solid" type="bool" value="true"/></properties></tile>
 <tile id="1"><properties><property name="one_way" type="bool" value="true"/></properties></tile>
 <tile id="3"><properties><property name="solid" type="bool" value="true"/></properties></tile>
</tileset>
"#;

fn map(floor: u32) -> String {
    let f = floor;
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="6" height="4" tilewidth="8" tileheight="8" infinite="0">
 <tileset firstgid="1" source="../tiles/cave.tsx"/>
 <layer id="1" name="ground" width="6" height="4">
  <properties><property name="collision" type="bool" value="true"/></properties>
  <data encoding="csv">
3,0,0,0,0,0,
0,0,0,0,0,0,
0,0,0,0,0,0,
{f},{f},{f},{f},{f},{f}
</data>
 </layer>
 <layer id="2" name="blockers" width="6" height="4" visible="0">
  <properties><property name="collision" type="bool" value="true"/></properties>
  <data encoding="csv">
0,0,0,0,0,0,
0,0,0,0,0,4,
0,0,0,0,0,0,
0,0,0,0,0,0
</data>
 </layer>
 <objectgroup id="3" name="spawns">
  <object id="1" name="start" type="Spawn" x="20" y="12"><point/></object>
 </objectgroup>
</map>
"#
    )
}

fn platformer(root: &Path) {
    write(
        root,
        "argus.toml",
        "name = \"tiled\"\nclock = \"continuous@60\"\ndefault_scene = \"room\"\nplugins = []\n",
    );
    let mut px = Vec::new();
    for _ in 0..8 {
        for c in [RED, GREEN, BLUE, YELLOW] {
            px.extend(c.repeat(8));
        }
    }
    argus_assets::encode_png(&root.join("assets/tiles/cave.png"), 32, 8, &px).unwrap();
    write(root, "assets/tiles/cave.tsx", SET);
    write(root, "assets/maps/room.tmx", &map(1));
    write(
        root,
        "scenes/room/scene.toml",
        "clock = \"continuous@60\"\nseed = 1\n",
    );
    write(
        root,
        "scenes/room/camera.toml",
        "[Transform]\npos = [3.0, 2.0]\n\n[Camera]\nheight = 4.0\n",
    );
    write(
        root,
        "scenes/room/body.toml",
        "[Transform]\npos = [2.5, 3.0]\n\n[Collider]\nsize = [0.8, 0.8]\n\n[KinematicBody]\ngravity = 30.0\nmax_fall = 20.0\n",
    );
    write(
        root,
        "scenes/room/floor.toml",
        "[Transform]\npos = [0.0, 4.0]\n\n[Tilemap]\nmap = \"assets/maps/room.tmx\"\n",
    );
}

const FILE: &str = "assets/maps/room.tmx";

fn body(d: &mut argus_daemon::Daemon) -> Json {
    ok(d, "world.get", json!({ "entity": "@body" }))["components"].clone()
}

fn near(px: &[u8], c: [u8; 4]) -> bool {
    px.iter()
        .zip(c)
        .take(3)
        .all(|(a, b)| (*a as i32 - b as i32).abs() <= 12)
}

#[test]
fn a_tilemap_on_a_tmx_collides_draws_and_is_in_the_semantic_frame() {
    let t = temp("tiled-map");
    platformer(t.path());
    let mut d = open(t.path());

    // The import reported it, with its structure.
    let a = ok(&mut d, "asset.get", json!({ "name": "maps/room" }));
    assert_eq!(a["kind"], "tilemap");
    assert_eq!(a["layers"][1]["name"], "blockers");
    assert_eq!(a["layers"][1]["visible"], false);
    assert_eq!(
        a["object_layers"][0]["objects"][0]["pos"],
        json!([2.5, -1.5])
    );
    let l = ok(&mut d, "asset.list", json!({ "kind": "tilemap" }));
    assert_eq!(l["rows"][0][0], "maps/room", "{l}");

    // It falls onto the map's floor: the .tsx says tile 1 is solid.
    ok(&mut d, "sim.step", json!({ "n": 60 }));
    let b = body(&mut d);
    assert_eq!(b["KinematicBody"]["grounded"], true, "{b}");
    let feet = b["Transform"]["pos"][1].as_f64().unwrap() - 0.4;
    assert!((feet - 1.0).abs() < 1e-3, "{feet}");

    // The semantic frame's tiles: the floor, and the hidden blocker, which
    // collides though Tiled hides it.
    let f = ok(
        &mut d,
        "obs.frame",
        json!({ "format": "semantic", "width": 160, "height": 80 }),
    );
    let m = &f["tiles"]["maps"][0];
    assert_eq!(m["entity"], "@floor", "{f}");
    let rows: Vec<&str> = m["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap())
        .collect();
    assert_eq!(rows.last().copied(), Some("######"), "{rows:?}");
    assert!(rows.iter().any(|r| r.ends_with(".#")), "{rows:?}");

    // Drawn: the floor's red along the bottom, the moss tile at the top
    // left, and not the hidden blocker's yellow.
    if let Some(r) = frame(&mut d, json!({ "width": 160, "height": 80 })) {
        assert_eq!(r["missing_atlases"], json!([]), "{r}");
        let (w, _, px) = argus_assets::decode_png(Path::new(r["path"].as_str().unwrap())).unwrap();
        let at = |x: u32, y: u32| &px[((y * w + x) * 4) as usize..((y * w + x) * 4 + 4) as usize];
        // 20 px a unit; the map's left edge is at x 20, its top at y 0.
        assert!(near(at(30, 70), RED), "{:?}", at(30, 70));
        assert!(near(at(130, 70), RED), "{:?}", at(130, 70));
        assert!(near(at(30, 10), BLUE), "{:?}", at(30, 10));
        let yellow = px.chunks(4).filter(|p| near(p, YELLOW)).count();
        assert_eq!(yellow, 0, "the hidden layer is not drawn");
    }

    // What the protocol says of the map: Tiled's, and what its file holds.
    let info = ok(&mut d, "scene.tiles.info", json!({ "file": FILE }));
    assert_eq!(
        (info["width"].clone(), info["height"].clone()),
        (json!(6), json!(4))
    );
    assert_eq!(info["layers"][0]["tileset"], "tiles/cave");
    assert_eq!(info["layers"][0]["tileset_found"], true);
    assert_eq!(info["layers"][1]["visible"], false);
    assert_eq!(info["tiled"]["map"]["tilesets"][0]["firstgid"], 1);
    assert!(info["tiled"]["read_only"]
        .as_str()
        .unwrap()
        .contains("edit it in Tiled"));
    let cells = ok(
        &mut d,
        "scene.tiles.get",
        json!({ "file": FILE, "rect": { "x": 0, "y": 3, "w": 6, "h": 1 } }),
    );
    assert_eq!(cells["cells"], json!([1, 1, 1, 1, 1, 1]));
    assert_eq!(cells["layers"], json!(["ground", "blockers"]));
    let listed = ok(&mut d, "scene.tiles.list", json!({}));
    assert_eq!(listed["rows"][0]["file"], FILE);
    assert!(listed["rows"][0]["tiled"].is_object());

    // An edit is refused, with where to make it.
    let tx = ok(
        &mut d,
        "edit.begin",
        json!({ "author": "agent:claude", "note": "dig" }),
    )["tx"]
        .clone();
    let e = err(
        &mut d,
        "edit.tiles",
        json!({ "tx": tx, "file": FILE, "layer": "ground", "rect": { "x": 0, "y": 3, "w": 1, "h": 1 }, "tile": 0 }),
    );
    assert!(
        e.hint.contains("This map is Tiled's: edit it in Tiled"),
        "{}",
        e.hint
    );
    ok(&mut d, "edit.abort", json!({ "tx": tx }));
    assert_eq!(read(t.path(), FILE), map(1), "the file is as Tiled left it");

    // Saved in Tiled with a floor of planks: read again, one-way now.
    write(t.path(), FILE, &map(2));
    let file = std::fs::File::options()
        .write(true)
        .open(t.path().join(FILE))
        .unwrap();
    file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5))
        .unwrap();
    drop(file);
    let col = ok(
        &mut d,
        "scene.tiles.collision",
        json!({ "file": FILE, "rect": { "x": 0, "y": 3, "w": 6, "h": 1 } }),
    );
    assert_eq!(col["rows"], json!(["------"]));
    let imported = ok(&mut d, "asset.import", json!({}));
    assert!(
        imported["imported"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n == "maps/room"),
        "{imported}"
    );
}
