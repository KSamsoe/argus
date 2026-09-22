//! §12 Tilemaps: layers over a grid, auto-tiling rules in TOML, collision
//! from tile properties, chunked storage (§4), rect edits; §17 4,096 x
//! 4,096 tiles and 8 layers without pauses.

use argus_assets::{encode_png, Importer};
use argus_tiles::{blob_masks, collision, paint, visible, Collide, LayerDef, MapFile, Rect, Rules};
use std::path::{Path, PathBuf};
use std::time::Instant;

fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("argus-tiles-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets/tiles")).unwrap();
    dir
}

fn layer(name: &str, tileset: Option<&str>, collision: bool) -> LayerDef {
    LayerDef {
        name: name.into(),
        tileset: tileset.map(str::to_string),
        z: 0.0,
        collision,
    }
}

/// A tileset of `cols x rows` 4-pixel frames with a sidecar.
fn tileset(root: &Path, name: &str, cols: u32, rows: u32, sidecar: &str) -> Importer {
    let (w, h) = (cols * 4, rows * 4);
    encode_png(
        &root.join(format!("assets/tiles/{name}.png")),
        w,
        h,
        &[200, 120, 60, 255].repeat((w * h) as usize),
    )
    .unwrap();
    std::fs::write(
        root.join(format!("assets/tiles/{name}.toml")),
        format!("grid = [{cols}, {rows}]\n{sidecar}"),
    )
    .unwrap();
    let mut imp = Importer::new(root);
    let r = imp.scan();
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    imp
}

fn sprite<'a>(imp: &'a Importer, name: &str) -> &'a argus_assets::SpriteAsset {
    imp.sprites().find(|(n, _)| *n == name).unwrap().1
}

