//! Phase 4 item 7: tilemap painting, driven headless over a real socket on
//! a copy of the fixture project with a map the agent carved. The person
//! paints in the viewport with the pencil (one stroke, one change, one
//! undo), fills a rectangle with an auto-tiling terrain, erases, picks a
//! tile off the map, sees what bodies hit, and makes a new map; the agent's
//! carving and the person's strokes land in the same chunk (§12).

#[path = "../../argus-daemon/tests/common/mod.rs"]
mod common;

use argus_client::Client;
use argus_daemon::{serve, socket_name_for, Transport};
use argus_editor::tiles::{Brush, Tool};
use argus_editor::Editor;
use eframe::egui::{self, Event, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use serde_json::{json, Value as Json};
use std::path::Path;
use std::time::{Duration, Instant};

const TILESET: &str = r#"grid = [4, 4]

[[frames]]
index = 15
properties = { solid = true }

[autotile.rock]
mode = "4bit"
tiles = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
"#;

const FILE: &str = "scenes/arena/room.tilemap.toml";

fn until(h: &mut Harness<'_, Editor>, what: &str, ok: impl Fn(&Editor) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        h.step();
        if ok(h.state()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "timed out waiting for {what}; problem {:?}; maps {:?}",
        h.state().problem,
        h.state().tiles.maps
    );
}

fn result(c: &mut Client, method: &str, params: Json) -> Json {
    let r = c.call(method, params).unwrap();
    assert!(r.get("error").is_none(), "{method}: {r}");
    r["result"].clone()
}

fn cells(c: &mut Client, x: i64, y: i64, w: i64, h: i64) -> Vec<u64> {
    result(
        c,
        "scene.tiles.get",
        json!({ "file": FILE, "layer": "ground", "rect": { "x": x, "y": y, "w": w, "h": h } }),
    )["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect()
}

fn at(h: &Harness<'_, Editor>, cell: [i64; 2]) -> Pos2 {
    let m = h.state().mapping().expect("a frame with its view");
    h.state()
        .tiles
        .screen_of_cell(&m, cell)
        .expect("on the map")
}

/// Press on one cell, move through the cells between in steps, let go.
fn stroke(h: &mut Harness<'_, Editor>, from: [i64; 2], to: [i64; 2]) {
    let (a, b) = (at(h, from), at(h, to));
    let button = |h: &mut Harness<'_, Editor>, p: Pos2, pressed: bool| {
        h.input_mut().events.push(Event::PointerMoved(p));
        h.input_mut().events.push(Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    };
    button(h, a, true);
    for i in 1..=6 {
        let p = a + (b - a) * (i as f32 / 6.0);
        h.input_mut().events.push(Event::PointerMoved(p));
        h.step();
    }
    button(h, b, false);
    h.step();
}

fn last_commit(c: &mut Client) -> Json {
    let feed = result(c, "edit.feed", json!({ "limit": 200 }));
    feed["rows"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|r| r["kind"] == "commit")
        .cloned()
        .unwrap()
}

#[test]
fn a_person_paints_the_map_the_agent_carved_and_both_land_in_one_chunk() {
    let t = common::temp("editor-tiles");
    common::project(t.path());
    // A 16 x 16 sheet of sixteen 4-pixel tiles, each its own colour.
    let mut px = Vec::new();
    for y in 0..16u32 {
        for x in 0..16u32 {
            let i = (y / 4) * 4 + x / 4;
            px.extend_from_slice(&[(i * 15) as u8, 200 - (i * 10) as u8, 90, 255]);
        }
    }
    std::fs::create_dir_all(t.path().join("assets/tiles")).unwrap();
    {
        let f = std::io::BufWriter::new(
            std::fs::File::create(t.path().join("assets/tiles/rock.png")).unwrap(),
        );
        let mut enc = png::Encoder::new(f, 16, 16);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&px).unwrap();
    }
    common::write(t.path(), "assets/tiles/rock.toml", TILESET);
    common::write(
        t.path(),
        "scenes/arena/camera.toml",
        "[Transform]\npos = [8.0, -4.0]\n\n[Camera]\nheight = 10.0\n",
    );
    let name = format!("{}-tiles", socket_name_for(t.path()));
    let (r, n) = (t.path().to_path_buf(), name.clone());
    std::thread::spawn(move || {
        let d = common::open(&r);
        let _ = serve(d, Transport::Socket(n), Some(&r));
    });
    let mut agent = loop {
        if let Ok(c) = Client::connect(&name) {
            break c;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    result(
        &mut agent,
        "session.identify",
        json!({ "author": "agent:claude" }),
    );
    // The agent makes a map, places it, and carves a floor at the rect level.
    result(
        &mut agent,
        "scene.tiles.create",
        json!({ "file": FILE, "width": 32, "height": 16, "chunk": 16,
                "layers": [{ "name": "ground", "tileset": "tiles/rock", "collision": true }] }),
    );
    let tx = result(
        &mut agent,
        "edit.begin",
        json!({ "author": "agent:claude", "note": "a room and its floor" }),
    )["tx"]
        .clone();
    result(
        &mut agent,
        "edit.spawn",
        json!({ "tx": tx, "id": "cave", "components": { "Tilemap": { "map": FILE }, "Transform": { "pos": [0.0, 0.0] } } }),
    );
    result(&mut agent, "edit.commit", json!({ "tx": tx }));
    let tx = result(
        &mut agent,
        "edit.begin",
        json!({ "author": "agent:claude", "note": "floor" }),
    )["tx"]
        .clone();
    result(
        &mut agent,
        "edit.tiles",
        json!({ "tx": tx, "file": FILE, "layer": "ground", "rect": { "x": 2, "y": 6, "w": 9, "h": 2 }, "terrain": "rock" }),
    );
    let carved = result(&mut agent, "edit.commit", json!({ "tx": tx }));
    let chunk = "scenes/arena/room.chunks/ground/0_0.bin";
    assert!(
        carved["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == chunk),
        "{carved}"
    );

    let mut h = Harness::builder()
        .with_size(egui::vec2(1500.0, 950.0))
        .wgpu()
        .build_state(|ctx, ed: &mut Editor| ed.ui(ctx), {
            let mut ed = Editor::new(&name, "human");
            ed.audio = false;
            ed
        });
    until(&mut h, "frames with their view", |ed| {
        ed.viewport.view.is_some()
    });
    h.get_by_label("Tiles").click();
    until(&mut h, "the map and its tileset", |ed| {
        ed.tiles.current().is_some_and(|m| m["entity"] == "@cave")
            && ed.tiles.tileset_frames("tiles/rock") == 16
            && ed.tiles.layer.as_deref() == Some("ground")
    });
    h.step();
    assert!(h.state().viewport.paint, "the pointer paints");

    // The pencil with tile 16: a stroke across four cells, one change.
    h.get_by_label("tile 16").click();
    h.step();
    assert_eq!(h.state().tiles.brush, Brush::Tile(16));
    stroke(&mut h, [3, 2], [6, 2]);
    until(&mut h, "the stroke landed", |ed| {
        ed.tiles.pending.is_none() && ed.tiles.stroke.is_empty()
    });
    assert_eq!(cells(&mut agent, 3, 2, 4, 1), vec![16, 16, 16, 16]);
    let c = last_commit(&mut agent);
    assert_eq!(c["author"], "human");
    assert_eq!(c["note"], "painted 4 cells of ground with tile 16");
    assert!(
        c["files"].as_array().unwrap().iter().any(|f| f == chunk),
        "the agent's chunk: {c}"
    );
    assert!(
        c["command"].as_str().unwrap().starts_with(
            "argus edit author=human note='painted 4 cells of ground with tile 16' ops='[{"
        ),
        "{}",
        c["command"]
    );
    // Undo is the person's: the stroke goes, the agent's floor stays.
    h.press_key_modifiers(Modifiers::COMMAND, egui::Key::Z);
    let deadline = Instant::now() + Duration::from_secs(10);
    while cells(&mut agent, 3, 2, 4, 1) != vec![0, 0, 0, 0] && Instant::now() < deadline {
        h.step();
    }
    assert_eq!(cells(&mut agent, 3, 2, 4, 1), vec![0, 0, 0, 0]);
    assert!(
        cells(&mut agent, 2, 6, 9, 2).iter().all(|t| *t > 0),
        "the agent's floor"
    );

    // The rectangle with the terrain: auto-tiled, edges and middle.
    h.get_by_label("Rectangle").click();
    h.get_by_label("rock").click();
    h.step();
    stroke(&mut h, [4, 1], [8, 3]);
    until(&mut h, "the rectangle landed", |ed| {
        ed.tiles.pending.is_none()
    });
    let rect = cells(&mut agent, 4, 1, 5, 3);
    assert!(rect.iter().all(|t| *t > 0), "{rect:?}");
    assert_eq!(rect[7], 16, "the middle is surrounded: {rect:?}");
    assert_ne!(rect[0], 16, "a corner is not: {rect:?}");
    assert!(last_commit(&mut agent)["note"]
        .as_str()
        .unwrap()
        .starts_with("painted 15 cells of ground with terrain rock"));

    // What bodies hit: the solid middle.
    h.get_by_label("What bodies hit").click();
    until(&mut h, "the collision rows", |ed| {
        ed.tiles.collision["rows"].as_array().is_some_and(|r| {
            r.iter()
                .any(|s| s.as_str().is_some_and(|s| s.contains('#')))
        })
    });

    // The eraser, with the pencil.
    h.get_by_label("Pencil").click();
    h.get_by_label("Eraser").click();
    h.step();
    stroke(&mut h, [4, 1], [5, 1]);
    until(&mut h, "erased", |ed| ed.tiles.pending.is_none());
    assert_eq!(cells(&mut agent, 4, 1, 2, 1), vec![0, 0]);

    // Pick: the agent's tile is the brush.
    let theirs = cells(&mut agent, 5, 6, 1, 1)[0];
    h.get_by_label("Pick").click();
    h.step();
    stroke(&mut h, [5, 6], [5, 6]);
    until(&mut h, "picked", |ed| ed.tiles.brush == Brush::Tile(theirs));
    assert_eq!(h.state().tiles.tool, Tool::Pencil);

    // The cell under the pointer says what is in it.
    let p = at(&h, [4, 2]);
    h.input_mut().events.push(Event::PointerMoved(p));
    until(&mut h, "the cell under the pointer", |ed| {
        ed.tiles.hover == Some([4, 2]) && ed.tiles.hover_tile.is_some()
    });
    save(&mut h, "editor-tiles.png");

    // A new map, placed at the view's corner, by the person.
    until(&mut h, "the tilesets", |ed| {
        ed.tiles.sprites.iter().any(|s| s == "tiles/rock")
    });
    h.state_mut().tiles.new_map.file = "scenes/arena/deco.tilemap.toml".into();
    h.state_mut().tiles.new_map.tileset = "tiles/rock".into();
    h.state_mut().tiles.new_map.width = 8;
    h.state_mut().tiles.new_map.height = 4;
    h.step();
    h.get_by_label("New tilemap").click();
    h.step();
    h.get_by_label("Create, its corner at the view's top left")
        .click();
    until(&mut h, "the new map placed", |ed| {
        ed.tiles
            .maps
            .iter()
            .any(|m| m["file"] == "scenes/arena/deco.tilemap.toml" && m["entity"] == "@deco")
    });
    assert!(t.path().join("scenes/arena/deco.tilemap.toml").is_file());
    assert!(
        t.path().join("scenes/arena/deco.toml").is_file(),
        "its entity's file"
    );
    let c = last_commit(&mut agent);
    assert_eq!(
        (c["author"].clone(), c["note"].clone()),
        (
            json!("human"),
            json!("a new tilemap, scenes/arena/deco.tilemap.toml")
        )
    );
}

fn save(h: &mut Harness<'_, Editor>, name: &str) {
    h.step();
    let img = h.render().expect("drawn");
    let (w, hh) = img.dimensions();
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let f = std::io::BufWriter::new(std::fs::File::create(&out).unwrap());
    let mut enc = png::Encoder::new(f, w, hh);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&img.into_raw())
        .unwrap();
}
