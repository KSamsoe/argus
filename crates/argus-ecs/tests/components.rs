//! §3 "Components": schema plus column, declared from Rust via derive or at
//! runtime via a schema builder, introspectable, with doc strings, ranges,
//! defaults and `tune`. §5 "Errors are documentation".

use argus_ecs::{
    codes, Component, ComponentKind, DefRef, Entity, Event, FieldType, SchemaBuilder, StrId, Value,
    Vec2, World,
};

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Freezes an entity's simulation for N frames after a hit. Read by sim.hitstop.",
    version = 2
)]
struct HitStop {
    #[argus(doc = "Remaining frozen frames", range(0, 30))]
    frames: u8,
    #[argus(doc = "Time scale while frozen. 0 = full stop", range(0, 1), tune)]
    scale: f32,
}

#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(name = "Health", doc = "Hit points")]
struct HealthComp {
    #[argus(doc = "Current", range(0, 1000))]
    cur: i32,
    #[argus(doc = "Maximum")]
    max: i32,
}

impl Default for HealthComp {
    fn default() -> Self {
        HealthComp { cur: 10, max: 10 }
    }
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
#[argus(doc = "A door")]
struct Door {
    #[argus(doc = "Entity ref by string id, room-qualified")]
    leads_to: Entity,
    #[argus(doc = "Def ref, not a GUID")]
    key: DefRef,
    #[argus(doc = "Display name")]
    label: StrId,
    #[argus(doc = "Locked?")]
    locked: bool,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
struct Enemy;

#[test]
fn derive_produces_an_introspectable_schema() {
    let mut w = World::new();
    w.register::<HitStop>().unwrap();
    let s = w.describe("HitStop").unwrap();
    assert_eq!(s.name, "HitStop");
    assert_eq!(s.version, 2);
    assert!(s.doc.starts_with("Freezes"));
    assert_eq!(s.kind(), ComponentKind::Data);
    assert_eq!(s.size, std::mem::size_of::<HitStop>());
    assert_eq!(s.align, std::mem::align_of::<HitStop>());
    assert!(
        s.source.as_deref().unwrap().contains("components.rs:"),
        "{:?}",
        s.source
    );

    let frames = s.field("frames").unwrap();
    assert_eq!(frames.ty, FieldType::U8);
    assert_eq!(frames.offset, 0);
    assert_eq!(frames.default, Value::UInt(0));
    assert_eq!(frames.range, Some((0.0, 30.0)));
    assert_eq!(frames.doc, "Remaining frozen frames");
    assert!(!frames.tune);

    let scale = s.field("scale").unwrap();
    assert_eq!(scale.ty, FieldType::F32);
    assert_eq!(scale.offset, 4);
    assert_eq!(scale.default, Value::Float(0.0));
    assert!(scale.tune, "tune = true marks a feel parameter");
}

#[test]
fn defaults_come_from_default_impl_and_name_can_be_overridden() {
    let mut w = World::new();
    w.register::<HealthComp>().unwrap();
    assert!(w.component_id("HealthComp").is_none());
    let s = w.describe("Health").unwrap();
    assert_eq!(s.field("cur").unwrap().default, Value::Int(10));
    let e = w.spawn();
    let id = w.insert_by_name(e, "Health").unwrap();
    assert!(w.has(e, id));
    assert_eq!(
        w.get::<HealthComp>(e).unwrap(),
        HealthComp { cur: 10, max: 10 }
    );
}

#[test]
fn tags_are_zero_size() {
    let mut w = World::new();
    let id = w.register::<Enemy>().unwrap();
    let s = w.describe("Enemy").unwrap();
    assert_eq!(s.kind(), ComponentKind::Tag);
    assert_eq!(s.size, 0);
    let e = w.spawn();
    w.insert(e, Enemy).unwrap();
    assert!(w.has(e, id));
    assert_eq!(w.query().with::<Enemy>().count(), 1);
    w.remove(e, id).unwrap();
    assert!(!w.has(e, id));
    assert!(
        w.archetype_stats().iter().map(|a| a.bytes).sum::<usize>() < 1024,
        "tag columns allocate nothing"
    );
}

#[test]
fn insert_get_update_remove_and_archetype_moves() {
    let mut w = World::new();
    let e = w.spawn();
    w.insert(
        e,
        HitStop {
            frames: 3,
            scale: 0.5,
        },
    )
    .unwrap();
    w.insert(e, HealthComp { cur: 4, max: 9 }).unwrap();
    assert_eq!(w.components_of(e).unwrap().len(), 2);
    assert_eq!(
        w.get::<HitStop>(e).unwrap(),
        HitStop {
            frames: 3,
            scale: 0.5
        }
    );
    w.update::<HitStop, _>(e, |h| h.frames -= 1).unwrap();
    assert_eq!(w.get::<HitStop>(e).unwrap().frames, 2);
    // Overwrite keeps the archetype.
    let arch_before = w.archetype_stats();
    w.insert(
        e,
        HitStop {
            frames: 9,
            scale: 1.0,
        },
    )
    .unwrap();
    assert_eq!(w.archetype_stats(), arch_before);
    assert_eq!(w.get::<HitStop>(e).unwrap().frames, 9);
    // Remove moves it; the other component survives the move.
    w.remove_type::<HitStop>(e).unwrap();
    assert!(!w.has_type::<HitStop>(e));
    assert_eq!(
        w.get::<HealthComp>(e).unwrap(),
        HealthComp { cur: 4, max: 9 }
    );
    assert_eq!(
        w.get::<HitStop>(e).unwrap_err().code,
        codes::MISSING_COMPONENT
    );
    // Removing an absent component is a no-op.
    w.remove_type::<HitStop>(e).unwrap();
    let events: Vec<_> = w.drain_events().into_iter().map(|r| r.event).collect();
    let hs = w.component_id("HitStop").unwrap();
    assert!(events.contains(&Event::ComponentAdded {
        entity: e,
        component: hs
    }));
    assert_eq!(
        events
            .iter()
            .filter(
                |ev| matches!(ev, Event::ComponentRemoved { component, .. } if *component == hs)
            )
            .count(),
        1
    );
}

#[test]
fn swap_remove_keeps_other_entities_addressable() {
    let mut w = World::new();
    let es: Vec<Entity> = (0..5)
        .map(|i| {
            let e = w.spawn();
            w.insert(e, HealthComp { cur: i, max: 100 }).unwrap();
            e
        })
        .collect();
    w.despawn(es[1]).unwrap();
    w.remove_type::<HealthComp>(es[0]).unwrap();
    for (i, &e) in es.iter().enumerate().skip(2) {
        assert_eq!(
            w.get::<HealthComp>(e).unwrap().cur,
            i as i32,
            "entity {i} still reads its own row"
        );
    }
}

#[test]
fn field_paths_read_and_write_with_coercion_and_range_checks() {
    let mut w = World::new();
    let e = w.spawn_authored("player").unwrap();
    w.insert(e, HitStop::default()).unwrap();
    w.set_field(e, "HitStop.frames", 12i64).unwrap();
    assert_eq!(w.get_field(e, "HitStop.frames").unwrap(), Value::UInt(12));
    w.set_field(e, "HitStop.scale", 1i64).unwrap();
    assert_eq!(w.get_field(e, "HitStop.scale").unwrap(), Value::Float(1.0));
    assert_eq!(
        w.get::<HitStop>(e).unwrap(),
        HitStop {
            frames: 12,
            scale: 1.0
        }
    );

    let err = w.set_field(e, "HitStop.scale", 1.5).unwrap_err();
    assert_eq!(err.code, codes::OUT_OF_RANGE);
    assert_eq!(
        err.message,
        "HitStop.scale must be in range [0, 1]; got 1.5"
    );
    assert!(
        err.hint.contains("1"),
        "hint names the legal value: {}",
        err.hint
    );
    assert!(
        err.r#ref.as_deref().unwrap().contains("components.rs:"),
        "ref points at the schema"
    );
    assert_eq!(
        w.get_field(e, "HitStop.scale").unwrap(),
        Value::Float(1.0),
        "rejected write leaves the value"
    );

    assert_eq!(
        w.set_field(e, "HitStop.frames", 300i64).unwrap_err().code,
        codes::OUT_OF_RANGE
    );
    assert_eq!(
        w.set_field(e, "HitStop.frames", "many").unwrap_err().code,
        codes::TYPE_MISMATCH
    );
    assert_eq!(
        w.get_field(e, "HitStop.nope").unwrap_err().code,
        codes::UNKNOWN_FIELD
    );
    assert_eq!(
        w.get_field(e, "Nope.x").unwrap_err().code,
        codes::UNKNOWN_COMPONENT
    );
    assert_eq!(w.get_field(e, "HitStop").unwrap_err().code, codes::BAD_PATH);
    assert_eq!(
        w.get_field(e, "Health.cur").unwrap_err().code,
        codes::UNKNOWN_COMPONENT
    );
    w.register::<HealthComp>().unwrap();
    assert_eq!(
        w.get_field(e, "Health.cur").unwrap_err().code,
        codes::MISSING_COMPONENT
    );
    let unknown = w.get_field(e, "HitStop.nope").unwrap_err();
    assert!(
        unknown.hint.contains("frames"),
        "hint lists the real fields"
    );
}

#[test]
fn string_ref_and_entity_fields_round_trip() {
    let mut w = World::new();
    let door = w.spawn_authored("door.east").unwrap();
    let target = w.spawn_authored("door.west").unwrap();
    w.insert(door, Door::default()).unwrap();
    w.set_field(door, "Door.leads_to", target).unwrap();
    w.set_field(door, "Door.key", Value::Ref("item:key.brass".into()))
        .unwrap();
    w.set_field(door, "Door.label", "East door").unwrap();
    w.set_field(door, "Door.locked", true).unwrap();
    assert_eq!(
        w.get_field(door, "Door.leads_to").unwrap(),
        Value::Entity(target)
    );
    assert_eq!(
        w.get_field(door, "Door.key").unwrap(),
        Value::Ref("item:key.brass".into())
    );
    assert_eq!(
        w.get_field(door, "Door.label").unwrap(),
        Value::Str("East door".into())
    );
    assert_eq!(w.get_field(door, "Door.locked").unwrap(), Value::Bool(true));
    // A str value is accepted for a ref field (the scene loader hands over TOML strings).
    w.set_field(door, "Door.key", "item:key.iron").unwrap();
    assert_eq!(
        w.get_field(door, "Door.key").unwrap(),
        Value::Ref("item:key.iron".into())
    );
    // Typed access sees the interned handle.
    let d = w.get::<Door>(door).unwrap();
    assert_eq!(w.strings().resolve(d.label), "East door");
    assert_eq!(w.strings().resolve(d.key.0), "item:key.iron");
    assert_eq!(d.leads_to, target);
    let all = w
        .get_component(door, w.component_id("Door").unwrap())
        .unwrap();
    assert_eq!(all.len(), 4);
    assert_eq!(all[3], ("locked".to_string(), Value::Bool(true)));
}

#[test]
fn runtime_schema_matches_rust_layout_and_is_indistinguishable() {
    // A Luau-declared schema with the same fields as HitStop.
    let schema = SchemaBuilder::new("HitStopLua")
        .doc("Script-declared twin of HitStop")
        .source("components/hitstop.luau:1")
        .field_full(
            "frames",
            FieldType::U8,
            Value::UInt(0),
            Some((0.0, 30.0)),
            "Remaining frozen frames",
            false,
        )
        .field_full(
            "scale",
            FieldType::F32,
            Value::Float(0.0),
            Some((0.0, 1.0)),
            "Time scale while frozen",
            true,
        )
        .build()
        .unwrap();
    assert_eq!(schema.size, std::mem::size_of::<HitStop>());
    assert_eq!(schema.align, std::mem::align_of::<HitStop>());
    assert_eq!(schema.field("scale").unwrap().offset, 4);

    let mut w = World::new();
    let id = w.register_schema(schema).unwrap();
    let e = w.spawn();
    w.insert_default(e, id).unwrap();
    w.set_field(e, "HitStopLua.frames", 7i64).unwrap();
    assert_eq!(w.get_field(e, "HitStopLua.frames").unwrap(), Value::UInt(7));
    let err = w.set_field(e, "HitStopLua.frames", 31i64).unwrap_err();
    assert_eq!(err.code, codes::OUT_OF_RANGE);
    assert_eq!(err.r#ref.as_deref(), Some("components/hitstop.luau:1"));
    let events: Vec<_> = w.drain_events().into_iter().map(|r| r.event).collect();
    assert!(events.contains(&Event::ComponentRegistered {
        component: id,
        name: "HitStopLua".into()
    }));
}

#[test]
fn runtime_schema_layout_follows_c_rules() {
    let s = SchemaBuilder::new("Mixed")
        .field("a", FieldType::Bool)
        .field("b", FieldType::F64)
        .field("c", FieldType::U16)
        .field("v", FieldType::Vec2)
        .field("e", FieldType::Entity)
        .build()
        .unwrap();
    let off: Vec<usize> = s.fields.iter().map(|f| f.offset).collect();
    assert_eq!(off, vec![0, 8, 16, 20, 32]);
    assert_eq!(s.size, 40);
    assert_eq!(s.align, 8);
    assert!(s.has_entity_refs());
}

#[test]
fn bad_schemas_are_rejected_with_reasons() {
    let dup = SchemaBuilder::new("X")
        .field("a", FieldType::U8)
        .field("a", FieldType::U8)
        .build()
        .unwrap_err();
    assert_eq!(dup.code, codes::BAD_SCHEMA);
    let bad_range = SchemaBuilder::new("X")
        .field_full(
            "s",
            FieldType::Str,
            Value::Str(String::new()),
            Some((0.0, 1.0)),
            "",
            false,
        )
        .build()
        .unwrap_err();
    assert_eq!(bad_range.code, codes::BAD_SCHEMA);
    let bad_default = SchemaBuilder::new("X")
        .field_full(
            "n",
            FieldType::U8,
            Value::UInt(99),
            Some((0.0, 1.0)),
            "",
            false,
        )
        .build()
        .unwrap_err();
    assert_eq!(bad_default.code, codes::BAD_SCHEMA);
    let bad_name = SchemaBuilder::new("a.b").build().unwrap_err();
    assert_eq!(bad_name.code, codes::BAD_SCHEMA);

    // Same name, different shape: conflict. Same name, same shape: same id.
    let mut w = World::new();
    let a = w
        .register_schema(
            SchemaBuilder::new("Dup")
                .field("x", FieldType::F32)
                .build()
                .unwrap(),
        )
        .unwrap();
    let again = w
        .register_schema(
            SchemaBuilder::new("Dup")
                .field("x", FieldType::F32)
                .build()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(a, again);
    let conflict = w.register_schema(
        SchemaBuilder::new("Dup")
            .field("x", FieldType::F64)
            .build()
            .unwrap(),
    );
    assert_eq!(conflict.unwrap_err().code, codes::BAD_SCHEMA);
    assert_eq!(w.registry().len(), 1);
}

#[test]
fn value_coercion_rules() {
    assert_eq!(
        Value::Int(3).coerce(FieldType::F32).unwrap(),
        Value::Float(3.0)
    );
    assert_eq!(Value::Int(3).coerce(FieldType::U8).unwrap(), Value::UInt(3));
    assert_eq!(Value::UInt(3).coerce(FieldType::I8).unwrap(), Value::Int(3));
    assert_eq!(
        Value::Int(-1).coerce(FieldType::U8).unwrap_err().code,
        codes::TYPE_MISMATCH
    );
    assert_eq!(
        Value::UInt(256).coerce(FieldType::U8).unwrap_err().code,
        codes::OUT_OF_RANGE
    );
    assert_eq!(
        Value::Int(-129).coerce(FieldType::I8).unwrap_err().code,
        codes::OUT_OF_RANGE
    );
    assert_eq!(
        Value::Float(1.0).coerce(FieldType::I32).unwrap_err().code,
        codes::TYPE_MISMATCH
    );
    assert_eq!(
        Value::Str("a".into()).coerce(FieldType::Ref).unwrap(),
        Value::Ref("a".into())
    );
    assert_eq!(
        Value::Vec2(Vec2::new(1.0, 2.0))
            .coerce(FieldType::Vec2)
            .unwrap(),
        Value::Vec2(Vec2::new(1.0, 2.0))
    );
    assert_eq!(FieldType::parse("vec2"), Some(FieldType::Vec2));
    assert_eq!(FieldType::parse("string"), Some(FieldType::Str));
    assert_eq!(FieldType::parse("mat3"), None);
}
