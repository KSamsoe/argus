//! §3 "Entities": 64-bit index+generation ids, authored string ids that are
//! unique and stable, runtime entities that never get one.

use argus_ecs::{codes, Entity, Event, World};

#[test]
fn entity_id_is_index_plus_generation() {
    let e = Entity::from_parts(4011, 2);
    assert_eq!(e.index(), 4011);
    assert_eq!(e.generation(), 2);
    assert_eq!(Entity::from_bits(e.to_bits()), e);
    assert_eq!(e.to_string(), "e:4011.2");
    assert_eq!(Entity::from_parts(7, 0).to_string(), "e:7");
    assert_eq!("e:4011.2".parse::<Entity>().unwrap(), e);
    assert_eq!("e:7".parse::<Entity>().unwrap(), Entity::from_parts(7, 0));
    assert_eq!("e:null".parse::<Entity>().unwrap(), Entity::NULL);
    assert!(Entity::NULL.is_null());
    assert_eq!(Entity::default(), Entity::NULL);
    assert!("player".parse::<Entity>().is_err());
}

#[test]
fn stale_handles_die_and_slots_are_recycled() {
    let mut w = World::new();
    let a = w.spawn();
    assert!(w.is_alive(a));
    w.despawn(a).unwrap();
    assert!(!w.is_alive(a));
    let b = w.spawn();
    assert_eq!(b.index(), a.index(), "slot is reused");
    assert_ne!(b.generation(), a.generation(), "generation bumped");
    assert!(!w.is_alive(a), "old handle stays dead");
    assert!(w.is_alive(b));
    let err = w.despawn(a).unwrap_err();
    assert_eq!(err.code, codes::ENTITY_NOT_ALIVE);
    assert_eq!(w.entity_count(), 1);
}

#[test]
fn authored_ids_are_unique_and_resolvable() {
    let mut w = World::new();
    let player = w.spawn_authored("player").unwrap();
    let door = w.spawn_authored("@door.east").unwrap();
    assert_eq!(w.string_id(player), Some("player"));
    assert_eq!(w.string_id(door), Some("door.east"));
    assert!(w.is_authored(player));
    assert_eq!(w.entity("@player").unwrap(), player);
    assert_eq!(w.entity("player").unwrap(), player);
    assert_eq!(w.entity(&player.to_string()).unwrap(), player);
    assert_eq!(w.authored_count(), 2);

    let dup = w.spawn_authored("player").unwrap_err();
    assert_eq!(dup.code, codes::DUPLICATE_ID);
    assert!(dup.message.contains("player"));
    assert!(!dup.hint.is_empty());

    assert_eq!(w.spawn_authored("").unwrap_err().code, codes::BAD_ID);
    assert_eq!(
        w.spawn_authored("has space").unwrap_err().code,
        codes::BAD_ID
    );
    assert_eq!(w.spawn_authored("e:12").unwrap_err().code, codes::BAD_ID);
    assert_eq!(w.entity("@ghost").unwrap_err().code, codes::UNKNOWN_ID);
    assert_eq!(w.entity("e:999").unwrap_err().code, codes::ENTITY_NOT_ALIVE);
}

#[test]
fn runtime_entities_never_get_string_ids() {
    let mut w = World::new();
    let bullet = w.spawn();
    assert_eq!(w.string_id(bullet), None);
    assert!(!w.is_authored(bullet));
    assert_eq!(w.authored_count(), 0);
    assert_eq!(w.entity_count(), 1);
}

#[test]
fn despawn_frees_the_string_id_for_reuse() {
    let mut w = World::new();
    let a = w.spawn_authored("spawner.north").unwrap();
    w.despawn(a).unwrap();
    assert_eq!(w.string_id(a), None);
    assert_eq!(
        w.entity("@spawner.north").unwrap_err().code,
        codes::UNKNOWN_ID
    );
    let b = w.spawn_authored("spawner.north").unwrap();
    assert_ne!(a, b);
    assert_eq!(w.entity("@spawner.north").unwrap(), b);
}

#[test]
fn spawn_and_despawn_emit_events_with_ids() {
    let mut w = World::new();
    let p = w.spawn_authored("player").unwrap();
    let r = w.spawn();
    w.despawn(p).unwrap();
    let events: Vec<Event> = w.drain_events().into_iter().map(|r| r.event).collect();
    assert_eq!(
        events,
        vec![
            Event::Spawned {
                entity: p,
                id: Some("player".into())
            },
            Event::Spawned {
                entity: r,
                id: None
            },
            Event::Despawned {
                entity: p,
                id: Some("player".into())
            },
        ]
    );
    assert!(w.events().is_empty(), "drained");
}

#[test]
fn named_entities_are_sorted_for_summary() {
    let mut w = World::new();
    w.spawn_authored("zed").unwrap();
    w.spawn_authored("alpha").unwrap();
    w.spawn();
    let names: Vec<&str> = w.named_entities().into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, vec!["alpha", "zed"]);
    let s = w.summary();
    assert_eq!(s.entities, 3);
    assert_eq!(s.authored, 2);
    assert_eq!(s.named.len(), 2);
}
