//! A scene's parented entity with a Transform sits relative to its parent:
//! the loader gives it the GlobalTransform the hierarchy fills. Without one
//! it drew at its local position in world space, and nothing said so (the
//! platformer template's flag, distribution item 2).

mod common;

use argus_ecs::{hierarchy, GlobalTransform};
use argus_scene::Scene;
use common::*;

#[test]
fn a_scene_child_with_a_transform_sits_relative_to_its_parent() {
    let t = temp("parent-global");
    write(
        t.path(),
        "argus.toml",
        "name = \"g\"\nclock = \"continuous@60\"\ndefault_scene = \"s\"\nplugins = []\n",
    );
    write(
        t.path(),
        "scenes/s/scene.toml",
        "clock = \"continuous@60\"\nseed = 1\n",
    );
    // The pole has its GlobalTransform; its flag, a child, does not say one.
    write(
        t.path(),
        "scenes/s/pole.toml",
        "[Transform]\npos = [10.0, 2.0]\n\n[GlobalTransform]\n\n[children.flag.Transform]\npos = [0.5, 3.0]\n",
    );
    let p = open(t.path());
    let mut w = world();
    let mut s = Scene::open(&p, "s", w.registry()).unwrap();
    s.spawn_all(&mut w).unwrap();
    hierarchy::propagate(&mut w).unwrap();
    let flag = w
        .named_entities()
        .into_iter()
        .find(|(id, _)| id.ends_with("flag"))
        .map(|(_, e)| e)
        .expect("the flag");
    let g = w
        .get::<GlobalTransform>(flag)
        .expect("the flag has a GlobalTransform");
    assert_eq!(
        (g.pos.x, g.pos.y),
        (10.5, 5.0),
        "the flag is beside its pole"
    );
}
