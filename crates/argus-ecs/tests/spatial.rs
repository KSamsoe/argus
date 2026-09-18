//! §12 "one broadphase: a spatial hash over the live world, rebuilt each
//! tick"; §17 "world.query over 100k entities < 2 ms: spatial predicates
//! hit the broadphase hash, not a scan"; §19 batch spawn.

use argus_ecs::{Component, Transform, Vec2, World};

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
struct Tag {
    n: u32,
}

fn at(w: &mut World, x: f32, y: f32) -> argus_ecs::Entity {
    let e = w.spawn();
    w.insert(
        e,
        Transform {
            pos: Vec2::new(x, y),
            ..Default::default()
        },
    )
    .unwrap();
    e
}

#[test]
fn within_and_nearest_come_from_the_hash_with_exact_distances() {
    let mut w = World::new();
    w.register::<Tag>().unwrap();
    let a = at(&mut w, 0.0, 0.0);
    let b = at(&mut w, 3.0, 4.0); // 5 away
    let c = at(&mut w, 10.0, 0.0);
    let _far = at(&mut w, 100.0, 100.0);
    let tagged = at(&mut w, 1.0, 1.0);
    w.insert(tagged, Tag { n: 1 }).unwrap();

    assert!(
        w.within(0.0, 0.0, 6.0).is_empty(),
        "nothing before a rebuild"
    );
    assert_eq!(w.rebuild_broadphase(), 5);

    let near: Vec<_> = w.within(0.0, 0.0, 6.0);
    assert_eq!(near.len(), 3, "{near:?}");
    assert_eq!(near[0].0, a);
    assert_eq!(near[1].0, tagged);
    assert!((near[2].1 - 5.0).abs() < 1e-5);
    assert_eq!(near[2].0, b);
    assert!(
        w.within(0.0, 0.0, 4.99).iter().all(|(e, _)| *e != b),
        "the edge is exact"
    );
    assert_eq!(w.within(50.0, 50.0, 1.0).len(), 0);

    let n = w.broadphase().nearest(9.0, 0.0, 20.0, |_| true).unwrap();
    assert_eq!(n.0, c);
    assert!((n.1 - 1.0).abs() < 1e-5);
    let only_tagged = w
        .broadphase()
        .nearest(9.0, 0.0, 20.0, |e| w.has_type::<Tag>(e))
        .unwrap();
    assert_eq!(only_tagged.0, tagged);
    assert!(w.broadphase().nearest(9.0, 0.0, 0.5, |_| true).is_none());

    // A despawned entity drops out of `within` even before the next rebuild.
    w.despawn(c).unwrap();
    assert!(w.within(10.0, 0.0, 0.1).is_empty());
}

#[test]
fn spawn_with_places_the_entity_in_its_archetype_at_defaults() {
    let mut w = World::new();
    let tag = w.register::<Tag>().unwrap();
    let tr = w.register::<Transform>().unwrap();
    let before = w.archetypes().len();
    let e = w.spawn_with(&[tr, tag, tag]).unwrap();
    assert!(w.has(e, tag) && w.has(e, tr));
    assert_eq!(w.get::<Transform>(e).unwrap(), Transform::default());
    assert_eq!(
        w.archetypes().len(),
        before + 1,
        "one archetype, reached in one move"
    );
    let e2 = w.spawn_with(&[tag, tr]).unwrap();
    assert_eq!(
        w.components_of(e).unwrap(),
        w.components_of(e2).unwrap(),
        "order does not matter"
    );
    assert!(w.spawn_with(&[argus_ecs::ComponentId(999)]).is_err());
}
