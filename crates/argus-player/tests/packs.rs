//! §15 packs (phase 2 item 7): Hollow built into release packs plays exactly
//! as Hollow from its folder; the packs carry the import cache and compiled
//! scripts, not the source art or source scripts; a later build ships as a
//! patch holding only the blobs that changed; a language pack lays its files
//! over the game's.

use argus_player::pack::{self, BuildOptions, Kind, Pack};
use argus_player::Player;
use serde_json::json;
use std::path::{Path, PathBuf};

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

/// A copy of Hollow in a temp directory (a build writes the import cache).
fn hollow(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("argus-packs-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hollow"),
        &dir,
    );
    dir
}

/// The world's summary at a checkpoint. The Luau heap's size is left out:
/// it depends on when the collector ran, and on whether the scripts were
/// compiled here or shipped as bytecode.
fn world(p: &Player) -> String {
    let mut s = p.status();
    s.as_object_mut().unwrap().remove("script_bytes");
    s.to_string()
}

/// Hollow's run scenario, as `tests/player.rs` has it.
fn run(p: &mut Player) -> Vec<String> {
    let mut out = vec![world(p)];
    p.inject("move", Some(&json!([1, 0])), Some(450), false)
        .unwrap();
    p.step(118).unwrap();
    out.push(world(p));
    p.inject("jump", Some(&json!(true)), Some(18), false)
        .unwrap();
    p.step(330).unwrap();
    out.push(world(p));
    p.step(600).unwrap();
    out.push(world(p));
    out
}

/// Write packs as a build's data/ directory, and mount it at a root that
/// is nowhere on disk.
fn mount(packs: &[Pack], tag: &str, lang: Option<&str>) -> (PathBuf, serde_json::Value) {
    let base = std::env::temp_dir().join(format!("argus-packs-data-{tag}-{}", std::process::id()));
    let data = base.join("data");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&data).unwrap();
    for p in packs {
        std::fs::write(data.join(p.file_name()), p.encode()).unwrap();
    }
    let root = base.join("game");
    let report = pack::mount_dir(&data, &root, lang).unwrap();
    (root, report)
}

