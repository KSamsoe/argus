//! Inactive entities (issue 18): switched off, an entity and everything
//! under it leave every query that does not ask for them, and the
//! broadphase; they stay in the world, the digest, and a save.

use argus_ecs::{
    hierarchy, Entity, Inactive, InactiveUnder, Parent, SchemaBuilder, Transform, World,
};

fn world() -> (World, [Entity; 4]) {
    let mut w = World::new();
    hierarchy::register(&mut w).unwrap();
    w.register_schema(SchemaBuilder::new("Thing").build().unwrap())
        .unwrap();
    let mk = |w: &mut World, x: f32, parent: Option<Entity>| {
        let e = w.spawn();
        w.insert(e, Transform::at(x, 0.0)).unwrap();
        w.insert_by_name(e, "Thing").unwrap();
        if let Some(p) = parent {
            w.insert(e, Parent { entity: p }).unwrap();
        }
        e
    };
    let room = mk(&mut w, 0.0, None);
    let table = mk(&mut w, 1.0, Some(room));
    let cup = mk(&mut w, 2.0, Some(table));
    let hall = mk(&mut w, 50.0, None);
    (w, [room, table, cup, hall])
}

fn things(w: &World) -> Vec<Entity> {
    let c = w.component_id("Thing").unwrap();
    let mut v = w.query().has(c).entities();
    v.sort();
    v
}

#[test]
fn an_entity_switched_off_takes_its_children_out_of_queries() {
    let (mut w, [room, table, cup, hall]) = world();
    assert_eq!(things(&w).len(), 4);
    w.set_active(room, false).unwrap();
    assert_eq!(
        things(&w),
        vec![hall],
        "the room, its table, and the cup on it are out"
    );
    for e in [room, table, cup] {
        assert!(!w.is_active(e), "{e}");
        assert!(w.is_alive(e), "but still in the world");
    }
    assert!(w.is_switched_off(room) && !w.is_switched_off(table));
    assert!(w.has_type::<InactiveUnder>(cup));
    let c = w.component_id("Thing").unwrap();
    assert_eq!(w.query().including_inactive().has(c).count(), 4);
    let mut off = w.query().only_inactive().has(c).entities();
    off.sort();
    assert_eq!(off, vec![room, table, cup]);

    // A child switched off itself stays off when its parent comes back.
    w.set_active(table, false).unwrap();
    w.set_active(room, true).unwrap();
    assert!(w.is_active(room));
    assert!(!w.is_active(table) && !w.is_active(cup));
    assert!(
        !w.has_type::<InactiveUnder>(table),
        "it is off itself, not under one"
    );
    w.set_active(table, true).unwrap();
    assert_eq!(things(&w).len(), 4);
    assert!(!w.has_type::<Inactive>(table) && !w.has_type::<InactiveUnder>(cup));
}

#[test]
fn moving_under_an_inactive_parent_or_out_from_under_it_is_settled() {
    let (mut w, [room, _, cup, hall]) = world();
    w.set_active(room, false).unwrap();
    // The hall moves under the room: off with it once settled (the
    // hierarchy stage does it every tick).
    w.insert(hall, Parent { entity: room }).unwrap();
    w.settle_inactive().unwrap();
    assert!(!w.is_active(hall));
    // The cup leaves the room's tree: on again.
    w.remove_type::<Parent>(cup).unwrap();
    w.settle_inactive().unwrap();
    assert!(w.is_active(cup));
    // A cycle does not hang it.
    let (mut w, [room, table, ..]) = world();
    w.insert(room, Parent { entity: table }).unwrap();
    w.set_active(table, false).unwrap();
    assert!(!w.is_active(room));
}

#[test]
fn an_inactive_entity_is_near_nothing_but_saved_and_digested() {
    let (mut w, [room, table, cup, hall]) = world();
    w.rebuild_broadphase();
    assert_eq!(w.within(1.0, 0.0, 5.0).len(), 3);
    let before = w.digest();
    w.set_active(room, false).unwrap();
    w.rebuild_broadphase();
    assert!(
        w.within(1.0, 0.0, 5.0).is_empty(),
        "the switched-off room is near nothing"
    );
    assert_ne!(w.digest(), before, "being off is part of the world");

    let bytes = w.encode_state();
    let mut b = World::new();
    hierarchy::register(&mut b).unwrap();
    b.register_schema(SchemaBuilder::new("Thing").build().unwrap())
        .unwrap();
    b.decode_state(&bytes).unwrap();
    assert_eq!(b.digest(), w.digest());
    assert_eq!(things(&b), vec![hall]);
    for e in [room, table, cup] {
        assert!(!b.is_active(e));
    }
    let snap = w.clone();
    w.set_active(room, true).unwrap();
    assert_eq!(things(&snap), vec![hall], "a snapshot keeps what was off");
    assert_eq!(things(&w).len(), 4);
}
