//! Phase 5 item 8: a game of Spine skeletons (the puppet example) ships
//! (`argus build`) and plays from its packs as from its folder: the
//! skeleton's JSON ships as a file, its images go through the sprite
//! catalog and its pages like any sprite, and the puppets pose, fire their
//! events, and draw the same.

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

fn puppet() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/puppet")
}

fn frame(p: &mut Player) -> Option<argus_render::Image> {
    let mut r = match Renderer::offscreen(320, 180, true) {
        Ok(r) => r,
        Err(e) if e.code == argus_render::codes::NO_ADAPTER => return None,
        Err(e) => panic!("{e}"),
    };
    r.load_catalog(&p.assets).unwrap();
    r.render(&p.world, 0, 1.0).unwrap();
    Some(r.capture().unwrap())
}

#[test]
fn the_puppets_ship_and_play_from_their_packs_as_from_their_folder() {
    let t =
        Temp(std::env::temp_dir().join(format!("argus-release-skeletal-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&*t);
    let built = pack::build(&puppet(), &BuildOptions::default()).unwrap();
    let files: Vec<&str> = built.packs[0].files.iter().map(|f| f.0.as_str()).collect();
    assert!(
        files.contains(&"assets/puppet/puppet.json"),
        "the skeleton ships as a file: {files:?}"
    );
    assert!(!files.contains(&"assets/puppet/images/head.png"));
    let left: Vec<&str> = built.report["sources_left_out"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        left.contains(&"assets/puppet/images/helmet.png"),
        "{left:?}"
    );

    let data = t.join("data");
    std::fs::create_dir_all(&data).unwrap();
    for p in &built.packs {
        std::fs::write(data.join(p.file_name()), p.encode()).unwrap();
    }
    let mounted = t.join("game");
    pack::mount_dir(&data, &mounted, None).unwrap();

    let mut a = Player::open(&puppet(), None, None).unwrap();
    let mut b = Player::open(&mounted, None, None).unwrap();
    assert!(
        b.assets.failures().is_empty(),
        "{:?}",
        b.assets.failures().keys().collect::<Vec<_>>()
    );
    // Through the idle, the walk, and the cued wave.
    for _ in 0..6 {
        a.step(50).unwrap();
        b.step(50).unwrap();
        assert_eq!(a.digest(), b.digest(), "tick {}", a.tick());
    }
    let waves = |p: &Player| {
        p.world
            .get_field(p.world.entity("@world").unwrap(), "Show.waves")
            .unwrap()
    };
    assert_eq!(waves(&a), argus_ecs::Value::UInt(1));
    assert_eq!(waves(&b), waves(&a));
    let (Some(fa), Some(fb)) = (frame(&mut a), frame(&mut b)) else {
        return;
    };
    // The puppets are drawn: their orange shirts and blue legs.
    let shirt = fa
        .rgba
        .chunks(4)
        .filter(|p| p[0] > 180 && (90..160).contains(&p[1]) && p[2] < 110)
        .count();
    assert!(shirt > 50, "the shirts are drawn: {shirt} pixels");
    // The packs draw what the folder draws, but for a few pixels on the
    // rims of the turned, shrunk heads: BC7, which their page is
    // compressed with, keeps a clear pixel's colour loosely, and a filtered
    // sample of a shrunk sprite mixes it into the rim. (Axis-aligned
    // sprites at their size, release_pages, match exactly; the pages'
    // judgement of BC7 is being fixed apart from this.)
    let c = fb.compare(&fa, 0.1).unwrap();
    assert!(
        c.differing * 2000 <= c.pixels,
        "{} of {} pixels differ",
        c.differing,
        c.pixels
    );
}
