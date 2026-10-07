//! Section 15 (phase 5 item 1): "compressed textures in atlases". A release
//! pack holds Hollow's sprites on shared pages, BC7 only where nobody could
//! tell, and Hollow drawn from its packs looks as Hollow drawn from its
//! folder: the same frames, from the same places, at the same edges.

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

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let name = e.file_name();
        if name.to_string_lossy().starts_with('.') {
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

#[test]
fn hollow_on_pages_looks_as_hollow_from_its_folder() {
    let t = Temp(std::env::temp_dir().join(format!("argus-release-pages-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&*t);
    let root = t.join("hollow");
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hollow"),
        &root,
    );
    let built = pack::build(&root, &BuildOptions::default()).unwrap();
    let pages = &built.report["pages"];
    assert_eq!(pages["sprites"], built.report["sprites"], "{pages}");
    assert_eq!(pages["too_big_for_a_page"], 0);
    let on_pages = pages["compressed"].as_u64().unwrap() + pages["lossless"].as_u64().unwrap();
    assert_eq!(Some(on_pages), pages["sprites"].as_u64());
    assert!(!pages["pages"].as_array().unwrap().is_empty());
    let game = &built.packs[0];
    assert!(
        !game
            .files
            .iter()
            .any(|f| f.0.starts_with(argus_assets::TEXTURES)),
        "no texture a sprite: {:?}",
        game.files
            .iter()
            .filter(|f| f.0.starts_with("argus.cache"))
            .collect::<Vec<_>>()
    );

    let data = t.join("data");
    std::fs::create_dir_all(&data).unwrap();
    for p in &built.packs {
        std::fs::write(data.join(p.file_name()), p.encode()).unwrap();
    }
    let mounted = t.join("game");
    pack::mount_dir(&data, &mounted, None).unwrap();

    // The hero at the cave mouth, and then in the shaft with the planks.
    let mut a = Player::open(&root, None, None).unwrap();
    let mut b = Player::open(&mounted, None, None).unwrap();
    for steps in [1, 240] {
        a.step(steps).unwrap();
        b.step(steps).unwrap();
        let (Some(fa), Some(fb)) = (frame(&a), frame(&b)) else {
            return;
        };
        // Something to compare: the cave's tiles, the hero, the HUD.
        let colours: std::collections::BTreeSet<&[u8]> = fa.rgba.chunks(4).collect();
        assert!(colours.len() > 16, "{} colours", colours.len());
        let c = fb.compare(&fa, 0.1).unwrap();
        assert_eq!(
            c.differing, 0,
            "after {steps} more ticks, {} of {} pixels differ",
            c.differing, c.pixels
        );
    }
}