#[test]
fn chunks_are_sparse_lazy_saved_incrementally_and_checked() {
    let dir = project("chunks");
    let rel = "scenes/a/tiles.tilemap.toml";
    let mut m = MapFile::create(
        &dir,
        rel,
        256,
        256,
        1.0,
        64,
        2,
        vec![layer("ground", None, true), layer("deco", None, false)],
    )
    .unwrap();
    let text = std::fs::read_to_string(dir.join(rel)).unwrap();
    assert!(
        text.contains("kind = \"tilemap\"") && text.contains("chunks = \"tiles.chunks\""),
        "{text}"
    );
    let g = m.layer("ground").unwrap();
    // A rect across four chunks.
    let r = Rect::new(60, 60, 8, 8);
    m.fill(g, r, 7).unwrap();
    assert_eq!(m.dirty(), 4);
    let rep = m.save().unwrap();
    assert_eq!(rep.written, 4, "only the chunks that hold tiles");
    assert!(
        rep.files
            .iter()
            .any(|f| f == "scenes/a/tiles.chunks/ground/0_0.bin"),
        "{:?}",
        rep.files
    );
    assert_eq!(m.chunk_count(), 4);
    assert!(
        !dir.join("scenes/a/tiles.chunks/deco").exists(),
        "an empty layer writes nothing"
    );

    // Reopen: nothing is read until asked, then the same cells.
    let mut m = MapFile::open(&dir, rel).unwrap();
    assert_eq!(m.loaded(), 0);
    assert_eq!(
        m.get(g, Rect::new(59, 59, 3, 3)).unwrap(),
        vec![0, 0, 0, 0, 7, 7, 0, 7, 7]
    );
    assert_eq!(m.cell(g, 200, 200).unwrap(), 0);
    assert_eq!(m.loaded(), 1, "reading an empty chunk loads nothing");
    // One cell changes: one chunk is rewritten. A comment written in the
    // index survives the save; only the hash line changes.
    let text = std::fs::read_to_string(dir.join(rel)).unwrap();
    std::fs::write(dir.join(rel), format!("# the cavern floor\n{text}")).unwrap();
    m.fill(g, Rect::new(61, 61, 1, 1), 9).unwrap();
    assert_eq!(m.save().unwrap().written, 1);
    let after = std::fs::read_to_string(dir.join(rel)).unwrap();
    assert!(after.starts_with("# the cavern floor\n"), "{after}");
    let changed: Vec<_> = text
        .lines()
        .zip(after.lines().skip(1))
        .filter(|(a, b)| a != b)
        .collect();
    assert!(
        changed.len() == 1 && changed[0].0.starts_with("hash = "),
        "{changed:?}"
    );
    assert_eq!(
        MapFile::open(&dir, rel).unwrap().cell(g, 61, 61).unwrap(),
        9
    );
    // Erasing a chunk's last tiles deletes its file.
    m.fill(g, Rect::new(64, 64, 4, 4), 0).unwrap();
    let rep = m.save().unwrap();
    assert_eq!((rep.written, rep.removed), (0, 1));
    assert!(!dir.join("scenes/a/tiles.chunks/ground/1_1.bin").exists());

    // Errors say what and where.
    assert_eq!(
        m.get(g, Rect::new(250, 0, 10, 1)).unwrap_err().code,
        argus_ecs::codes::OUT_OF_RANGE
    );
    let e = m.fill(g, Rect::new(0, 0, 1, 1), 70_000).unwrap_err();
    assert!(e.message.contains("does not fit a 2-byte cell"), "{e}");
    assert!(m.layer("sky").unwrap_err().hint.contains("ground, deco"));

    // A chunk edited outside the tool is caught when read; a manifest, on open.
    std::fs::write(
        dir.join("scenes/a/tiles.chunks/ground/0_0.bin"),
        vec![1u8; 64 * 64 * 2],
    )
    .unwrap();
    let mut m = MapFile::open(&dir, rel).unwrap();
    assert_eq!(
        m.cell(g, 0, 0).unwrap_err().code,
        argus_tiles::codes::MAP_MISMATCH
    );
    std::fs::write(dir.join("scenes/a/tiles.chunks/manifest.txt"), "tampered").unwrap();
    let e = MapFile::open(&dir, rel).unwrap_err();
    assert_eq!(e.code, argus_tiles::codes::MAP_MISMATCH);
    assert_eq!(e.r#ref.as_deref(), Some(rel));

    // Names become paths, so one that leaves the map's folder is refused,
    // from create and from a hand-edited index alike.
    for bad in ["../up", "a/b", ".hidden", ""] {
        let e = MapFile::create(
            &dir,
            "scenes/a/x.tilemap.toml",
            4,
            4,
            1.0,
            4,
            1,
            vec![layer(bad, None, false)],
        )
        .unwrap_err();
        assert_eq!(e.code, argus_tiles::codes::BAD_MAP, "{bad:?}");
    }
    let text = std::fs::read_to_string(dir.join(rel)).unwrap();
    let moved = text.replace("chunks = \"tiles.chunks\"", "chunks = \"../../outside\"");
    assert_ne!(moved, text);
    std::fs::write(dir.join(rel), moved).unwrap();
    let e = MapFile::open(&dir, rel).unwrap_err();
    assert_eq!(e.code, argus_tiles::codes::BAD_MAP);
    assert!(e.message.contains("one folder name"), "{e}");
    let wide = text.replace("width = 256", "width = -3");
    std::fs::write(dir.join(rel), wide).unwrap();
    let e = MapFile::open(&dir, rel).unwrap_err();
    assert!(e.message.contains("width = -3"), "{e}");
    std::fs::remove_dir_all(&dir).ok();
}

const FOURBIT: &str = r#"
# tile n is frame n - 1: tile 15 is solid, tile 16 one-way
[[frames]]
index = 14
properties = { solid = true }

[[frames]]
index = 15
properties = { one_way = true }

[autotile.rock]
mode = "4bit"
tiles = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
edges = "open"
"#;

#[test]
fn four_bit_auto_tiling_picks_each_tile_from_its_neighbours() {
    let dir = project("fourbit");
    let imp = tileset(&dir, "rock", 4, 4, FOURBIT);
    let rules = Rules::new(sprite(&imp, "tiles/rock"));
    assert_eq!(rules.terrains(), ["rock"]);
    let mut m = MapFile::create(
        &dir,
        "m.tilemap.toml",
        8,
        8,
        1.0,
        4,
        1,
        vec![layer("g", Some("tiles/rock"), true)],
    )
    .unwrap();
    let touched = paint(&mut m, 0, Rect::new(2, 2, 3, 3), Some("rock"), &rules).unwrap();
    assert_eq!(
        touched,
        Rect::new(1, 1, 5, 5),
        "the rect and a ring around it"
    );
    // Mask N=1 E=2 S=4 W=8 picks tiles[mask]; tile ids are mask + 1 here.
    let g = m.get(0, Rect::new(2, 2, 3, 3)).unwrap();
    assert_eq!(
        g,
        vec![
            1 + (2 | 4),
            1 + (2 | 4 | 8),
            1 + (4 | 8),
            1 + (1 | 2 | 4),
            1 + 15,
            1 + (1 | 4 | 8),
            1 + (1 | 2),
            1 + (1 | 2 | 8),
            1 + (1 | 8),
        ]
    );
    assert_eq!(m.cell(0, 1, 1).unwrap(), 0, "the ring stays empty");
    // Erase the centre: the four around it lose a neighbour and re-pick.
    paint(&mut m, 0, Rect::new(3, 3, 1, 1), None, &rules).unwrap();
    assert_eq!(m.cell(0, 3, 3).unwrap(), 0);
    assert_eq!(
        m.cell(0, 3, 2).unwrap(),
        1 + (2 | 8),
        "top middle has only west and east now"
    );
    // Edges: "open" treats outside the map as another terrain.
    paint(&mut m, 0, Rect::new(0, 0, 1, 1), Some("rock"), &rules).unwrap();
    assert_eq!(m.cell(0, 0, 0).unwrap(), 1, "no neighbours");
    assert!(
        paint(&mut m, 0, Rect::new(0, 0, 1, 1), Some("lava"), &rules)
            .unwrap_err()
            .hint
            .contains("rock")
    );

    // Collision comes from the tiles' properties: 15 solid, 16 one-way.
    m.fill(0, Rect::new(6, 6, 2, 1), 15).unwrap();
    m.fill(0, Rect::new(6, 7, 2, 1), 16).unwrap();
    let c = collision(&mut m, &imp, Rect::new(5, 6, 3, 2)).unwrap();
    let row: String = c.iter().map(|c| c.ch()).collect();
    assert_eq!(row, ".##.--");
    assert_eq!(c[1], Collide::Solid);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn blob47_uses_the_eight_neighbours_with_corners_only_between_edges() {
    let masks = blob_masks();
    assert_eq!(masks.len(), 47);
    assert_eq!((masks[0], masks[46]), (0, 255));
    let dir = project("blob");
    let tiles: Vec<String> = (1..=47).map(|t| t.to_string()).collect();
    let imp = tileset(
        &dir,
        "cave",
        8,
        6,
        &format!(
            "[autotile.cave]\nmode = \"blob47\"\ntiles = [{}]\n",
            tiles.join(", ")
        ),
    );
    let rules = Rules::new(sprite(&imp, "tiles/cave"));
    let mut m = MapFile::create(
        &dir,
        "m.tilemap.toml",
        6,
        6,
        1.0,
        8,
        1,
        vec![layer("g", Some("tiles/cave"), false)],
    )
    .unwrap();
    paint(&mut m, 0, Rect::new(1, 1, 3, 3), Some("cave"), &rules).unwrap();
    let idx = |mask: u32| masks.iter().position(|&m| m == mask).unwrap() as u32 + 1;
    assert_eq!(m.cell(0, 2, 2).unwrap(), idx(255), "surrounded");
    // Top-left of the block: E, SE, S. SE counts because E and S do.
    assert_eq!(m.cell(0, 1, 1).unwrap(), idx(4 | 8 | 16));
    // Top middle: E, SE, S, SW, W.
    assert_eq!(m.cell(0, 2, 1).unwrap(), idx(4 | 8 | 16 | 32 | 64));
    // A sidecar with the wrong count is refused where it is written.
    std::fs::write(
        dir.join("assets/tiles/cave.toml"),
        "grid = [8, 6]\n\n[autotile.cave]\nmode = \"blob47\"\ntiles = [1, 2]\n",
    )
    .unwrap();
    let mut imp = Importer::new(&dir);
    let r = imp.scan();
    assert!(
        r.failed[0]
            .error
            .message
            .contains("needs 47 tiles; it lists 2"),
        "{:?}",
        r.failed
    );
    assert_eq!(
        r.failed[0].error.r#ref.as_deref(),
        Some("assets/tiles/cave.toml:3")
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn visible_tiles_are_the_ones_the_view_covers() {
    let dir = project("visible");
    let mut m = MapFile::create(
        &dir,
        "m.tilemap.toml",
        100,
        100,
        2.0,
        16,
        2,
        vec![layer("g", Some("tiles/x"), false)],
    )
    .unwrap();
    m.fill(0, Rect::new(0, 0, 100, 100), 3).unwrap();
    // Origin at (0, 0) is the top-left corner; a 4x4-unit view centred at (4, -4)
    // covers columns 1..4 and rows 1..4 of 2-unit tiles.
    let v = visible(&mut m, [0.0, 0.0], [4.0, -4.0], [2.0, 2.0]).unwrap();
    assert_eq!(v.len(), 4, "{v:?}");
    assert_eq!(v[0].pos, [3.0, -3.0]);
    assert_eq!(v[0].frame, 2);
    assert_eq!(v[0].size, 2.0);
    // Off the map: nothing.
    assert!(visible(&mut m, [0.0, 0.0], [-50.0, 50.0], [2.0, 2.0])
        .unwrap()
        .is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

/// §17: 4,096 x 4,096 tiles, 8 layers: chunked storage, rect edits, and
/// auto-tiling at that size without pauses. Release only.
#[test]
#[ignore]
fn perf_4096_square_8_layers_without_pauses() {
    let dir = project("perf");
    let imp = tileset(&dir, "rock", 4, 4, FOURBIT);
    let rules = Rules::new(sprite(&imp, "tiles/rock"));
    let layers: Vec<LayerDef> = (0..8)
        .map(|i| layer(&format!("l{i}"), Some("tiles/rock"), i == 0))
        .collect();
    let t = Instant::now();
    let mut m = MapFile::create(&dir, "big.tilemap.toml", 4096, 4096, 1.0, 64, 2, layers).unwrap();
    let create = t.elapsed();

    // A 64x64 brush of terrain, auto-tiled, saved: what an edit costs.
    let mut worst = std::time::Duration::ZERO;
    for i in 0..20u32 {
        let t = Instant::now();
        let r = Rect::new(100 + i * 180, 200 + (i % 5) * 700, 64, 64);
        paint(&mut m, (i % 8) as usize, r, Some("rock"), &rules).unwrap();
        m.save().unwrap();
        worst = worst.max(t.elapsed());
    }
    // A big fill: a 1024x1024 area on one layer.
    let t = Instant::now();
    m.fill(1, Rect::new(1024, 1024, 1024, 1024), 5).unwrap();
    let rep = m.save().unwrap();
    let big = t.elapsed();
    // Reads: a screen's worth.
    let t = Instant::now();
    let v = visible(&mut m, [0.0, 0.0], [1500.0, -1500.0], [40.0, 22.5]).unwrap();
    let view = t.elapsed();
    let t = Instant::now();
    let m2 = MapFile::open(&dir, "big.tilemap.toml").unwrap();
    let open = t.elapsed();
    eprintln!(
        "4096x4096x8: create {create:?}, 64x64 auto-tiled brush + save worst {worst:?}, 1024x1024 fill + save {big:?} ({} chunks), visible view {view:?} ({} tiles), reopen {open:?} ({} chunks on disk)",
        rep.written,
        v.len(),
        m2.chunk_count()
    );
    assert!(create.as_millis() < 50, "{create:?}");
    assert!(
        worst.as_millis() < 16,
        "a brush stroke must fit a frame: {worst:?}"
    );
    assert!(view.as_millis() < 5, "{view:?}");
    assert!(open.as_millis() < 50, "{open:?}");
    std::fs::remove_dir_all(&dir).ok();
}
