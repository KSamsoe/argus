//! Phase 5 item 4: an Aseprite sprite ships as any sprite does. Hollow's
//! hero redrawn as `hero.aseprite` (the same eight frames, tags for its
//! clips, its sidecar still naming the rest) looks as the PNG hero did from
//! the project's folder, and from a release pack, which carries the import
//! cache and leaves the Aseprite file out.

#[path = "../../argus-assets/tests/ase_writer/mod.rs"]
mod ase_writer;

use argus_player::pack::{self, BuildOptions};
use argus_player::Player;
use argus_render::Renderer;
use ase_writer::{stored_zlib, Ase};
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

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let name = e.file_name();
        let n = name.to_string_lossy();
        if n.starts_with('.') || n == "dist" {
            continue;
        }
        let (a, b) = (e.path(), to.join(&name));
        if a.is_dir() {
            copy(&a, &b);
        } else {
            std::fs::copy(&a, &b).unwrap();
        }
    }
}

/// A frame of the player's world at 640 x 360, drawn in software.
fn frame(p: &Player) -> Option<argus_render::Image> {
    let mut r = match Renderer::offscreen(640, 360, true) {
        Ok(r) => r,
        Err(e) if e.code == argus_render::codes::NO_ADAPTER => return None,
        Err(e) => panic!("{e}"),
    };
    r.load_catalog(&p.assets).unwrap();
    r.render(&p.world, 0, 1.0).unwrap();
    Some(r.capture().unwrap())
}

fn hero(p: &Player) -> argus_assets::SpriteAsset {
    p.assets
        .sprites()
        .find(|(n, _)| *n == "hero")
        .unwrap()
        .1
        .clone()
}

#[test]
fn hollows_hero_from_an_aseprite_file_looks_as_the_png_from_its_folder_and_its_pack() {
    let t =
        Temp(std::env::temp_dir().join(format!("argus-release-aseprite-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&*t);
    let hollow = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hollow");
    let png_root = t.join("png");
    let root = t.join("ase");
    copy(&hollow, &png_root);
    copy(&hollow, &root);

    // The hero's eight 8 x 13 frames, as an artist would have them in
    // Aseprite: a frame each, some cels compressed, tags for the clips.
    let (w, h, px) = argus_assets::decode_png(&root.join("assets/hero.png")).unwrap();
    assert_eq!((w, h), (64, 13));
    let mut a = Ase::new(8, 13, 32);
    for f in 0..8u32 {
        let cel: Vec<u8> = (0..13u32)
            .flat_map(|y| {
                let at = ((y * w + f * 8) * 4) as usize;
                px[at..at + 32].to_vec()
            })
            .collect();
        a.frame(100);
        if f == 0 {
            a.image_layer("hero").tags(&[
                (0, 0, 0, 0, "idle"),
                (1, 4, 0, 0, "run"),
                (5, 5, 0, 0, "jump"),
                (6, 6, 0, 0, "fall"),
            ]);
        }
        if f % 2 == 0 {
            a.cel(0, 0, 0, 8, 13, &cel);
        } else {
            a.cel_zlib(0, 0, 0, 8, 13, &stored_zlib(&cel));
        }
    }
    std::fs::remove_file(root.join("assets/hero.png")).unwrap();
    std::fs::write(root.join("assets/hero.aseprite"), a.bytes()).unwrap();
    // The sidecar keeps its clips (land is two frames no tag has); the
    // grid is the file's now.
    let side = std::fs::read_to_string(root.join("assets/hero.toml")).unwrap();
    let side: String = side
        .lines()
        .filter(|l| !l.starts_with("grid"))
        .map(|l| format!("{l}\n"))
        .collect();
    std::fs::write(root.join("assets/hero.toml"), side).unwrap();

    let mut png = Player::open(&png_root, None, None).unwrap();
    let mut ase = Player::open(&root, None, None).unwrap();
    let e = ase.assets.get("hero").unwrap();
    assert!(e.warnings.is_empty(), "{:?}", e.warnings);
    let (hp, ha) = (hero(&png), hero(&ase));
    assert_eq!(ha.source.as_deref(), Some("hero.aseprite"));
    assert_eq!(
        ha.clips, hp.clips,
        "the tags and the sidecar make the same clips"
    );
    assert_eq!(ha.frames.len(), 8);
    for (fa, fp) in ha.frames.iter().zip(&hp.frames) {
        assert_eq!(
            (fa.pivot, fa.duration_ms, &fa.properties),
            (fp.pivot, fp.duration_ms, &fp.properties)
        );
    }
    assert_eq!(ha.texture_size, [32, 32], "four by two, not 64 x 16");

    // The release pack: the cache's texture on a page, the file left out.
    let built = pack::build(&root, &BuildOptions::default()).unwrap();
    let left: Vec<&str> = built.report["sources_left_out"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    assert!(left.contains(&"assets/hero.aseprite"), "{left:?}");
    assert!(left.contains(&"assets/hero.toml"), "{left:?}");
    assert!(
        !built.packs[0]
            .files
            .iter()
            .any(|f| f.0.ends_with(".aseprite")),
        "no Aseprite file in the pack"
    );
    let data = t.join("data");
    std::fs::create_dir_all(&data).unwrap();
    for p in &built.packs {
        std::fs::write(data.join(p.file_name()), p.encode()).unwrap();
    }
    let mounted = t.join("game");
    pack::mount_dir(&data, &mounted, None).unwrap();
    let mut packed = Player::open(&mounted, None, None).unwrap();
    let hb = hero(&packed);
    assert_eq!(hb.clips, ha.clips);
    assert_eq!(
        hb.frames.iter().map(|f| f.duration_ms).collect::<Vec<_>>(),
        ha.frames.iter().map(|f| f.duration_ms).collect::<Vec<_>>()
    );
    assert!(packed.assets.failures().is_empty());

    // The hero at the cave mouth, later, then running and stopping: the
    // same pixels from the PNG, the Aseprite file, and the pack.
    let check = |png: &Player, ase: &Player, packed: &Player, when: &str| -> bool {
        let (Some(fp), Some(fa), Some(fb)) = (frame(png), frame(ase), frame(packed)) else {
            return false;
        };
        for (other, what) in [(&fa, "the Aseprite file"), (&fb, "the pack")] {
            let c = other.compare(&fp, 0.1).unwrap();
            assert_eq!(
                c.differing, 0,
                "{when}: {} of {} pixels differ from the PNG hero in {what}",
                c.differing, c.pixels
            );
        }
        true
    };
    for steps in [1, 240] {
        for p in [&mut png, &mut ase, &mut packed] {
            p.step(steps).unwrap();
        }
        if !check(&png, &ase, &packed, &format!("after {steps} more ticks")) {
            eprintln!("no GPU adapter here: the pixels were not compared");
            return;
        }
    }
    for p in [&mut png, &mut ase, &mut packed] {
        p.inject("move", Some(&serde_json::json!([1, 0])), Some(40), false)
            .unwrap();
    }
    for tick in 1..=50 {
        for p in [&mut png, &mut ase, &mut packed] {
            p.step(1).unwrap();
        }
        if tick % 5 == 0 {
            check(&png, &ase, &packed, &format!("{tick} ticks into the run"));
        }
    }
}
