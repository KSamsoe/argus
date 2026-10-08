//! Phase 5 item 4: Tiled's files import natively (§14), with their internal
//! structure exposed, and a Tiled map reads as the engine's map and its
//! layers (§12). The fixtures are written here as Tiled writes them: a
//! tileset with properties, margin, spacing, and an animation; a map with
//! CSV, zlib, and gzip layers, a group, flipped tiles, a layer drawing
//! from two tilesets, an embedded tileset, and an object layer; the same
//! in Tiled's JSON; an infinite map; and files the engine refuses.

use argus_assets::tiled::{read_map, TiledMap};
use argus_assets::{encode_png, Asset, Importer};
use std::io::Write;
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
    let dir = std::env::temp_dir().join(format!("argus-tiled-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    Temp(dir)
}

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn le(gids: &[u32]) -> Vec<u8> {
    gids.iter().flat_map(|g| g.to_le_bytes()).collect()
}

fn zlib(b: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(b).unwrap();
    e.finish().unwrap()
}

fn gzip(b: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(b).unwrap();
    e.finish().unwrap()
}

fn b64(b: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                s.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

fn csv(gids: &[u32]) -> String {
    gids.iter()
        .map(|g| g.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// One colour per tile of a `cols` x `rows` sheet of 8-pixel tiles with a
/// margin and spacing; tile `clear` is transparent.
fn sheet(
    root: &Path,
    rel: &str,
    cols: u32,
    rows: u32,
    margin: u32,
    spacing: u32,
    colours: &[[u8; 4]],
) {
    let w = 2 * margin + cols * 8 + (cols - 1) * spacing;
    let h = 2 * margin + rows * 8 + (rows - 1) * spacing;
    let mut px = vec![0u8; (w * h * 4) as usize];
    for r in 0..rows {
        for c in 0..cols {
            let col = colours[(r * cols + c) as usize];
            for y in 0..8 {
                for x in 0..8 {
                    let (px_x, px_y) = (
                        margin + c * (8 + spacing) + x,
                        margin + r * (8 + spacing) + y,
                    );
                    let i = ((px_y * w + px_x) * 4) as usize;
                    px[i..i + 4].copy_from_slice(&col);
                }
            }
        }
    }
    encode_png(&root.join(rel), w, h, &px).unwrap();
}

pub const RED: [u8; 4] = [200, 40, 40, 255];
pub const GREEN: [u8; 4] = [40, 200, 40, 255];
pub const BLUE: [u8; 4] = [40, 40, 200, 255];
pub const CLEAR: [u8; 4] = [0, 0, 0, 0];
pub const YELLOW: [u8; 4] = [230, 210, 30, 255];
pub const CYAN: [u8; 4] = [30, 210, 230, 255];

const CAVE_TSX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.11.0" name="cave" tilewidth="8" tileheight="8" spacing="2" margin="1" tilecount="4" columns="2">
 <image source="cave.png" width="20" height="20"/>
 <tile id="0">
  <properties>
   <property name="solid" type="bool" value="true"/>
   <property name="hp" type="int" value="3"/>
   <property name="friction" type="float" value="0.5"/>
   <property name="note" value="rock &amp; roll"/>
  </properties>
 </tile>
 <tile id="1" type="water">
  <properties>
   <property name="one_way" type="bool" value="true"/>
  </properties>
  <animation>
   <frame tileid="1" duration="200"/>
   <frame tileid="2" duration="200"/>
  </animation>
 </tile>
 <tile id="2">
  <properties>
   <property name="slope" value="[0, 1]"/>
  </properties>
 </tile>
</tileset>
"#;

const CAVE_TSJ: &str = r#"{ "type": "tileset", "name": "cave", "tilewidth": 8, "tileheight": 8, "spacing": 2, "margin": 1,
  "tilecount": 4, "columns": 2, "image": "../tiles/cave.png", "imagewidth": 20, "imageheight": 20,
  "tiles": [
    { "id": 0, "properties": [
      { "name": "solid", "type": "bool", "value": true },
      { "name": "hp", "type": "int", "value": 3 },
      { "name": "friction", "type": "float", "value": 0.5 },
      { "name": "note", "type": "string", "value": "rock & roll" } ] },
    { "id": 1, "type": "water", "properties": [ { "name": "one_way", "type": "bool", "value": true } ],
      "animation": [ { "tileid": 1, "duration": 200 }, { "tileid": 2, "duration": 200 } ] },
    { "id": 2, "properties": [ { "name": "slope", "type": "string", "value": "[0, 1]" } ] } ] }
"#;

/// The layers of the fixture map, 6 x 4: Tiled's GIDs, row by row.
fn ground() -> Vec<u32> {
    let mut g = vec![0; 24];
    g[2 * 6 + 2] = 2;
    g[2 * 6 + 3] = 3;
    for x in 0..6 {
        g[3 * 6 + x] = 1;
    }
    g
}
fn back() -> Vec<u32> {
    let mut g = vec![0; 24];
    g[5] = 2;
    g
}
/// Both tilesets: cave's tile 3, and the embedded set's second tile.
fn mixed() -> Vec<u32> {
    let mut g = vec![0; 24];
    g[6 + 1] = 3;
    g[6 + 4] = 6;
    g
}
/// A tile flipped horizontally, and a plain one.
fn glow() -> Vec<u32> {
    let mut g = vec![0; 24];
    g[0] = 0x8000_0001;
    g[1] = 2;
    g
}

fn level_tmx() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" tiledversion="1.11.0" orientation="orthogonal" renderorder="right-down" width="6" height="4" tilewidth="8" tileheight="8" infinite="0" nextlayerid="9" nextobjectid="3">
 <properties>
  <property name="music" value="cave"/>
 </properties>
 <tileset firstgid="1" source="../tiles/cave.tsx"/>
 <tileset firstgid="5" name="Deco items" tilewidth="8" tileheight="8" tilecount="2" columns="2">
  <image source="deco.png" width="16" height="8"/>
  <tile id="1">
   <properties>
    <property name="solid" type="bool" value="true"/>
   </properties>
  </tile>
 </tileset>
 <layer id="1" name="ground" width="6" height="4">
  <properties>
   <property name="collision" type="bool" value="true"/>
  </properties>
  <data encoding="csv">
{}
</data>
 </layer>
 <layer id="2" name="back" width="6" height="4">
  <data encoding="base64" compression="zlib">
   {}
  </data>
 </layer>
 <layer id="3" name="mixed" width="6" height="4">
  <data encoding="base64" compression="gzip">{}</data>
 </layer>
 <group id="4" name="fx" offsetx="8" offsety="0" visible="0">
  <layer id="5" name="glow" width="6" height="4" opacity="0.5">
   <properties>
    <property name="z" type="float" value="-2"/>
   </properties>
   <data encoding="csv">{}</data>
  </layer>
 </group>
 <objectgroup id="6" name="spawns">
  <object id="1" name="start" type="Spawn" x="12" y="20">
   <point/>
  </object>
  <object id="2" name="door" class="Door" x="32" y="16" width="8" height="16">
   <properties>
    <property name="to" value="level2"/>
   </properties>
  </object>
 </objectgroup>
</map>
"#,
        csv(&ground()),
        b64(&zlib(&le(&back()))),
        b64(&gzip(&le(&mixed()))),
        csv(&glow()),
    )
}

fn level_tmj() -> String {
    format!(
        r#"{{ "type": "map", "version": "1.10", "orientation": "orthogonal", "renderorder": "right-down",
  "width": 6, "height": 4, "tilewidth": 8, "tileheight": 8, "infinite": false,
  "properties": [ {{ "name": "music", "type": "string", "value": "cave" }} ],
  "tilesets": [
    {{ "firstgid": 1, "source": "cave.tsj" }},
    {{ "firstgid": 5, "name": "Deco items", "tilewidth": 8, "tileheight": 8, "tilecount": 2, "columns": 2,
       "image": "../maps/deco.png", "imagewidth": 16, "imageheight": 8,
       "tiles": [ {{ "id": 1, "properties": [ {{ "name": "solid", "type": "bool", "value": true }} ] }} ] }} ],
  "layers": [
    {{ "type": "tilelayer", "id": 1, "name": "ground", "width": 6, "height": 4, "opacity": 1, "visible": true, "x": 0, "y": 0,
       "properties": [ {{ "name": "collision", "type": "bool", "value": true }} ], "data": [{}] }},
    {{ "type": "tilelayer", "id": 2, "name": "back", "width": 6, "height": 4, "opacity": 1, "visible": true, "x": 0, "y": 0,
       "encoding": "base64", "compression": "zlib", "data": "{}" }},
    {{ "type": "tilelayer", "id": 3, "name": "mixed", "width": 6, "height": 4, "opacity": 1, "visible": true, "x": 0, "y": 0,
       "encoding": "base64", "compression": "gzip", "data": "{}" }},
    {{ "type": "group", "id": 4, "name": "fx", "offsetx": 8, "offsety": 0, "opacity": 1, "visible": false, "layers": [
      {{ "type": "tilelayer", "id": 5, "name": "glow", "width": 6, "height": 4, "opacity": 0.5, "visible": true, "x": 0, "y": 0,
         "properties": [ {{ "name": "z", "type": "float", "value": -2 }} ], "data": [{}] }} ] }},
    {{ "type": "objectgroup", "id": 6, "name": "spawns", "opacity": 1, "visible": true, "objects": [
      {{ "id": 1, "name": "start", "type": "Spawn", "x": 12, "y": 20, "width": 0, "height": 0, "rotation": 0, "visible": true, "point": true }},
      {{ "id": 2, "name": "door", "type": "Door", "x": 32, "y": 16, "width": 8, "height": 16, "rotation": 0, "visible": true,
         "properties": [ {{ "name": "to", "type": "string", "value": "level2" }} ] }} ] }} ] }}
"#,
        csv(&ground()),
        b64(&zlib(&le(&back()))),
        b64(&gzip(&le(&mixed()))),
        csv(&glow()),
    )
}