#[test]
fn hollow_from_its_packs_plays_as_hollow_from_its_folder() {
    let dir = hollow("same");
    let built = pack::build(
        &dir,
        &BuildOptions {
            code: "test".into(),
            since: vec![],
        },
    )
    .unwrap();
    let r = &built.report;
    println!("{r:#}");
    // The game, and French: its string table travels in a pack of its own
    // (phase 3 item 4), so a language can be added without a rebuild.
    assert_eq!(built.packs.len(), 2);
    assert_eq!(built.packs[0].kind, Kind::Base);
    assert_eq!(built.packs[1].kind, Kind::Lang("fr".into()));
    assert!(built.packs[1]
        .files
        .iter()
        .any(|f| f.0 == "strings/fr.toml"));
    assert!(!built.packs[0]
        .files
        .iter()
        .any(|f| f.0 == "strings/fr.toml"));
    assert!(
        built.packs[0]
            .files
            .iter()
            .any(|f| f.0 == "strings/en.toml"),
        "the base's is the game's"
    );
    assert_eq!(
        r["scripts_compiled"], 10,
        "the Run and Hermit components, and the pit, run, signs, pause, pause menu, hermit, music, and light systems"
    );
    assert!(r["sprites"].as_u64().unwrap() >= 3, "{r}");
    let left_out = r["sources_left_out"].to_string();
    assert!(left_out.contains("assets/hero.png") && left_out.contains("assets/hero.toml"));
    // A pack reads back as it was written.
    let bytes = built.packs[0].encode();
    assert_eq!(Pack::decode(&bytes).unwrap(), built.packs[0]);
    let mut damaged = bytes.clone();
    let n = damaged.len();
    damaged[n - 3] ^= 1;
    assert!(Pack::decode(&damaged).is_err(), "a damaged blob is refused");

    let (root, report) = mount(&built.packs, "same", None);
    // Every language's strings are there, in English or not, to switch to.
    assert!(argus_os::fs::exists(root.join("strings/fr.toml")));
    assert_eq!(
        report["files"].as_u64().unwrap(),
        r["files"].as_u64().unwrap() + 1,
        "the game's files and French's table"
    );
    assert!(!root.exists(), "the game is in memory");
    assert!(
        !argus_os::fs::exists(root.join("assets/hero.png")),
        "no source art"
    );
    assert!(!argus_os::fs::exists(root.join("tests")), "no tests");
    let script = argus_os::fs::read(root.join("systems/pit.luau")).unwrap();
    assert!(script[0] < b'\n', "the script ships as bytecode");

    let from_folder = run(&mut Player::open(&dir, None, None).unwrap());
    let from_packs = run(&mut Player::open(&root, None, None).unwrap());
    assert_eq!(
        from_packs, from_folder,
        "the same world at every checkpoint"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_patch_carries_only_what_changed_and_a_language_lays_over_the_game() {
    let dir = hollow("patch");
    let first = pack::build(
        &dir,
        &BuildOptions {
            code: "one".into(),
            since: vec![],
        },
    )
    .unwrap();
    // The next build: one sign says something else, and French arrives.
    // (Its words are set whole, not searched for: the game's own change
    // them, as the phase 2 gate's proposal did.)
    let sign = dir.join("scenes/hollow/rooms/b/sign.toml");
    let text = std::fs::read_to_string(&sign).unwrap();
    let says = |words: &str| {
        let out: Vec<String> = text
            .lines()
            .map(|l| {
                if l.starts_with("text = ") {
                    format!("text = \"{words}\"")
                } else {
                    l.to_string()
                }
            })
            .collect();
        assert_ne!(out.join("\n"), text.trim_end(), "the sign has a text line");
        out.join("\n") + "\n"
    };
    std::fs::write(&sign, says("The top of the shaft. Nothing but the wind.")).unwrap();
    let fr = dir.join("lang/fr/scenes/hollow/rooms/b/sign.toml");
    std::fs::create_dir_all(fr.parent().unwrap()).unwrap();
    std::fs::write(&fr, says("Le haut du puits. Rien ici que la vue.")).unwrap();
    let second = pack::build(
        &dir,
        &BuildOptions {
            code: "two".into(),
            since: first.packs.clone(),
        },
    )
    .unwrap();
    let r = &second.report;
    assert_eq!(r["kind"], "patch");
    assert_eq!(r["build"], 2);
    assert_eq!(r["blobs_new"], 1, "only the changed sign: {r}");
    assert_eq!(second.packs[0].file_name(), "patch-2.pack");
    assert_eq!(second.packs[1].file_name(), "lang-fr.pack");
    // Only what changed, manifest included (phase 3 item 6: at Hollow's
    // 10,000 files a whole manifest was most of a one-file patch): the
    // sign's room, whose entities a release pack holds in one bundle (phase
    // 5 item 1).
    assert_eq!(
        second.packs[0]
            .files
            .iter()
            .map(|f| f.0.as_str())
            .collect::<Vec<_>>(),
        ["scenes/hollow/rooms/b/entities.bin"]
    );
    assert!(second.packs[0].removed.is_empty());
    assert!(second.packs[0].encode().len() < first.packs[0].encode().len() / 100);

    // The patch alone is not a game.
    let e = pack::resolve(&second.packs[..1], None).unwrap_err();
    assert!(
        e.message.contains("a patch needs the packs before it"),
        "{e:?}"
    );

    let mut all = first.packs.clone();
    all.extend(second.packs.iter().cloned());
    let file = |files: &[(String, Vec<u8>)], path: &str| {
        files
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, b)| b.clone())
    };
    let (files, report) = pack::resolve(&all, None).unwrap();
    let bundle = file(&files, "scenes/hollow/rooms/b/entities.bin").unwrap();
    let docs = argus_scene::binary::decode_entities(&bundle, &|stem| {
        format!("scenes/hollow/rooms/b/{stem}.toml")
    })
    .unwrap();
    let (_, sign) = docs.iter().find(|(stem, _)| stem == "sign").unwrap();
    let describe = sign
        .components
        .iter()
        .find(|c| c.name == "Describe")
        .unwrap();
    let text = describe.fields.iter().find(|f| f.name == "text").unwrap();
    assert!(
        text.raw.to_string().contains("but the wind"),
        "{:?}",
        text.raw
    );
    assert_eq!(report["build"], 2);
    assert_eq!(report["languages"], json!(["fr"]));
    let (files, report) = pack::resolve(&all, Some("fr")).unwrap();
    let fr = file(&files, "scenes/hollow/rooms/b/sign.toml").unwrap();
    assert!(String::from_utf8(fr)
        .unwrap()
        .contains("Rien ici que la vue"));
    assert_eq!(report["language"], "fr");

    // Played in French: the sign says so.
    let (root, _) = mount(&all, "fr", Some("fr"));
    let p = Player::open(&root, None, None).unwrap();
    let e = p.world.entity("@b/sign").unwrap();
    assert_eq!(
        p.world.get_field(e, "Describe.text").unwrap(),
        argus_ecs::Value::Str("Le haut du puits. Rien ici que la vue.".into())
    );
    let _ = std::fs::remove_dir_all(&dir);
}
