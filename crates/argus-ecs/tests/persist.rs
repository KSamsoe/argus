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

/// Needs as a game's first version declares it, and its second: `food`
/// renamed `hunger` and made a float, `rest` added, `mood` dropped.
fn needs(w: &mut World, version: u32) {
    let b = if version == 1 {
        SchemaBuilder::new("Needs")
            .field("food", FieldType::U8)
            .field("mood", FieldType::Str)
    } else {
        SchemaBuilder::new("Needs")
            .version(2)
            .field("hunger", FieldType::F32)
            .field("rest", FieldType::F32)
            .field("who", FieldType::Str)
    };
    w.register_schema(b.build().unwrap()).unwrap();
}

/// A migration as a script host would run one: Needs v1 → v2.
struct NeedsV2 {
    calls: usize,
}

impl argus_ecs::Migrate for NeedsV2 {
    fn declares(&self, component: &str) -> bool {
        component == "Needs"
    }
    fn migrate(
        &mut self,
        live: &argus_ecs::ComponentSchema,
        from: u32,
        row: &mut argus_ecs::Row,
    ) -> argus_ecs::Result<()> {
        assert_eq!((live.name.as_str(), live.version, from), ("Needs", 2, 1));
        self.calls += 1;
        let food = row.iter().find(|(n, _)| n == "food").unwrap().1.clone();
        let Value::UInt(food) = food else {
            panic!("food is a u8: {food:?}")
        };
        let mood = row.iter().find(|(n, _)| n == "mood").unwrap().1.clone();
        *row = vec![
            ("hunger".into(), Value::Float(food as f64 / 100.0)),
            ("who".into(), mood),
            ("gone".into(), Value::Bool(true)),
        ];
        Ok(())
    }
}

/// §6 save games (phase 3 item 0): a world saved with Needs v1 read by
/// one with Needs v2, through its migration: every row brought up, the
/// fields it did not set at their defaults, and the whole of it reported
/// ("Health migrated v2→v3 for 1,204 entities").
#[test]
fn rows_saved_at_an_older_version_are_migrated_and_reported() {
    let mut a = World::new();
    needs(&mut a, 1);
    let mut colonists = Vec::new();
    for i in 0..5u64 {
        let c = a.spawn();
        a.insert_by_name(c, "Needs").unwrap();
        a.set_field(c, "Needs.food", Value::UInt(i * 20)).unwrap();
        a.set_field(c, "Needs.mood", format!("colonist {i}"))
            .unwrap();
        colonists.push(c);
    }
    let bytes = a.encode_state();

    let mut b = World::new();
    needs(&mut b, 2);
    let mut m = NeedsV2 { calls: 0 };
    let report = b.decode_state_with(&bytes, Some(&mut m)).unwrap();
    assert_eq!(m.calls, 5);
    assert_eq!(
        report.migrated,
        vec![argus_ecs::Migrated {
            component: "Needs".into(),
            from: 1,
            to: 2,
            rows: 5,
            declared: true,
            left: vec!["gone".into()],
        }]
    );
    assert!(report.remapped.is_empty(), "{report:?}");
    for (i, c) in colonists.iter().enumerate() {
        assert_eq!(
            b.get_field(*c, "Needs.hunger").unwrap(),
            Value::Float((i as f32 * 0.2) as f64)
        );
        assert_eq!(b.get_field(*c, "Needs.rest").unwrap(), Value::Float(0.0));
        assert_eq!(
            b.get_field(*c, "Needs.who").unwrap(),
            Value::Str(format!("colonist {i}"))
        );
    }

    // No migration declared: field by field, by name, and still reported,
    // with what nothing took.
    let mut c = World::new();
    needs(&mut c, 2);
    let report = c.decode_state(&bytes).unwrap();
    let m = &report.migrated[0];
    assert_eq!((m.from, m.to, m.rows, m.declared), (1, 2, 5, false));
    assert_eq!(m.left, vec!["food".to_string(), "mood".to_string()]);
    assert_eq!(
        c.get_field(colonists[3], "Needs.hunger").unwrap(),
        Value::Float(0.0)
    );

    // A world saved by a newer version says so.
    let mut newer = World::new();
    needs(&mut newer, 2);
    let c0 = newer.spawn();
    newer.insert_by_name(c0, "Needs").unwrap();
    let mut old = World::new();
    needs(&mut old, 1);
    let report = old.decode_state(&newer.encode_state()).unwrap();
    assert_eq!(report.newer, vec![("Needs".to_string(), 2, 1)]);
}

/// A migration that returns a value the live field cannot hold names the
/// component, the versions, and the entity.
#[test]
fn a_migration_that_returns_the_wrong_type_says_where() {
    struct Bad;
    impl argus_ecs::Migrate for Bad {
        fn declares(&self, _: &str) -> bool {
            true
        }
        fn migrate(
            &mut self,
            _: &argus_ecs::ComponentSchema,
            _: u32,
            row: &mut argus_ecs::Row,
        ) -> argus_ecs::Result<()> {
            *row = vec![("hunger".into(), Value::Str("lots".into()))];
            Ok(())
        }
    }
    let mut a = World::new();
    needs(&mut a, 1);
    let c = a.spawn();
    a.insert_by_name(c, "Needs").unwrap();
    let bytes = a.encode_state();
    let mut b = World::new();
    needs(&mut b, 2);
    let e = b.decode_state_with(&bytes, Some(&mut Bad)).unwrap_err();
    assert_eq!(e.code, codes::BAD_STATE);
    assert!(
        e.message
            .starts_with(&format!("Needs v1→v2 on {c}: hunger: expected a f32 value")),
        "{}",
        e.message
    );
}
