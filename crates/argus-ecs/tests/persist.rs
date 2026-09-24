//! A world written to bytes and read back (phase 2 item 2): exactly, into
//! a world whose string table numbers things differently, into one whose
//! components changed, and not at all from damaged bytes.

use argus_ecs::{codes, Entity, FieldType, SchemaBuilder, Transform, Value, World};

/// Components as a game's scripts declare them.
fn register(w: &mut World, with_max: bool, with_mood: bool) {
    w.register::<Transform>().unwrap();
    let mut h = SchemaBuilder::new("Health");
    if with_max {
        h = h.field("max", FieldType::I32);
    }
    let h = h
        .field("cur", FieldType::I32)
        .field("label", FieldType::Str);
    w.register_schema(h.build().unwrap()).unwrap();
    w.register_schema(
        SchemaBuilder::new("Door")
            .field("leads_to", FieldType::Entity)
            .field("key", FieldType::Ref)
            .build()
            .unwrap(),
    )
    .unwrap();
    w.register_schema(SchemaBuilder::new("Enemy").build().unwrap())
        .unwrap();
    if with_mood {
        w.register_schema(
            SchemaBuilder::new("Mood")
                .field("calm", FieldType::F32)
                .build()
                .unwrap(),
        )
        .unwrap();
    }
}

/// A small world with authored and runtime entities, a hole in the slots,
/// strings, references, and a tag.
fn world() -> (World, Entity, Entity) {
    let mut w = World::new();
    register(&mut w, false, true);
    let hero = w.spawn_authored("a/hero").unwrap();
    w.insert(hero, Transform::at(3.0, 1.8)).unwrap();
    w.insert_by_name(hero, "Health").unwrap();
    w.set_field(hero, "Health.cur", 7i64).unwrap();
    w.set_field(hero, "Health.label", "brave").unwrap();
    let door = w.spawn_authored("a/door").unwrap();
    w.insert_by_name(door, "Door").unwrap();
    w.set_field(door, "Door.leads_to", hero).unwrap();
    w.set_field(door, "Door.key", Value::Ref("item:key.brass".into()))
        .unwrap();
    for i in 0..6 {
        let bat = w.spawn();
        w.insert(bat, Transform::at(i as f32 * 1.5, 0.25)).unwrap();
        w.insert_by_name(bat, "Enemy").unwrap();
        w.insert_by_name(bat, "Health").unwrap();
        w.set_field(bat, "Health.label", format!("bat {i}"))
            .unwrap();
        if i % 2 == 0 {
            w.insert_by_name(bat, "Mood").unwrap();
            w.set_field(bat, "Mood.calm", 0.1 * i as f64).unwrap();
        }
    }
    // Two holes in the slots, freed in an order the next spawns must repeat.
    let gone: Vec<Entity> = w.live().filter(|e| !w.is_authored(*e)).take(3).collect();
    w.despawn(gone[2]).unwrap();
    w.despawn(gone[0]).unwrap();
    w.end_frame();
    (w, hero, door)
}

fn rows(w: &World) -> Vec<(Entity, Option<Value>)> {
    w.live()
        .map(|e| (e, w.get_field(e, "Health.label").ok()))
        .collect()
}

#[test]
fn a_world_reads_back_exactly() {
    let (mut a, hero, door) = world();
    let bytes = a.encode_state();

    let mut b = World::new();
    register(&mut b, false, true);
    let report = b.decode_state(&bytes).unwrap();
    assert_eq!(report.entities, a.entity_count());
    assert!(
        report.remapped.is_empty() && report.dropped.is_empty(),
        "{report:?}"
    );

    assert_eq!(a.digest(), b.digest());
    assert_eq!(
        rows(&a),
        rows(&b),
        "the same entities, iterated in the same order"
    );
    assert_eq!(b.entity("a/hero").unwrap(), hero);
    assert_eq!(b.entity("@hero").unwrap(), hero, "short ids come back too");
    assert_eq!(
        b.get_field(door, "Door.leads_to").unwrap(),
        Value::Entity(hero)
    );
    assert_eq!(b.frame(), a.frame());
    // The free list comes back in order: the next spawns reuse the same slots.
    let (x, y) = (a.spawn(), a.spawn());
    assert_eq!((b.spawn(), b.spawn()), (x, y));
    // And writing it again gives the same bytes.
    let (a2, _, _) = world();
    assert_eq!(a2.encode_state(), bytes);
}

#[test]
fn strings_are_translated_into_the_reading_worlds_table() {
    let (a, hero, door) = world();
    let bytes = a.encode_state();
    let mut b = World::new();
    // The reading world's own load interned other strings first.
    for s in ["zebra", "item:key.iron", "bat 3", "yak"] {
        b.strings().intern(s);
    }
    register(&mut b, false, true);
    b.decode_state(&bytes).unwrap();
    assert_eq!(a.digest(), b.digest());
    assert_eq!(
        b.get_field(hero, "Health.label").unwrap(),
        Value::Str("brave".into())
    );
    assert_eq!(
        b.get_field(door, "Door.key").unwrap(),
        Value::Ref("item:key.brass".into())
    );
}

#[test]
fn changed_components_are_copied_by_field_and_gone_ones_dropped() {
    let (a, hero, _) = world();
    let bytes = a.encode_state();
    // Health gained a field before its others; Mood is gone.
    let mut b = World::new();
    register(&mut b, true, false);
    let report = b.decode_state(&bytes).unwrap();
    assert_eq!(report.remapped, vec!["Health".to_string()]);
    assert_eq!(report.dropped, vec!["Mood".to_string()]);
    assert_eq!(report.entities, a.entity_count());
    assert_eq!(b.get_field(hero, "Health.cur").unwrap(), Value::Int(7));
    assert_eq!(b.get_field(hero, "Health.max").unwrap(), Value::Int(0));
    assert_eq!(
        b.get_field(hero, "Health.label").unwrap(),
        Value::Str("brave".into())
    );
    // Bats with and without Mood are one archetype now, in the old order.
    let bats: Vec<Value> = b
        .live()
        .filter(|e| b.has(*e, b.registry().id("Enemy").unwrap()))
        .map(|e| b.get_field(e, "Health.label").unwrap())
        .collect();
    let before: Vec<Value> = a
        .live()
        .filter(|e| a.has(*e, a.registry().id("Enemy").unwrap()))
        .map(|e| a.get_field(e, "Health.label").unwrap())
        .collect();
    assert_eq!(before, bats);
    assert_eq!(b.archetypes().iter().filter(|a| !a.is_empty()).count(), 3);
}

#[test]
fn damaged_bytes_are_an_error_and_leave_the_world_alone() {
    let (a, _, _) = world();
    let bytes = a.encode_state();
    let (mut b, _, _) = world();
    let before = b.digest();
    for cut in [0, 3, 8, 40, bytes.len() / 2, bytes.len() - 1] {
        let e = b.decode_state(&bytes[..cut]).unwrap_err();
        assert_eq!(e.code, codes::BAD_STATE, "cut at {cut}: {}", e.message);
        assert_eq!(b.digest(), before, "cut at {cut}");
    }
    let mut wrong = bytes.clone();
    wrong[0] = b'X';
    assert!(b
        .decode_state(&wrong)
        .unwrap_err()
        .message
        .contains("expected"));
    let mut extra = bytes;
    extra.push(0);
    assert!(b
        .decode_state(&extra)
        .unwrap_err()
        .message
        .contains("after the end"));
}