/// The cave tileset, the fixture map in both forms, and their images.
fn tiled_project(root: &Path) {
    sheet(
        root,
        "assets/tiles/cave.png",
        2,
        2,
        1,
        2,
        &[RED, GREEN, BLUE, CLEAR],
    );
    write(root, "assets/tiles/cave.tsx", CAVE_TSX);
    write(
        root,
        "assets/tiles/cave.toml",
        "# auto-tiling over a Tiled tileset\n[autotile.rock]\nmode = \"single\"\ntiles = [1]\n",
    );
    sheet(root, "assets/maps/deco.png", 2, 1, 0, 0, &[YELLOW, CYAN]);
    write(root, "assets/maps/level1.tmx", &level_tmx());
    write(root, "assets/json/cave.tsj", CAVE_TSJ);
    write(root, "assets/json/level1.tmj", &level_tmj());
}

fn read(root: &Path, rel: &str) -> TiledMap {
    let bytes = std::fs::read(root.join(rel)).unwrap();
    let mut load = |p: &str| std::fs::read(root.join(p)).map_err(|e| e.to_string());
    read_map(rel, rel, &bytes, &mut load).unwrap()
}

#[test]
fn a_tileset_imports_as_a_sprite_with_its_tiles_properties_and_animations() {
    let t = project("tileset");
    tiled_project(&t);
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
    // cave.tsx and cave.png are one asset: the tileset, drawn from its image.
    let e = imp.get("tiles/cave").unwrap();
    assert_eq!(e.kind(), "sprite");
    let Asset::Sprite(s) = &e.asset else { panic!() };
    assert_eq!(s.source.as_deref(), Some("tiles/cave.png"));
    // Margin 1, spacing 2: Tiled's ids in order, tile n is frame n - 1.
    let rects: Vec<[u32; 4]> = s.frames.iter().map(|f| f.rect).collect();
    assert_eq!(
        rects,
        vec![[1, 1, 8, 8], [11, 1, 8, 8], [1, 11, 8, 8], [11, 11, 8, 8]]
    );
    assert_eq!(s.tile_property(1, "solid"), Some("true"));
    assert_eq!(s.tile_property(1, "hp"), Some("3"));
    assert_eq!(s.tile_property(1, "friction"), Some("0.5"));
    assert_eq!(s.tile_property(1, "note"), Some("rock & roll"));
    assert_eq!(s.tile_property(2, "one_way"), Some("true"));
    assert_eq!(s.tile_property(3, "slope"), Some("[0, 1]"));
    assert_eq!(argus_assets::parse_slope("[0, 1]"), Some((0.0, 1.0)));
    // One tile, one world unit; the animation is a clip named by its class.
    assert_eq!(s.pixels_per_unit, 8.0);
    let water = &s.clips["water"];
    assert_eq!((water.frames.clone(), water.fps), (vec![1, 2], Some(5.0)));
    // The sidecar beside it adds auto-tiling over the Tiled tileset.
    assert_eq!(s.autotile["rock"].tiles, vec![1]);
    let says = |m: &str| e.warnings.iter().any(|w| w.message.contains(m));
    assert!(says("1 tiles are empty (3)"), "{:?}", e.warnings);
    assert!(
        says("tile animations became clips (water)"),
        "{:?}",
        e.warnings
    );
    assert!(!says("not power-of-two") || s.texture_size == [32, 32]);
    // asset.get shows the frames' properties as any sprite's.
    let j = e.to_json();
    assert_eq!(j["frames"][0]["properties"]["solid"], "true");
    assert_eq!(j["kind"], "sprite");

    // The .tsj says the same.
    let Asset::Sprite(js) = &imp.get("json/cave").unwrap().asset else {
        panic!()
    };
    assert_eq!(js.frames, s.frames);
    assert_eq!(js.clips, s.clips);
}

