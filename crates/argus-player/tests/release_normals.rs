//! Phase 5 item 3: "normal-mapped sprites optional". A sprite's normal map
//! is `<name>.normal.png` beside its image: imported with it, drawn by the
//! lighting from the project's folder and from a release pack alike, and
//! kept on a texture of its own in the pack (not a shared page) so the
//! sprite and its normals are one layout. One that does not fit is refused
//! with why.

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

/// Every pixel's normal leaning toward +x.
fn leaning(w: u32, h: u32) -> Vec<u8> {
    [230u8, 128, 204, 255].repeat((w * h) as usize)
}

fn normal_mapped(p: &Player, name: &str) -> bool {
    let Ok(mut r) = Renderer::offscreen(64, 36, true) else {
        return true;
    };
    r.load_catalog(&p.assets).unwrap();
    r.gpu().atlas(name).is_some_and(|a| a.normal_mapped())
}

#[test]
fn a_gem_with_a_normal_map_is_lit_by_it_from_its_folder_and_its_pack() {
    let t =
        Temp(std::env::temp_dir().join(format!("argus-release-normals-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&*t);
    let root = t.join("hollow");
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hollow"),
        &root,
    );
    let (w, h, _) = argus_assets::decode_png(&root.join("assets/gem.png")).unwrap();
    argus_assets::encode_png(&root.join("assets/gem.normal.png"), w, h, &leaning(w, h)).unwrap();

    // From the folder: the gem has its normals, and no sprite named for them.
    let a = Player::open(&root, None, None).unwrap();
    let gem = a.assets.sprites().find(|(n, _)| *n == "gem").unwrap().1;
    assert!(gem.normal.is_some(), "imported with the gem");
    assert!(
        a.assets.sprites().all(|(n, _)| n != "gem.normal"),
        "a normal map is no sprite of its own"
    );
    assert!(normal_mapped(&a, "gem"));

    // From the pack: off the shared pages, its normals beside it.
    let built = pack::build(&root, &BuildOptions::default()).unwrap();
    let pages = &built.report["pages"];
    assert_eq!(pages["normal_mapped"], 1, "{pages}");
    let data = t.join("data");
    std::fs::create_dir_all(&data).unwrap();
    for p in &built.packs {
        std::fs::write(data.join(p.file_name()), p.encode()).unwrap();
    }
    let mounted = t.join("game");
    pack::mount_dir(&data, &mounted, None).unwrap();
    let b = Player::open(&mounted, None, None).unwrap();
    assert!(normal_mapped(&b, "gem"), "the pack's gem has its normals");
    if let Ok(mut r) = Renderer::offscreen(64, 36, true) {
        r.load_catalog(&b.assets).unwrap();
        let page = r.gpu().atlas("gem").unwrap().page;
        let shared = r
            .gpu()
            .atlas_names()
            .iter()
            .filter(|n| r.gpu().atlas(n).is_some_and(|x| x.page == page))
            .count();
        assert_eq!(shared, 1, "the gem's texture is its own");
    }
}

#[test]
fn a_normal_map_that_does_not_fit_is_refused_with_why() {
    let t = Temp(std::env::temp_dir().join(format!("argus-bad-normals-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&*t);
    let root = t.join("hollow");
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hollow"),
        &root,
    );
    // The wrong size, and one with no sprite.
    argus_assets::encode_png(&root.join("assets/gem.normal.png"), 3, 3, &leaning(3, 3)).unwrap();
    argus_assets::encode_png(
        &root.join("assets/nothing.normal.png"),
        4,
        4,
        &leaning(4, 4),
    )
    .unwrap();
    let mut imp = argus_assets::Importer::new(&root);
    imp.scan();
    let fails = imp.failures();
    let why = |n: &str| {
        fails
            .get(n)
            .map(|f| f.error.message.clone())
            .unwrap_or_default()
    };
    assert!(
        why("gem").contains("a normal map is its sprite's size"),
        "{}",
        why("gem")
    );
    assert!(
        why("nothing").contains("no nothing.png"),
        "{}",
        why("nothing")
    );
}
