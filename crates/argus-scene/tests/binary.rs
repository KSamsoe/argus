//! Defs and scenes in a binary form of the same schema (section 15; phase 5
//! item 1). The fixture's entity files, prefabs, and def tables, packed as
//! a release pack holds them, load as the same world and the same tables,
//! and an error in them names the source line it came from.

mod common;

use argus_scene::binary::{self, Packed};
use argus_scene::{Project, Scene};
use common::*;
use std::path::Path;

fn files(dir: &Path, base: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, base, out);
        } else {
            let rel = p
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, std::fs::read(&p).unwrap()));
        }
    }
}

/// `src` written to `dst` as a pack holds it.
fn packed_copy(src: &Path, dst: &Path) -> Packed {
    let mut all = Vec::new();
    files(src, src, &mut all);
    let packed = binary::pack(&mut all, &|p| p.to_string());
    for (rel, bytes) in all {
        write_bytes(dst, &rel, &bytes);
    }
    packed
}

fn write_bytes(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

fn spawned(p: &Project, scene: &str) -> u64 {
    let mut w = world();
    let mut s = Scene::open(p, scene, w.registry()).unwrap();
    assert!(s.issues().is_empty(), "{scene}: {:?}", s.issues());
    s.spawn_all(&mut w).unwrap();
    w.digest()
}

#[test]
fn the_binary_forms_load_as_the_same_world_and_tables() {
    let (text, bin) = (temp("binary-text"), temp("binary-bin"));
    project(text.path());
    let packed = packed_copy(text.path(), bin.path());
    assert!(packed.left_as_toml.is_empty(), "{:?}", packed.left_as_toml);
    assert_eq!(packed.tables, 2, "items and recipes");
    assert!(packed.prefabs > 0 && packed.bundles > 0, "{packed:?}");
    assert!(!bin.path().join("defs/items.toml").exists());
    assert!(bin.path().join("defs/items.bin").exists());
    assert!(!bin.path().join("scenes/arena/spawner.north.toml").exists());
    assert!(bin.path().join("scenes/arena/entities.bin").exists());
    assert!(!bin.path().join("prefabs/door/base.toml").exists());

    let (a, b) = (open(text.path()), open(bin.path()));
    for t in ["items", "recipes"] {
        assert_eq!(a.defs.table(t), b.defs.table(t), "{t}");
    }
    for scene in ["arena", "hollow"] {
        assert_eq!(spawned(&a, scene), spawned(&b, scene), "{scene}");
    }
}

#[test]
fn an_error_in_a_binary_form_names_its_source_line() {
    let (text, bin) = (temp("binary-err-text"), temp("binary-err-bin"));
    project(text.path());
    // A row the table refuses when it types it: the TOML is fine.
    let items = read(text.path(), "defs/items.toml");
    write(
        text.path(),
        "defs/items.toml",
        &format!("{items}\n[\"lead\"]\nname = \"Lead\"\nweight = \"heavy\"\n"),
    );
    // And an entity file that is not TOML at all: it stays as it is.
    write(
        text.path(),
        "scenes/arena/broken.toml",
        "[Transform\npos = 1\n",
    );
    let packed = packed_copy(text.path(), bin.path());
    assert_eq!(packed.left_as_toml, ["scenes/arena/broken.toml"]);

    let issues = |root: &Path| -> Vec<(String, Option<String>)> {
        let p = Project::open(root, def_schemas()).unwrap();
        let mut out: Vec<_> = p
            .issues()
            .iter()
            .map(|e| (e.message.clone(), e.r#ref.clone()))
            .collect();
        let w = world();
        let s = Scene::open(&p, "arena", w.registry()).unwrap();
        out.extend(
            s.issues()
                .iter()
                .map(|e| (e.message.clone(), e.r#ref.clone())),
        );
        out
    };
    let (a, b) = (issues(text.path()), issues(bin.path()));
    assert_eq!(a, b);
    assert!(
        a.iter()
            .any(|(_, at)| at.as_deref() == Some("defs/items.toml:10")),
        "{a:?}"
    );
    assert!(
        a.iter()
            .any(|(m, _)| m.contains("broken.toml is not valid TOML")),
        "{a:?}"
    );
}
