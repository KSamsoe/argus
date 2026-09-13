//! §3 "Relations" (dangling refs nulled at frame end with an event), §2 and
//! §6 (snapshot, restore, fork), §8 L0 (summary).

use argus_ecs::{Component, Entity, Event, FieldType, SchemaBuilder, Value, World};

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
#[argus(doc = "Who owns this entity")]
struct OwnedBy {
    owner: Entity,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
struct Hp {
    cur: i32,
}

#[test]
fn dangling_references_are_nulled_at_frame_end_with_an_event() {
    let mut w = World::new();
    let owner = w.spawn_authored("player").unwrap();
    let a = w.spawn();
    let b = w.spawn();
    w.insert(a, OwnedBy { owner }).unwrap();
    w.insert(b, OwnedBy { owner }).unwrap();
    let keeper = w.spawn();
    w.insert(keeper, OwnedBy { owner: a }).unwrap();

    assert_eq!(w.maintain(), 0, "nothing dangling yet");
    w.despawn(owner).unwrap();
    assert_eq!(
        w.get::<OwnedBy>(a).unwrap().owner,
        owner,
        "stale until frame end"
    );
    w.drain_events();

    let frame = w.frame();
    assert_eq!(w.end_frame(), 2);
    assert_eq!(w.frame(), frame + 1);
    assert_eq!(w.get::<OwnedBy>(a).unwrap().owner, Entity::NULL);
    assert_eq!(w.get::<OwnedBy>(b).unwrap().owner, Entity::NULL);
    assert_eq!(
        w.get::<OwnedBy>(keeper).unwrap().owner,
        a,
        "live refs untouched"
    );

    let comp = w.component_id_of::<OwnedBy>().unwrap();
    let evs: Vec<_> = w.events().iter().collect();
    assert_eq!(evs.len(), 2);
    for r in &evs {
        assert_eq!(r.frame, frame, "tagged with the frame that ended");
        match &r.event {
            Event::DanglingRefNulled {
                component,
                field,
                target,
                ..
            } => {
                assert_eq!(*component, comp);
                assert_eq!(field, "owner");
                assert_eq!(*target, owner);
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    // Already-null refs are not reported again.
    assert_eq!(w.end_frame(), 0);
}

#[test]
fn dynamic_schema_entity_fields_are_maintained_too() {
    let mut w = World::new();
    let id = w
        .register_schema(
            SchemaBuilder::new("Target")
                .field("who", FieldType::Entity)
                .build()
                .unwrap(),
        )
        .unwrap();
    let victim = w.spawn();
    let hunter = w.spawn();
    w.insert_default(hunter, id).unwrap();
    w.set_field(hunter, "Target.who", victim).unwrap();
    w.despawn(victim).unwrap();
    assert_eq!(w.maintain(), 1);
    assert_eq!(
        w.get_field(hunter, "Target.who").unwrap(),
        Value::Entity(Entity::NULL)
    );
}

#[test]
fn snapshot_restore_round_trips_everything() {
    let mut w = World::new();
    let p = w.spawn_authored("player").unwrap();
    w.insert(p, Hp { cur: 9 }).unwrap();
    let id = w
        .register_schema(
            SchemaBuilder::new("Name")
                .field("s", FieldType::Str)
                .build()
                .unwrap(),
        )
        .unwrap();
    w.insert_default(p, id).unwrap();
    w.set_field(p, "Name.s", "Ada").unwrap();
    w.end_frame();
    let pending = w.events().len();
    let snap = w.snapshot();
    assert_eq!(snap.frame(), 1);
    assert_eq!(snap.entity_count(), 1);
    assert!(snap.bytes() > 0);

    // Mutate after the snapshot.
    w.update::<Hp, _>(p, |h| h.cur = 1).unwrap();
    w.set_field(p, "Name.s", "Bob").unwrap();
    let extra = w.spawn_authored("extra").unwrap();
    w.despawn(p).unwrap();
    w.end_frame();
    w.end_frame();

    w.restore(&snap);
    assert_eq!(w.frame(), 1);
    assert!(w.is_alive(p));
    assert!(!w.is_alive(extra));
    assert_eq!(w.entity("@player").unwrap(), p);
    assert_eq!(w.get::<Hp>(p).unwrap().cur, 9);
    assert_eq!(w.get_field(p, "Name.s").unwrap(), Value::Str("Ada".into()));
    assert_eq!(
        w.events().len(),
        pending,
        "event queue is part of the snapshot"
    );
    // Registry survives too.
    assert!(w.describe("Name").is_ok());
    // And a stale handle from the abandoned timeline stays dead.
    assert!(w.spawn_authored("extra").is_ok());
}

#[test]
fn forks_evolve_independently() {
    let mut w = World::new();
    let e = w.spawn_authored("e").unwrap();
    w.insert(e, Hp { cur: 5 }).unwrap();
    let mut fork = w.fork();
    fork.update::<Hp, _>(e, |h| h.cur = 100).unwrap();
    fork.spawn();
    assert_eq!(w.get::<Hp>(e).unwrap().cur, 5);
    assert_eq!(fork.get::<Hp>(e).unwrap().cur, 100);
    assert_eq!(w.entity_count(), 1);
    assert_eq!(fork.entity_count(), 2);
    // A snapshot can be forked many times.
    let snap = w.snapshot();
    let a = snap.fork();
    let b = snap.fork();
    assert_eq!(a.get::<Hp>(e).unwrap(), b.get::<Hp>(e).unwrap());
}

#[test]
fn summary_and_archetype_stats_orient_in_one_screen() {
    let mut w = World::new();
    let p = w.spawn_authored("player").unwrap();
    w.insert(p, Hp { cur: 1 }).unwrap();
    for _ in 0..3 {
        let b = w.spawn();
        w.insert(b, Hp { cur: 1 }).unwrap();
        w.insert(b, OwnedBy { owner: p }).unwrap();
    }
    let s = w.summary();
    assert_eq!(s.entities, 4);
    assert_eq!(s.authored, 1);
    assert_eq!(s.components, 2);
    assert_eq!(s.named, vec![("player".to_string(), p)]);
    assert_eq!(s.archetypes.len(), 2);
    assert_eq!(s.archetypes[0].entities, 3, "largest first");
    assert_eq!(
        s.archetypes[0].components,
        vec!["Hp".to_string(), "OwnedBy".to_string()]
    );
    assert_eq!(s.archetypes[1].components, vec!["Hp".to_string()]);
    assert!(s.pending_events > 0);
}
