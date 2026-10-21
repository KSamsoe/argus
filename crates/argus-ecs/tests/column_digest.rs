//! The column digest (docs/multiplayer-checklist.md item 0): the cheap
//! check two players' worlds still agree. It changes when any field does,
//! and only then: not with the order storage holds rows in, how the string
//! table numbers a string, a NaN's bits, or a round trip through bytes.

use argus_ecs::{Entity, FieldType, SchemaBuilder, Value, Vec2, World};

fn schemas(w: &mut World) {
    let all = [
        SchemaBuilder::new("Pos").field("p", FieldType::Vec2),
        SchemaBuilder::new("Vel").field("v", FieldType::Vec2),
        SchemaBuilder::new("Stats")
            .field("alive", FieldType::Bool)
            .field("lvl", FieldType::U8)
            .field("hp", FieldType::I32)
            .field("xp", FieldType::U64)
            .field("calm", FieldType::F32)
            .field("age", FieldType::F64)
            .field("name", FieldType::Str)
            .field("kind", FieldType::Ref)
            .field("target", FieldType::Entity),
        SchemaBuilder::new("Tag"),
    ];
    for s in all {
        w.register_schema(s.build().unwrap()).unwrap();
    }
}

fn add(w: &mut World, e: Entity, name: &str) {
    let c = w.component_id(name).unwrap();
    w.insert_default(e, c).unwrap();
}

/// Three entities; `scrambled` builds the same world by another history:
/// strings interned in another order, components added in another order
/// (so other archetypes, made in another order), and rows moved by a
/// despawn of a fourth.
fn world(scrambled: bool) -> World {
    let mut w = World::new();
    schemas(&mut w);
    if scrambled {
        for s in ["zebra", "goblin", "wolf"] {
            w.strings().intern(s);
        }
    }
    let es: Vec<Entity> = (0..4).map(|_| w.spawn()).collect();
    let order: &[(usize, &str)] = if scrambled {
        &[
            (3, "Pos"),
            (2, "Tag"),
            (1, "Vel"),
            (0, "Stats"),
            (2, "Pos"),
            (1, "Pos"),
            (0, "Pos"),
            (0, "Vel"),
        ]
    } else {
        &[
            (0, "Pos"),
            (0, "Vel"),
            (0, "Stats"),
            (1, "Pos"),
            (1, "Vel"),
            (2, "Pos"),
            (2, "Tag"),
            (3, "Pos"),
        ]
    };
    for (i, c) in order {
        add(&mut w, es[*i], c);
    }
    w.despawn(es[3]).unwrap();
    for (i, e) in es[..3].iter().enumerate() {
        w.set_field(*e, "Pos.p", Vec2::new(i as f32 * 1.5, -2.0))
            .unwrap();
    }
    w.set_field(es[0], "Vel.v", Vec2::new(0.25, 9.0)).unwrap();
    w.set_field(es[1], "Vel.v", Vec2::new(-1.0, 0.0)).unwrap();
    let e = es[0];
    w.set_field(e, "Stats.alive", true).unwrap();
    w.set_field(e, "Stats.lvl", Value::UInt(3)).unwrap();
    w.set_field(e, "Stats.hp", -7).unwrap();
    w.set_field(e, "Stats.xp", Value::UInt(1 << 40)).unwrap();
    w.set_field(e, "Stats.calm", 0.1).unwrap();
    w.set_field(e, "Stats.age", 1e10).unwrap();
    w.set_field(e, "Stats.name", "goblin").unwrap();
    w.set_field(e, "Stats.kind", Value::Ref("unit:wolf".into()))
        .unwrap();
    w.set_field(e, "Stats.target", Value::Entity(es[2]))
        .unwrap();
    w.drain_events();
    w
}

#[test]
fn the_same_world_by_another_history_has_the_same_digest() {
    let (a, b) = (world(false), world(true));
    // The worlds' storage differs: other archetypes, rows, string ids.
    assert_ne!(a.strings().lookup("goblin"), b.strings().lookup("goblin"));
    assert_eq!(a.digest(), b.digest(), "the text digest agrees");
    assert_eq!(a.column_digest(), b.column_digest());
}

