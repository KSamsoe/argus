//! Issue 24: Outpost's systems share code through modules in `lib/` (its
//! clock and its UI builders). A release build compiles `lib/` into the
//! pack with the other scripts and leaves `tests/lib/` out with the rest of
//! `tests/`; Outpost from its packs plays as Outpost from its folder. A
//! mod's own `lib/` goes into the mod's pack.

use argus_player::pack::{self, BuildOptions, Pack};
use argus_player::Player;
use std::path::{Path, PathBuf};

fn folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/outpost")
}

/// A temp directory removed on drop, used as its path.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for Dir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn temp(tag: &str) -> Dir {
    let dir = std::env::temp_dir().join(format!("argus-modules-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Dir(dir)
}

/// The project's files but its dot folders and its builds.
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

/// Packs written as a build's data/ directory and mounted at a root that
/// is nowhere on disk.
fn mount(packs: &[Pack]) -> (Dir, PathBuf) {
    let base = temp("data");
    let data = base.join("data");
    std::fs::create_dir_all(&data).unwrap();
    for p in packs {
        std::fs::write(data.join(p.file_name()), p.encode()).unwrap();
    }
    let root = base.join("game");
    pack::mount_dir(&data, &root, None).unwrap();
    (base, root)
}

/// What a pack holds at `path`.
fn blob<'a>(p: &'a Pack, path: &str) -> &'a [u8] {
    let (_, h, _) = p.files.iter().find(|f| f.0 == path).unwrap();
    &p.blobs[h]
}

#[test]
fn outpost_from_its_packs_shares_its_modules_and_plays_as_from_its_folder() {
    let dir = temp("game");
    copy(&folder(), &dir);
    let built = pack::build(
        &dir,
        &BuildOptions {
            code: "test".into(),
            since: vec![],
            ..Default::default()
        },
    )
    .unwrap();
    let game = &built.packs[0];
    let files: Vec<&str> = game.files.iter().map(|f| f.0.as_str()).collect();
    for m in ["lib/clock.luau", "lib/ui.luau"] {
        assert!(files.contains(&m), "{m} ships: {files:?}");
        assert!(
            !blob(game, m).starts_with(b"--"),
            "{m} ships compiled, as the systems do"
        );
    }
    assert!(
        !files.iter().any(|f| f.starts_with("tests/")),
        "tests/lib/ stays out with the rest of tests/: {files:?}"
    );

    let (_data, root) = mount(&built.packs);
    let mut a = Player::open(&dir, None, None).unwrap();
    let mut b = Player::open(&root, None, None).unwrap();
    // A day and a half: the HUD's hour turns through dusk, and the menu
    // and the inspector are built, from the same modules.
    for _ in 0..9 {
        a.step(600).unwrap();
        b.step(600).unwrap();
        assert_eq!(b.digest(), a.digest(), "at tick {}", a.tick());
    }
    let hud = |p: &Player| {
        let id = p.world.component_id("Hud").unwrap();
        let text = p.world.component_id("UiText").unwrap();
        let mut lines: Vec<String> = p
            .world
            .query()
            .has(id)
            .has(text)
            .entities()
            .into_iter()
            .map(|e| format!("{:?}", p.world.get_field(e, "UiText.text").unwrap()))
            .collect();
        lines.sort();
        lines
    };
    assert_eq!(hud(&b), hud(&a));
    assert!(
        hud(&a).iter().any(|l| l.contains(':')),
        "the HUD shows the hour: {:?}",
        hud(&a)
    );
}

#[test]
fn a_mods_own_modules_go_into_its_pack() {
    let dir = temp("mod");
    copy(&folder(), &dir);
    let m = dir.join("mods/tally");
    std::fs::create_dir_all(m.join("lib")).unwrap();
    std::fs::create_dir_all(m.join("systems")).unwrap();
    std::fs::write(m.join("mod.toml"), "id = \"tally\"\nname = \"Tally\"\n").unwrap();
    std::fs::write(
        m.join("lib/count.luau"),
        "-- Counting.\nreturn { twice = function(n) return 2 * n end }\n",
    )
    .unwrap();
    std::fs::write(
        m.join("systems/tally.luau"),
        "local count = require(\"count\")\nlocal clock = require(\"clock\")\nreturn system \"tally.sim.count\" { stage = \"sim\", run = function(q) local _ = count.twice(clock.hour(0.5, 0.75)) end }\n",
    )
    .unwrap();
    let built = pack::build_mod(
        &dir,
        "tally",
        &BuildOptions {
            code: "test".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let p = &built.packs[0];
    let files: Vec<&str> = p.files.iter().map(|f| f.0.as_str()).collect();
    assert!(files.contains(&"lib/count.luau"), "{files:?}");
    assert!(!blob(p, "lib/count.luau").starts_with(b"--"), "compiled");
}
