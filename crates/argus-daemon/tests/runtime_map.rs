//! Phase 5 item 4: a runtime map, whose layers are authored and whose cells
//! are the run's: a game's script writes them (`q:tiles`, Delve's floors
//! from the seed), the world reads them as any map's (collision, the
//! semantic frame), and a seek, a replay, and a save carry them, so a run
//! stays a value (§6). Nothing writes them to disk.

mod common;

use common::*;
use serde_json::{json, Value as Json};

const MAP: &str = "kind = \"tilemap\"\nwidth = 8\nheight = 2\ntile_size = 1.0\nchunk = 4\ncell = 1\nchunks = \"floor.chunks\"\nhash = \"\"\nruntime = true\n\n[[layers]]\nname = \"walls\"\nz = 0.0\ncollision = true\n";

/// Every tick a wall at a column the run's random stream picks, row 0.
const CARVE: &str = r#"return system "sim.carve" {
  stage = "sim",
  reads = { "Tilemap" },
  writes = { "Tilemap" },
  rng = "carve",
  doc = "Walls in one cell of the top row a tick, at a column the seed picks.",
  run = function(q)
    local t = q:tiles("@floor")
    local w, _ = t:size()
    t:set("walls", q:rng():range(0, w - 1), 0, 1)
  end,
}
"#;

fn project_with_floor(root: &std::path::Path) {
    project(root);
    write(root, "maps/floor.tilemap.toml", MAP);
    write(
        root,
        "scenes/arena/floor.toml",
        "[Transform]\npos = [0.0, 2.0]\n\n[Tilemap]\nmap = \"maps/floor.tilemap.toml\"\n",
    );
    write(root, "systems/carve.luau", CARVE);
}

fn rows(d: &mut argus_daemon::Daemon) -> Json {
    ok(
        d,
        "scene.tiles.collision",
        json!({ "file": "maps/floor.tilemap.toml", "rect": { "x": 0, "y": 0, "w": 8, "h": 2 } }),
    )["rows"]
        .clone()
}

#[test]
fn a_runtime_map_is_the_runs_and_a_seek_a_replay_and_a_save_keep_it() {
    let t = temp("runtime-map");
    project_with_floor(t.path());
    let mut d = open(t.path());
    assert_eq!(
        rows(&mut d),
        json!(["........", "........"]),
        "empty at the start"
    );
    ok(&mut d, "sim.play", json!({}));
    ok(&mut d, "sim.pause", json!({}));
    ok(&mut d, "sim.step", json!({ "n": 3 }));
    let at3 = rows(&mut d);
    ok(&mut d, "sim.step", json!({ "n": 7 }));
    let at10 = rows(&mut d);
    let walls = |r: &Json| r[0].as_str().unwrap().matches('#').count();
    assert!(
        walls(&at10) >= walls(&at3) && walls(&at3) >= 1,
        "{at3} {at10}"
    );
    assert_eq!(at10[1], "........", "only the top row");
    // scene.tiles.get reads the run's cells too, as the docs say (issue 9).
    let got = ok(
        &mut d,
        "scene.tiles.get",
        json!({ "file": "maps/floor.tilemap.toml", "layer": "walls", "rect": { "x": 0, "y": 0, "w": 8, "h": 1 } }),
    );
    let set = got["cells"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c.as_u64() == Some(1))
        .count();
    assert_eq!(set, walls(&at10), "{got}");

    // The semantic frame reads it as the world's map.
    let f = ok(
        &mut d,
        "obs.frame",
        json!({ "format": "semantic", "width": 320, "height": 180 }),
    );
    assert!(
        f["tiles"].to_string().contains('#'),
        "the frame's tiles: {}",
        f["tiles"]
    );

    // Back to tick 3: the walls the run had then; on to 10: the same as before.
    ok(&mut d, "sim.seek", json!({ "frame": 3 }));
    assert_eq!(rows(&mut d), at3);
    ok(&mut d, "sim.step", json!({ "n": 7 }));
    assert_eq!(rows(&mut d), at10);

    // A save keeps them; a load puts them back over whatever is there.
    ok(&mut d, "sim.save", json!({ "name": "carved" }));
    ok(&mut d, "sim.step", json!({ "n": 20 }));
    assert_ne!(rows(&mut d), at10);
    ok(&mut d, "sim.load", json!({ "name": "carved" }));
    assert_eq!(rows(&mut d), at10);

    // Nothing on disk: no chunk folder, and the index as it was.
    assert!(!t.path().join("maps/floor.chunks").exists());
    assert_eq!(read(t.path(), "maps/floor.tilemap.toml"), MAP);
    // An edit of a runtime map is refused at commit: its tiles are the run's.
    let tx = ok(&mut d, "edit.begin", json!({ "author": "agent:claude" }))["tx"].clone();
    ok(
        &mut d,
        "edit.tiles",
        json!({ "tx": tx, "file": "maps/floor.tilemap.toml", "layer": "walls", "rect": { "x": 0, "y": 1, "w": 1, "h": 1 }, "tile": 1 }),
    );
    let e = err(&mut d, "edit.commit", json!({ "tx": tx }));
    assert!(e.message.contains("the run's"), "{}", e.message);
    assert_eq!(rows(&mut d), at10, "nothing kept");
}