#[test]
fn any_field_changes_it() {
    let base = world(false);
    let d = base.column_digest();
    let e = base.live().min().unwrap();
    let changes: Vec<(&str, Value)> = vec![
        ("Pos.p", Value::Vec2(Vec2::new(0.0, -2.000_000_2))),
        ("Vel.v", Value::Vec2(Vec2::new(0.25, -9.0))),
        ("Stats.alive", Value::Bool(false)),
        ("Stats.lvl", Value::UInt(4)),
        ("Stats.hp", Value::Int(-8)),
        ("Stats.xp", Value::UInt((1 << 40) + 1)),
        ("Stats.calm", Value::Float(0.100_000_01)),
        ("Stats.age", Value::Float(1e10 + 1e-6)),
        ("Stats.name", Value::Str("goblins".into())),
        ("Stats.kind", Value::Ref("unit:wolf2".into())),
        ("Stats.target", Value::Entity(Entity::NULL)),
    ];
    for (path, v) in changes {
        let w = base.fork();
        let was = w.get_field(e, path).unwrap();
        w.set_field(e, path, v.clone()).unwrap();
        assert_ne!(w.column_digest(), d, "{path} = {v:?}");
        w.set_field(e, path, was).unwrap();
        assert_eq!(w.column_digest(), d, "{path} back");
    }
    // Zero and negative zero are different bits, so different worlds.
    let w = base.fork();
    w.set_field(e, "Stats.calm", 0.0).unwrap();
    let plus = w.column_digest();
    w.set_field(e, "Stats.calm", -0.0).unwrap();
    assert_ne!(w.column_digest(), plus);
    // An entity, a component, or a tag more or less.
    let mut w = base.fork();
    w.spawn();
    assert_ne!(w.column_digest(), d);
    let mut w = base.fork();
    let tag = w.component_id("Tag").unwrap();
    w.insert_default(e, tag).unwrap();
    assert_ne!(w.column_digest(), d);
    let mut w = base.fork();
    let tagged = w.query().has(tag).entities()[0];
    w.remove(tagged, tag).unwrap();
    assert_ne!(w.column_digest(), d);
}

#[test]
fn a_nan_is_one_nan_whatever_its_bits() {
    let a = world(false);
    let b = a.fork();
    let e = a.live().min().unwrap();
    // x86's and ARM's NaNs for 0/0 differ in their sign bit.
    a.set_field(e, "Stats.calm", f32::from_bits(0xffc0_0000) as f64)
        .unwrap();
    b.set_field(e, "Stats.calm", f32::from_bits(0x7fc0_0001) as f64)
        .unwrap();
    assert_eq!(a.column_digest(), b.column_digest());
}

#[test]
fn a_round_trip_through_bytes_keeps_it() {
    let a = world(false);
    let mut b = World::new();
    schemas(&mut b);
    // A table that numbers strings its own way before the load.
    b.strings().intern("unrelated");
    b.decode_state(&a.encode_state()).unwrap();
    assert_eq!(b.column_digest(), a.column_digest());
    assert_eq!(b.digest(), a.digest());
    let mut c = World::new();
    c.restore(&a.snapshot());
    assert_eq!(c.column_digest(), a.column_digest());
}

/// Swarm's size, about 900 enemies with five components each, read every
/// second of a match: the checklist wants well under a millisecond.
#[test]
#[ignore = "release timing"]
fn it_costs_well_under_a_millisecond_for_a_swarm() {
    let mut w = World::new();
    schemas(&mut w);
    w.register_schema(
        SchemaBuilder::new("Enemy")
            .field("speed", FieldType::F32)
            .field("damage", FieldType::I32)
            .field("wave", FieldType::U32)
            .build()
            .unwrap(),
    )
    .unwrap();
    for i in 0..900 {
        let e = w.spawn();
        for c in ["Pos", "Vel", "Stats", "Enemy", "Tag"] {
            add(&mut w, e, c);
        }
        w.set_field(e, "Pos.p", Vec2::new(i as f32, 0.5 * i as f32))
            .unwrap();
        w.set_field(e, "Stats.name", if i % 2 == 0 { "a" } else { "b" })
            .unwrap();
    }
    let runs = 200;
    let t = std::time::Instant::now();
    let mut d = 0;
    for _ in 0..runs {
        d = std::hint::black_box(w.column_digest());
    }
    let each = t.elapsed() / runs;
    let t = std::time::Instant::now();
    std::hint::black_box(w.digest());
    let text = t.elapsed();
    eprintln!("column digest {each:?} a world (text digest {text:?}); {d:x}");
    assert!(each.as_micros() < 250, "{each:?}");
}
