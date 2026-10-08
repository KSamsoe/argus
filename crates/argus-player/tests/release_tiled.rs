//! Phase 5 item 4: a game whose map is a Tiled `.tmx` and whose tileset is
//! a `.tsx` ships (`argus build`) and plays from its packs as from its
//! folder: the tilesets go through the sprite catalog and its pages like
//! any sprite (their images left out), the `.tmx` ships as a file, and the
//! map collides and draws the same.

use argus_player::pack::{self, BuildOptions};
use argus_player::Player;
use argus_render::Renderer;
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

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// Tiles of one colour each, 8 pixels, in a row.
fn strip(root: &Path, rel: &str, colours: &[[u8; 4]]) {
    let n = colours.len() as u32;
    let mut px = Vec::new();
    for _ in 0..8 {
        for c in colours {
            px.extend(c.repeat(8));
        }
    }
    argus_assets::encode_png(&root.join(rel), 8 * n, 8, &px).unwrap();
}

const SET: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" name="cave" tilewidth="8" tileheight="8" tilecount="2" columns="2">
 <image source="cave.png" width="16" height="8"/>
 <tile id="0"><properties><property name="solid" type="bool" value="true"/></properties></tile>
</tileset>
"#;

/// A floor of rock, a moss tile, and a crate from a tileset written in the map.
const MAP: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="6" height="4" tilewidth="8" tileheight="8" infinite="0">
 <tileset firstgid="1" source="../tiles/cave.tsx"/>
 <tileset firstgid="3" name="crates" tilewidth="8" tileheight="8" tilecount="1" columns="1">
  <image source="crates.png" width="8" height="8"/>
 </tileset>
 <layer id="1" name="ground" width="6" height="4">
  <properties><property name="collision" type="bool" value="true"/></properties>
  <data encoding="csv">
2,0,0,0,0,3,
0,0,0,0,0,0,
0,0,0,0,0,0,
1,1,1,1,1,1
</data>
 </layer>
</map>
"#;

fn game(root: &Path) {
    write(
        root,
        "argus.toml",
        "name = \"tiled\"\nclock = \"continuous@60\"\ndefault_scene = \"room\"\nplugins = []\n",
    );
    strip(
        root,
        "assets/tiles/cave.png",
        &[[200, 40, 40, 255], [40, 40, 200, 255]],
    );
    strip(root, "assets/maps/crates.png", &[[230, 210, 30, 255]]);
    write(root, "assets/tiles/cave.tsx", SET);
    write(root, "assets/maps/room.tmx", MAP);
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

/// The player's world with its tiles, 160 x 80, drawn in software.
fn frame(p: &mut Player) -> Option<argus_render::Image> {
    let mut r = match Renderer::offscreen(160, 80, true) {
        Ok(r) => r,
        Err(e) if e.code == argus_render::codes::NO_ADAPTER => return None,
        Err(e) => panic!("{e}"),
    };
    r.load_catalog(&p.assets).unwrap();
    let view = r.view(&p.world);
    let extras = p.tile_extras(view);
    assert_eq!(extras.len(), 8, "the floor, the moss, and the crate");
    r.render_with(&p.world, 0, 1.0, &extras).unwrap();
    Some(r.capture().unwrap())
}

#[test]
fn a_game_on_a_tmx_ships_and_plays_from_its_packs_as_from_its_folder() {
    let t = Temp(std::env::temp_dir().join(format!("argus-release-tiled-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&*t);
    let root = t.join("game-src");
    game(&root);

    let built = pack::build(&root, &BuildOptions::default()).unwrap();
    let r = &built.report;
    let left: Vec<&str> = r["sources_left_out"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(left.contains(&"assets/tiles/cave.png"), "{left:?}");
    assert!(left.contains(&"assets/maps/crates.png"), "{left:?}");
    let files: Vec<&str> = built.packs[0].files.iter().map(|f| f.0.as_str()).collect();
    assert!(
        files.contains(&"assets/maps/room.tmx"),
        "the map ships as a file: {files:?}"
    );
    assert!(files.contains(&argus_assets::CATALOG));
    assert!(!files.contains(&"assets/tiles/cave.png"));
    let catalog: serde_json::Value = serde_json::from_slice(
        &built.packs[0].blobs[&built.packs[0]
            .files
            .iter()
            .find(|f| f.0 == argus_assets::CATALOG)
            .unwrap()
            .1],
    )
    .unwrap();
    let names: Vec<&str> = catalog["sprites"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"tiles/cave") && names.contains(&"maps/room.tmx/crates"),
        "{names:?}"
    );

    let data = t.join("data");
    std::fs::create_dir_all(&data).unwrap();
    for p in &built.packs {
        std::fs::write(data.join(p.file_name()), p.encode()).unwrap();
    }
    let mounted = t.join("game");
    pack::mount_dir(&data, &mounted, None).unwrap();

    let mut a = Player::open(&root, None, None).unwrap();
    let mut b = Player::open(&mounted, None, None).unwrap();
    assert!(
        b.assets.failures().is_empty(),
        "{:?}",
        b.assets.failures().keys().collect::<Vec<_>>()
    );
    a.step(60).unwrap();
    b.step(60).unwrap();
    // It landed on the map's floor (its top at y = 1), in both.
    let (pa, pb) = (a.position("@body").unwrap(), b.position("@body").unwrap());
    assert!((pa[1] - 1.4).abs() < 1e-3, "{pa:?}");
    assert_eq!(pa, pb);
    assert_eq!(a.digest(), b.digest());
    let (Some(fa), Some(fb)) = (frame(&mut a), frame(&mut b)) else {
        return;
    };
    let red = fa
        .rgba
        .chunks(4)
        .filter(|p| p[0] > 150 && p[1] < 90)
        .count();
    assert!(red > 1000, "the floor is drawn: {red} red pixels");
    let c = fb.compare(&fa, 0.1).unwrap();
    assert_eq!(
        c.differing, 0,
        "{} of {} pixels differ",
        c.differing, c.pixels
    );
}