/// A cell from a world point and back, a read off the map's edge, and a
/// run's digest that tells two floors apart (Delve found it did not).
#[test]
fn a_map_answers_in_world_places_and_the_runs_digest_holds_its_cells() {
    let t = temp("runtime-map-places");
    project_with_floor(t.path());
    write(
        t.path(),
        "systems/probe.luau",
        r#"return system "sim.probe" {
  stage = "sim",
  reads = { "Tilemap", "Transform" },
  doc = "Says where the player is on the floor, and what is off its edge.",
  run = function(q)
    local t = q:tiles("@floor")
    local x, y = t:cell(q:entity("@player"))
    local c = t:center(3, 1)
    q:emit("probe", { x = x, y = y, cx = c.x, cy = c.y, off = t:get("walls", -1, 0) })
  end,
}
"#,
    );
    let mut d = open(t.path());
    ok(&mut d, "sim.step", json!({ "n": 1 }));
    let ev = ok(&mut d, "obs.events", json!({ "name": "probe" }));
    let f = &ev["rows"][0]["fields"];
    // The floor's top-left is at (0, 2), a cell a unit: the player at the
    // origin is in column 0, row 2 (below its two rows).
    assert_eq!(
        (f["x"].as_f64(), f["y"].as_f64()),
        (Some(0.0), Some(2.0)),
        "{ev}"
    );
    assert_eq!(
        (f["cx"].as_f64(), f["cy"].as_f64()),
        (Some(3.5), Some(0.5)),
        "{ev}"
    );
    assert_eq!(f["off"], 0, "{ev}");

    // The run's digest moves with the cells, and a seek's verify sees them.
    let digest = |d: &mut argus_daemon::Daemon| {
        ok(d, "sim.status", json!({ "digest": true }))["digest"].clone()
    };
    ok(&mut d, "sim.play", json!({}));
    ok(&mut d, "sim.pause", json!({}));
    ok(&mut d, "sim.step", json!({ "n": 130 }));
    let at = ok(&mut d, "sim.status", json!({}))["tick"]
        .as_u64()
        .unwrap();
    let full = digest(&mut d);
    // The keyframe a second back, replayed from the one before it: the
    // world and the cells (keyframes come a second apart here).
    let v = ok(
        &mut d,
        "sim.seek",
        json!({ "frame": at - 5, "verify": true }),
    );
    assert_eq!(v["verified"]["ok"], true, "{v}");
    assert_eq!(v["verified"]["tiles_same"], true, "{v}");
    ok(&mut d, "sim.seek", json!({ "frame": 3 }));
    assert_ne!(digest(&mut d), full, "fewer walls, another digest");
    ok(&mut d, "sim.seek", json!({ "frame": at }));
    assert_eq!(digest(&mut d), full);
}