#[test]
fn a_map_imports_with_its_layers_tilesets_and_objects_exposed() {
    let t = project("map");
    tiled_project(&t);
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
    let e = imp.get("maps/level1").unwrap();
    assert_eq!(e.kind(), "tilemap");
    let j = e.to_json();
    assert_eq!(j["file"], "maps/level1.tmx");
    assert_eq!(j["size"], serde_json::json!([6, 4]));
    assert_eq!(j["properties"]["music"], "cave");
    // Tilesets: the file, and the embedded one, each a sprite asset.
    let ts = j["tilesets"].as_array().unwrap();
    assert_eq!(ts[0]["name"], "cave");
    assert_eq!(ts[0]["firstgid"], 1);
    assert_eq!(ts[0]["tiles"], 4);
    assert_eq!(ts[0]["asset"], "tiles/cave");
    assert_eq!(ts[1]["name"], "Deco items");
    assert_eq!(ts[1]["firstgid"], 5);
    assert_eq!(ts[1]["asset"], "maps/level1.tmx/Deco_items");
    assert_eq!(ts[1]["embedded"], true);
    let deco = imp.get("maps/level1.tmx/Deco_items").unwrap();
    let Asset::Sprite(d) = &deco.asset else {
        panic!()
    };
    assert_eq!(d.frames.len(), 2);
    assert_eq!(d.tile_property(2, "solid"), Some("true"));
    assert_eq!(d.source.as_deref(), Some("maps/deco.png"));
    // The engine's layers: one per tile layer, the mixed one split by
    // tileset, the group's layer flattened under its name.
    let layers = j["layers"].as_array().unwrap();
    let names: Vec<&str> = layers.iter().map(|l| l["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "ground",
            "back",
            "mixed (cave)",
            "mixed (Deco items)",
            "fx/glow"
        ]
    );
    assert_eq!(layers[0]["collision"], true);
    assert_eq!(layers[0]["tileset"], "tiles/cave");
    assert_eq!(layers[0]["z"], 0.0);
    assert_eq!(layers[1]["z"], 1.0);
    assert_eq!(
        (layers[2]["z"].clone(), layers[3]["z"].clone()),
        (2.0.into(), 2.0.into())
    );
    assert_eq!(layers[3]["tileset"], "maps/level1.tmx/Deco_items");
    assert_eq!(layers[4]["z"], -2.0);
    assert_eq!(layers[4]["visible"], false);
    assert_eq!(layers[4]["kind"], "tiles");
    let tl = j["tiled_layers"].as_array().unwrap();
    assert_eq!(tl[2]["tilesets"], serde_json::json!(["cave", "Deco items"]));
    assert_eq!(
        tl[2]["layers"],
        serde_json::json!(["mixed (cave)", "mixed (Deco items)"])
    );
    assert_eq!(tl[4]["kind"], "objects");
    // Objects are data: world units from the map's top-left corner, y up.
    let objs = &j["object_layers"][0];
    assert_eq!(objs["name"], "spawns");
    let start = &objs["objects"][0];
    assert_eq!(
        (
            start["name"].clone(),
            start["class"].clone(),
            start["shape"].clone()
        ),
        ("start".into(), "Spawn".into(), "point".into())
    );
    assert_eq!(start["pos"], serde_json::json!([1.5, -2.5]));
    let door = &objs["objects"][1];
    assert_eq!(door["class"], "Door");
    assert_eq!(door["pos"], serde_json::json!([4.0, -2.0]));
    assert_eq!(door["size"], serde_json::json!([1.0, 2.0]));
    assert_eq!(door["properties"]["to"], "level2");
    // What the engine cannot carry is said once a layer.
    let says = |m: &str| e.warnings.iter().filter(|w| w.message.contains(m)).count();
    assert_eq!(says("1 flipped or rotated tiles"), 1, "{:?}", e.warnings);
    assert_eq!(says("opacity 0.5"), 1, "{:?}", e.warnings);
    assert!(
        e.warnings
            .iter()
            .all(|w| w.at.starts_with("assets/maps/level1.tmx:")),
        "{:?}",
        e.warnings
    );
    // asset.list's kind.
    assert!(imp
        .entries()
        .values()
        .any(|e| e.kind() == "tilemap" && e.name == "maps/level1"));

    // The cells, as argus-tiles takes them: Tiled's ids made each tileset's
    // own, flags dropped, the group's offset moving its layer a tile right.
    let m = read(&t, "assets/maps/level1.tmx");
    let cells = |n: &str| m.layers.iter().find(|l| l.name == n).unwrap().cells.clone();
    let mut g = cells("ground");
    g.sort();
    assert_eq!(g.len(), 8);
    assert!(g.contains(&[2, 2, 2]) && g.contains(&[3, 2, 3]) && g.contains(&[0, 3, 1]));
    assert_eq!(cells("back"), vec![[5, 0, 2]]);
    assert_eq!(cells("mixed (cave)"), vec![[1, 1, 3]]);
    assert_eq!(cells("mixed (Deco items)"), vec![[4, 1, 2]]);
    assert_eq!(cells("fx/glow"), vec![[1, 0, 1], [2, 0, 2]]);
}

