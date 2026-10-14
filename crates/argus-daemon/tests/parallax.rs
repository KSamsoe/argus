//! 2D staples item 3: parallax. A sprite with a Parallax and a tile layer
//! with a factor are drawn shifted by the camera's distance from their
//! origin, and the semantic frame puts the sprite where it is drawn.

mod common;

use common::*;
use serde_json::json;
use std::path::Path;

fn project(root: &Path) {
    write(
        root,
        "argus.toml",
        "name = \"parallax\"\nclock = \"continuous@60\"\ndefault_scene = \"main\"\nplugins = []\n",
    );
    argus_assets::encode_png(
        &root.join("assets/tiles/blue.png"),
        4,
        4,
        &[0, 0, 255, 255].repeat(16),
    )
    .unwrap();
    write(root, "assets/tiles/blue.toml", "grid = [1, 1]\n");
    write(
        root,
        "scenes/main/scene.toml",
        "clock = \"continuous@60\"\nseed = 1\n",
    );
    // A camera that stays at x 20, 20 units tall: 18 pixels a unit in a
    // 640 x 360 frame, x = 320 + 18 (wx - 20), y = 180 - 18 wy.
    write(
        root,
        "scenes/main/camera.toml",
        "[Transform]\npos = [20.0, 0.0]\n\n[Camera]\nheight = 20.0\n",
    );
    // A far hill at the origin, half as fast: drawn 10 units right.
    write(
        root,
        "scenes/main/hill.toml",
        "[Transform]\npos = [0.0, 0.0]\n\n[Sprite]\nkind = \"rect\"\nsize = [2.0, 2.0]\ncolor = \"#FF0000\"\nz = 1.0\n\n[Parallax]\nfactor = [0.5, 0.5]\n",
    );
    write(
        root,
        "scenes/main/back.toml",
        "[Transform]\npos = [0.0, 10.0]\n\n[Tilemap]\nmap = \"maps/back.tilemap.toml\"\n",
    );
}

#[test]
fn a_far_sprite_and_a_far_layer_are_drawn_where_the_camera_says() {
    let t = temp("parallax");
    project(t.path());
    let mut d = open(t.path());
    let file = "maps/back.tilemap.toml";
    ok(
        &mut d,
        "scene.tiles.create",
        json!({ "file": file, "width": 40, "height": 20, "chunk": 20,
                "layers": [{ "name": "back", "tileset": "tiles/blue", "parallax": 0.5 }] }),
    );
    // One column of tiles, at x 10 to 11.
    commit(
        &mut d,
        "agent:claude",
        "a pillar",
        &[(
            "edit.tiles",
            json!({ "file": file, "layer": "back", "rect": { "x": 10, "y": 0, "w": 1, "h": 20 }, "tile": 1 }),
        )],
    );
    ok(&mut d, "scene.load", json!({ "scene": "main" }));

    // The hill is where it is drawn: 2 units wide about x 10.
    let f = ok(&mut d, "obs.frame", json!({ "format": "semantic" }));
    let rows = f["entities"].as_array().unwrap();
    let hill = rows.iter().find(|r| r[0] == "@hill").expect("listed");
    let b: Vec<i64> = hill[3]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    // Edges are floored and ceiled: a hair past 158 is 159.
    assert!(
        b[0] == 122 && (158..=159).contains(&b[2]) && b[1] == 162 && b[3] == 198,
        "{hill}"
    );

    if let Some(r) = frame(&mut d, json!({ "width": 640, "height": 360 })) {
        let (w, _, px) = argus_assets::decode_png(Path::new(r["path"].as_str().unwrap())).unwrap();
        let at = |x: u32, y: u32| {
            let i = ((y * w + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2]]
        };
        let near = |p: [u8; 3], q: [u8; 3]| p.iter().zip(q).all(|(a, b)| a.abs_diff(b) < 40);
        assert!(
            near(at(140, 180), [255, 0, 0]),
            "the hill: {:?}",
            at(140, 180)
        );
        // The pillar's centre, x 10.5, is drawn 10 units right: x 329.
        assert!(
            near(at(329, 180), [0, 0, 255]),
            "the pillar: {:?}",
            at(329, 180)
        );
        assert!(
            !near(at(149, 180), [0, 0, 255]),
            "not where its tiles are: {:?}",
            at(149, 180)
        );
    }
}
