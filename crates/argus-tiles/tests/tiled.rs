//! Phase 5 item 4: "a Tiled map becomes the engine's map and its layers"
//! (§12). The store reads a `.tmx` into the engine's form in memory: its
//! cells, its layers' tilesets, z, and collision, a hidden collision layer
//! that collides and is not drawn, collision from the `.tsx`'s tile
//! properties, edits refused with the hint to use Tiled, and a save in
//! Tiled read again.

use argus_assets::{encode_png, Importer};
use argus_tiles::{visible, Collide, MapFile, MapStore, Rect};
use std::path::{Path, PathBuf};

/// A temp directory removed on drop, used as its path.
struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for Temp {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn project(name: &str) -> Temp {
    let dir = std::env::temp_dir().join(format!("argus-tiles-tiled-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    Temp(dir)
}

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// Rock (solid), a plank (one-way), and a slope: 8-pixel tiles in a row.
const SET: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" name="cave" tilewidth="8" tileheight="8" tilecount="3" columns="3">
 <image source="cave.png" width="24" height="8"/>
 <tile id="0"><properties><property name="solid" type="bool" value="true"/></properties></tile>
 <tile id="1"><properties><property name="one_way" type="bool" value="true"/></properties></tile>
 <tile id="2"><properties><property name="slope" value="[0, 1]"/></properties></tile>
</tileset>
"#;

/// 6 x 4: a floor, a plank and a slope on a collision layer; a hidden
/// collision layer with one block; a decoration layer drawn, not hit.
fn map(floor_tile: u32) -> String {
    let f = floor_tile;
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="6" height="4" tilewidth="8" tileheight="8" infinite="0">
 <properties><property name="tile_size" type="float" value="2"/></properties>
 <tileset firstgid="1" source="../tiles/cave.tsx"/>
 <layer id="1" name="ground" width="6" height="4">
  <properties><property name="collision" type="bool" value="true"/></properties>
  <data encoding="csv">
0,0,0,0,0,0,
0,0,0,0,0,0,
0,0,2,3,0,0,
{f},{f},{f},{f},{f},{f}
</data>
 </layer>
 <layer id="2" name="blockers" width="6" height="4" visible="0">
  <properties><property name="collision" type="bool" value="true"/></properties>
  <data encoding="csv">
0,0,0,0,0,0,
0,0,0,0,0,1,
0,0,0,0,0,0,
0,0,0,0,0,0
</data>
 </layer>
 <layer id="3" name="deco" width="6" height="4">
  <properties><property name="z" type="int" value="5"/></properties>
  <data encoding="csv">
2,0,0,0,0,0,
0,0,0,0,0,0,
0,0,0,0,0,0,
0,0,0,0,0,0
</data>
 </layer>
</map>
"#
    )
}

fn setup(name: &str) -> (Temp, Importer) {
    let t = project(name);
    encode_png(
        &t.join("assets/tiles/cave.png"),
        24,
        8,
        &[90, 80, 70, 255].repeat(24 * 8),
    )
    .unwrap();
    write(&t, "assets/tiles/cave.tsx", SET);
    write(&t, "assets/maps/room.tmx", &map(1));
    let mut imp = Importer::new(&t);
    let rep = imp.scan();
    assert!(
        rep.failed.is_empty(),
        "{:?}",
        rep.failed
            .iter()
            .map(|f| &f.error.message)
            .collect::<Vec<_>>()
    );
    (t, imp)
}

const FILE: &str = "assets/maps/room.tmx";

#[test]
fn a_tiled_map_is_the_engines_map_and_collides_by_its_tilesets_properties() {
    let (t, imp) = setup("open");
    assert!(MapFile::is_tilemap(&t, FILE));
    let mut store = MapStore::new(&t);
    store.set_tilesets(&imp);
    let m = store.open(FILE).unwrap();
    assert!(m.is_in_memory());
    assert_eq!(
        (m.index.width, m.index.height, m.index.tile_size),
        (6, 4, 2.0)
    );
    let layers: Vec<(&str, Option<&str>, f32, bool)> = m
        .index
        .layers
        .iter()
        .map(|l| (l.name.as_str(), l.tileset.as_deref(), l.z, l.collision))
        .collect();
    assert_eq!(
        layers,
        vec![
            ("ground", Some("tiles/cave"), 0.0, true),
            ("blockers", Some("tiles/cave"), 1.0, true),
            ("deco", Some("tiles/cave"), 5.0, false),
        ]
    );
    assert_eq!(
        m.get(0, Rect::new(0, 2, 6, 2)).unwrap(),
        vec![0, 0, 2, 3, 0, 0, 1, 1, 1, 1, 1, 1]
    );
    assert!(m.layer_visible(0) && !m.layer_visible(1) && m.layer_visible(2));
    let info = &m.tiled().unwrap().info;
    assert_eq!(info["tilesets"][0]["tiles"], 3);
    assert_eq!(info["layers"][1]["visible"], false);

    // Collision from the .tsx's properties, the hidden layer included.
    let g = store.grid(FILE, [0.0, 8.0]).unwrap();
    assert_eq!((g.width, g.height, g.tile), (6, 4, 2.0));
    assert_eq!(store.collide(&g, 0, 3), Collide::Solid);
    assert_eq!(store.collide(&g, 2, 2), Collide::OneWay);
    assert_eq!(
        store.collide(&g, 3, 2),
        Collide::Slope {
            left: 0.0,
            right: 1.0
        }
    );
    assert_eq!(
        store.collide(&g, 5, 1),
        Collide::Solid,
        "a hidden collision layer still collides"
    );
    assert_eq!(
        store.collide(&g, 0, 0),
        Collide::Empty,
        "decoration is not hit"
    );
    let (rows, _, _) = store
        .collision_rows(&g, [0.0, 0.0, 12.0, 8.0], [64, 36])
        .unwrap();
    assert_eq!(rows, vec!["......", ".....#", "..-/..", "######"]);
    let rect = Rect::new(0, 0, 6, 4);
    let m = store.open(FILE).unwrap();
    let all = argus_tiles::collision(m, &imp, rect).unwrap();
    assert_eq!(all[6 + 5], Collide::Solid);

    // Drawn: every visible layer's tiles, the hidden one's not.
    let m = store.open(FILE).unwrap();
    let drawn = visible(m, [0.0, 8.0], [6.0, 4.0], [10.0, 10.0]).unwrap();
    assert_eq!(drawn.len(), 6 + 2 + 1);
    assert!(drawn
        .iter()
        .all(|d| d.tileset == "tiles/cave" && d.size == 2.0));
    let deco = drawn.iter().find(|d| d.z == 5.0).unwrap();
    assert_eq!((deco.frame, deco.pos), (1, [1.0, 7.0]));
    assert!(
        !drawn.iter().any(|d| d.pos == [11.0, 5.0]),
        "the hidden block is not drawn"
    );
}

#[test]
fn a_tiled_map_is_tileds_to_edit_and_a_save_in_tiled_is_read_again() {
    let (t, imp) = setup("edit");
    let mut store = MapStore::new(&t);
    store.set_tilesets(&imp);
    let m = store.open(FILE).unwrap();
    let e = m.set(0, Rect::new(0, 0, 1, 1), &[1]).unwrap_err();
    assert!(e.message.contains("is a Tiled map"), "{}", e.message);
    assert!(e.hint.contains("edit it in Tiled"), "{}", e.hint);
    assert!(m.fill(0, Rect::new(0, 0, 2, 2), 0).is_err());
    assert!(store.save(FILE).is_err(), "nothing writes a Tiled map");
    assert_eq!(
        std::fs::read_to_string(t.join(FILE)).unwrap(),
        map(1),
        "the file is as Tiled left it"
    );
    let before = store.revision(FILE).unwrap();

    // Saved again in Tiled: the floor is planks now.
    write(&t, FILE, &map(2));
    let f = std::fs::File::options()
        .write(true)
        .open(t.join(FILE))
        .unwrap();
    f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5))
        .unwrap();
    drop(f);
    let m = store.open(FILE).unwrap();
    assert_eq!(m.cell(0, 0, 3).unwrap(), 2);
    assert_ne!(
        store.revision(FILE).unwrap(),
        before,
        "what was derived from it is made again"
    );
    let g = store.grid(FILE, [0.0, 0.0]).unwrap();
    assert_eq!(store.collide(&g, 0, 3), Collide::OneWay);

    // A file Tiled could not have written is an error, not a panic.
    write(&t, FILE, "<map orientation=\"orthogonal\"><layer>");
    let f = std::fs::File::options()
        .write(true)
        .open(t.join(FILE))
        .unwrap();
    f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(10))
        .unwrap();
    drop(f);
    let e = store.open(FILE).unwrap_err();
    assert_eq!(e.code, argus_tiles::codes::BAD_MAP);
    assert!(e.message.contains("assets/maps/room.tmx"), "{}", e.message);
    // A map of objects only has nothing to draw: said so.
    write(
        &t,
        "assets/maps/marks.tmx",
        "<map orientation=\"orthogonal\" width=\"2\" height=\"2\" tilewidth=\"8\" tileheight=\"8\"><objectgroup name=\"o\"/></map>",
    );
    let e = store.open("assets/maps/marks.tmx").unwrap_err();
    assert!(e.message.contains("no tile layers"), "{}", e.message);
}