#[test]
fn tmj_and_tsj_read_as_the_same_map() {
    let t = project("json");
    tiled_project(&t);
    let x = read(&t, "assets/maps/level1.tmx");
    let j = read(&t, "assets/json/level1.tmj");
    assert_eq!(x.format, "tmx");
    assert_eq!(j.format, "tmj");
    assert_eq!(
        (x.width, x.height, x.tile_px),
        (j.width, j.height, j.tile_px)
    );
    let shape = |m: &TiledMap| {
        m.layers
            .iter()
            .map(|l| (l.name.clone(), l.z, l.collision, l.visible, l.cells.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(shape(&x), shape(&j));
    assert_eq!(
        x.tilesets
            .iter()
            .map(|t| (t.firstgid, t.name.clone(), t.tile_count))
            .collect::<Vec<_>>(),
        j.tilesets
            .iter()
            .map(|t| (t.firstgid, t.name.clone(), t.tile_count))
            .collect::<Vec<_>>()
    );
    assert_eq!(j.tilesets[0].asset, "json/cave");
    assert_eq!(x.objects, j.objects);
    assert_eq!(x.properties, j.properties);
    let msgs = |m: &TiledMap| {
        m.warnings
            .iter()
            .map(|w| w.message.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(msgs(&x), msgs(&j));
    // Both import, and the JSON map's embedded tileset is its own sprite.
    let mut imp = Importer::new(&t);
    assert!(imp.scan().failed.is_empty());
    assert!(imp.get("json/level1.tmj/Deco_items").is_ok());
    assert_eq!(imp.get("json/level1").unwrap().kind(), "tilemap");
}

#[test]
fn an_infinite_map_is_the_bounds_of_its_chunks() {
    let t = project("infinite");
    tiled_project(&t);
    let mut a = vec![0u32; 256];
    a[2 * 16 + 3] = 1;
    let mut b = vec![0u32; 256];
    b[15 * 16 + 15] = 2;
    write(
        &t,
        "assets/maps/endless.tmx",
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="30" height="20" tilewidth="8" tileheight="8" infinite="1">
 <tileset firstgid="1" source="../tiles/cave.tsx"/>
 <layer id="1" name="ground" width="30" height="20">
  <data encoding="base64" compression="zlib">
   <chunk x="-16" y="-16" width="16" height="16">{}</chunk>
   <chunk x="0" y="0" width="16" height="16">{}</chunk>
  </data>
 </layer>
 <objectgroup id="2" name="marks">
  <object id="1" name="here" x="0" y="0"><point/></object>
 </objectgroup>
</map>
"#,
            b64(&zlib(&le(&a))),
            b64(&zlib(&le(&b)))
        ),
    );
    let m = read(&t, "assets/maps/endless.tmx");
    assert!(m.infinite);
    assert_eq!((m.width, m.height), (32, 32));
    assert_eq!(m.origin, [-16, -16]);
    // Tiled's (-13, -14) is the engine's (3, 2); Tiled's (15, 15) is (31, 31).
    assert_eq!(m.layers[0].cells, vec![[3, 2, 1], [31, 31, 2]]);
    // An object at Tiled's origin is 16 tiles right and down of the corner.
    assert_eq!(m.objects[0].objects[0].pos, [16.0, -16.0]);
}

#[test]
fn what_the_engine_cannot_read_is_refused_with_why_and_the_rest_imports() {
    let t = project("refused");
    tiled_project(&t);
    let head = r#"<?xml version="1.0" encoding="UTF-8"?>"#;
    // Cut short: the layer never closes.
    write(
        &t,
        "assets/bad/broken.tmx",
        &format!("{head}\n<map orientation=\"orthogonal\" width=\"1\" height=\"1\" tilewidth=\"8\" tileheight=\"8\">\n <layer name=\"a\" width=\"1\" height=\"1\">\n  <data encoding=\"csv\">1</data>\n</map>\n"),
    );
    write(
        &t,
        "assets/bad/zstd.tmx",
        &format!("{head}\n<map orientation=\"orthogonal\" width=\"1\" height=\"1\" tilewidth=\"8\" tileheight=\"8\">\n <tileset firstgid=\"1\" source=\"../tiles/cave.tsx\"/>\n <layer name=\"a\" width=\"1\" height=\"1\"><data encoding=\"base64\" compression=\"zstd\">KLUv/SAEIQAAAQAAAA==</data></layer>\n</map>\n"),
    );
    write(
        &t,
        "assets/bad/iso.tmx",
        &format!("{head}\n<map orientation=\"isometric\" width=\"1\" height=\"1\" tilewidth=\"16\" tileheight=\"8\">\n <layer name=\"a\" width=\"1\" height=\"1\"><data encoding=\"csv\">0</data></layer>\n</map>\n"),
    );
    write(
        &t,
        "assets/bad/short.tmx",
        &format!("{head}\n<map orientation=\"orthogonal\" width=\"3\" height=\"2\" tilewidth=\"8\" tileheight=\"8\">\n <layer name=\"a\" width=\"3\" height=\"2\"><data encoding=\"csv\">1,1,1</data></layer>\n</map>\n"),
    );
    write(
        &t,
        "assets/bad/pile.tsx",
        &format!("{head}\n<tileset name=\"pile\" tilewidth=\"8\" tileheight=\"8\" tilecount=\"1\" columns=\"0\">\n <tile id=\"0\"><image source=\"../tiles/cave.png\" width=\"20\" height=\"20\"/></tile>\n</tileset>\n"),
    );
    write(
        &t,
        "assets/bad/garbled.tmj",
        "{ \"type\": \"map\", \"layers\": [ ",
    );
    let mut imp = Importer::new(&t);
    let rep = imp.scan();
    let why = |n: &str| {
        let f = imp
            .failures()
            .get(n)
            .unwrap_or_else(|| panic!("{n} imported"));
        (
            f.error.message.clone(),
            f.error.hint.clone(),
            f.error.r#ref.clone().unwrap_or_default(),
        )
    };
    let (m, _, at) = why("bad/broken");
    assert!(m.contains("not valid XML"), "{m}");
    assert!(at.starts_with("assets/bad/broken.tmx:"), "{at}");
    let (m, hint, _) = why("bad/zstd");
    assert!(m.contains("zstd"), "{m}");
    assert!(hint.contains("Save it with zlib, gzip, or CSV"), "{hint}");
    let (m, _, _) = why("bad/iso");
    assert!(m.contains("isometric") && m.contains("orthogonal"), "{m}");
    let (m, _, _) = why("bad/short");
    assert!(m.contains("3 tiles for its 3x2 cells"), "{m}");
    let (m, hint, _) = why("bad/pile");
    assert!(m.contains("image collection"), "{m}");
    assert!(hint.contains("one image"), "{hint}");
    let (m, _, _) = why("bad/garbled");
    assert!(m.contains("not valid JSON"), "{m}");
    assert_eq!(
        rep.failed.len(),
        6,
        "{:?}",
        rep.failed.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    // One bad file stops nothing else.
    assert!(imp.get("maps/level1").is_ok());
    assert!(imp.get("tiles/cave").is_ok());
}

#[test]
fn the_watcher_reimports_a_changed_tileset_and_a_maps_embedded_tilesets_follow_the_map() {
    let t = project("watch");
    tiled_project(&t);
    let mut imp = Importer::new(&t);
    imp.scan();
    assert!(imp.get("maps/level1.tmx/Deco_items").is_ok());
    // A tile becomes one-way in Tiled.
    let tsx = CAVE_TSX.replace(
        r#"<property name="slope" value="[0, 1]"/>"#,
        r#"<property name="one_way" type="bool" value="true"/>"#,
    );
    write(&t, "assets/tiles/cave.tsx", &tsx);
    imp.touch("tiles/cave");
    let rep = imp.scan();
    assert!(
        rep.imported.contains(&"tiles/cave".to_string()),
        "{:?}",
        rep.imported
    );
    let Asset::Sprite(s) = &imp.get("tiles/cave").unwrap().asset else {
        panic!()
    };
    assert_eq!(s.tile_property(3, "one_way"), Some("true"));
    // The embedded tileset renamed in the map: the old sprite goes, the new comes.
    let tmx = level_tmx().replace("name=\"Deco items\"", "name=\"props\"");
    write(&t, "assets/maps/level1.tmx", &tmx);
    imp.touch("maps/level1");
    let rep = imp.scan();
    assert!(
        rep.removed
            .contains(&"maps/level1.tmx/Deco_items".to_string()),
        "{:?}",
        rep.removed
    );
    assert!(
        rep.imported.contains(&"maps/level1.tmx/props".to_string()),
        "{:?}",
        rep.imported
    );
    // The map gone, its embedded tileset goes with it.
    std::fs::remove_file(t.join("assets/maps/level1.tmx")).unwrap();
    let rep = imp.scan();
    assert!(rep.removed.contains(&"maps/level1".to_string()));
    assert!(
        rep.removed.contains(&"maps/level1.tmx/props".to_string()),
        "{:?}",
        rep.removed
    );
}

#[test]
fn a_damaged_map_or_tileset_is_an_error_never_a_panic() {
    let t = project("damaged");
    tiled_project(&t);
    let mut load = |p: &str| std::fs::read(t.join(p)).map_err(|e| e.to_string());
    for (rel, text) in [
        ("assets/maps/level1.tmx", level_tmx()),
        ("assets/json/level1.tmj", level_tmj()),
    ] {
        let bytes = text.into_bytes();
        // Cut short at every byte, and every byte made something else.
        for n in 0..bytes.len() {
            let _ = read_map(rel, rel, &bytes[..n], &mut load);
            for b in [b'<', b'"', b'9', b'-', 0xff] {
                let mut x = bytes.clone();
                x[n] = b;
                let _ = read_map(rel, rel, &x, &mut load);
            }
        }
    }
    for (rel, text) in [
        ("assets/tiles/cave.tsx", CAVE_TSX),
        ("assets/json/cave.tsj", CAVE_TSJ),
    ] {
        let bytes = text.as_bytes();
        for n in 0..bytes.len() {
            let _ = argus_assets::tiled::read_tileset(rel, &bytes[..n]);
            let mut x = bytes.to_vec();
            x[n] = b'9';
            let _ = argus_assets::tiled::read_tileset(rel, &x);
        }
    }
}