#[test]
fn a_script_writes_only_a_runtime_maps_tiles_and_only_when_it_says_so() {
    // Without Tilemap in its writes: refused, naming the list.
    let t = temp("runtime-map-undeclared");
    project_with_floor(t.path());
    write(
        t.path(),
        "systems/carve.luau",
        &CARVE.replace("  writes = { \"Tilemap\" },\n", ""),
    );
    let mut d = open(t.path());
    ok(&mut d, "sim.step", json!({ "n": 1 }));
    let errs = ok(&mut d, "script.errors", json!({}));
    assert!(errs.to_string().contains("without declaring it"), "{errs}");
    assert_eq!(rows(&mut d), json!(["........", "........"]));

    // The project's own map: refused, whatever the declarations.
    let t = temp("runtime-map-authored");
    project_with_floor(t.path());
    write(
        t.path(),
        "maps/floor.tilemap.toml",
        // An empty manifest's hash: no chunk folder yet.
        &MAP.replace("runtime = true\n", "")
            .replace("hash = \"\"", "hash = \"fnv1a64:cbf29ce484222325\""),
    );
    let mut d = open(t.path());
    ok(&mut d, "sim.step", json!({ "n": 1 }));
    let errs = ok(&mut d, "script.errors", json!({}));
    assert!(errs.to_string().contains("the project's map"), "{errs}");
}

/// Issue 19: a block of cells copied between maps and layers in one call,
/// as a game keeps a room on a storage map and shows it on the drawn one.
#[test]
fn map_copy_moves_a_block_of_cells_between_maps() {
    let t = temp("runtime-map-copy");
    project(t.path());
    write(t.path(), "maps/floor.tilemap.toml", MAP);
    write(
        t.path(),
        "maps/store.tilemap.toml",
        &MAP.replace("floor.chunks", "store.chunks"),
    );
    for (id, file) in [("floor", "floor"), ("store", "store")] {
        write(
            t.path(),
            &format!("scenes/arena/{id}.toml"),
            &format!(
                "[Transform]\npos = [0.0, 2.0]\n\n[Tilemap]\nmap = \"maps/{file}.tilemap.toml\"\n"
            ),
        );
    }
    write(
        t.path(),
        "systems/rooms.luau",
        r#"return system "sim.rooms" {
  stage = "sim",
  reads = { "Tilemap" },
  writes = { "Tilemap" },
  doc = "Keeps a room on the store and shows it on the floor, offset by two.",
  run = function(q)
    if q.tick ~= 0 then return end
    local store, floor = q:tiles("@store"), q:tiles("@floor")
    store:set("walls", 0, 0, 7)
    store:set("walls", 1, 1, 9)
    store:copy("walls", 0, 0, 3, 2, floor, nil, 2, 0)
    -- Off the store's right edge copies as empty.
    floor:set("walls", 0, 1, 5)
    store:copy("walls", 7, 1, 2, 1, floor, "walls", 0, 1)
  end,
}
"#,
    );
    let mut d = open(t.path());
    ok(&mut d, "sim.step", json!({ "n": 1 }));
    let cells = ok(
        &mut d,
        "scene.tiles.get",
        json!({ "file": "maps/floor.tilemap.toml", "layer": "walls", "rect": { "x": 0, "y": 0, "w": 8, "h": 2 } }),
    )["cells"]
        .clone();
    assert_eq!(
        cells,
        json!([0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0]),
        "the block at column 2, and the empty pair over the 5"
    );
}

/// Issue 25: a scenario's setup sets a runtime map's cells in the run, which
/// edit.tiles refuses, and says what it wants when the cells do not fit.
#[test]
fn sim_tiles_sets_a_runtime_maps_cells_and_names_a_wrong_count() {
    let t = temp("runtime-map-sim-tiles");
    project_with_floor(t.path());
    let mut d = open(t.path());
    ok(
        &mut d,
        "sim.tiles",
        json!({ "file": "maps/floor.tilemap.toml", "layer": "walls", "rect": { "x": 5, "y": 1, "w": 2, "h": 1 }, "tile": 1 }),
    );
    ok(
        &mut d,
        "sim.tiles",
        json!({ "file": "maps/floor.tilemap.toml", "layer": "walls", "rect": { "x": 0, "y": 1, "w": 2, "h": 1 }, "cells": [1, 0] }),
    );
    assert_eq!(rows(&mut d)[1], "#....##.");
    let e = err(
        &mut d,
        "sim.tiles",
        json!({ "file": "maps/floor.tilemap.toml", "layer": "walls", "rect": { "x": 0, "y": 0, "w": 2, "h": 1 }, "cells": [1] }),
    );
    assert!(e.message.contains("2 cells"), "{}", e.message);
}
