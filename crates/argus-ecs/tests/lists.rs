//! List fields (issue 22): a list is one value, held in a row as a string
//! handle. It reads back exactly, element by element and type by type; it
//! is in the digest; it survives a save into a world whose string table
//! numbers things differently, a migration to a changed schema, and a
//! snapshot.

use argus_ecs::{FieldType, ListOf, SchemaBuilder, Value, Vec2, World};

fn list(of: ListOf) -> FieldType {
    FieldType::List(of)
}

fn register(w: &mut World, extra: bool) {
    let mut b = SchemaBuilder::new("Tile")
        .field("on", list(ListOf::Str))
        .field("pool", list(ListOf::Ref))
        .field("floors", list(ListOf::U8))
        .field("heat", list(ListOf::F32))
        .field("exact", list(ListOf::F64))
        .field("path", list(ListOf::Vec2))
        .field("lit", list(ListOf::Bool))
        .field("depth", list(ListOf::I32));
    if extra {
        b = b.field("added", FieldType::I32);
    }
    w.register_schema(b.build().unwrap()).unwrap();
}

fn strs(v: &[&str]) -> Value {
    Value::List(v.iter().map(|s| Value::Str(s.to_string())).collect())
}

/// One entity with every kind of list set, and strings that look like the
/// encoding's own separators.
fn world() -> (World, argus_ecs::Entity) {
    let mut w = World::new();
    register(&mut w, false);
    let e = w.spawn_authored("a/tile").unwrap();
    w.insert_by_name(e, "Tile").unwrap();
    w.set_field(
        e,
        "Tile.on",
        strs(&["oiled", "burning", "a:b;c", "", "é 3:x"]),
    )
    .unwrap();
    w.set_field(
        e,
        "Tile.pool",
        Value::List(vec![
            Value::Str("monster:rat".into()),
            Value::Ref("monster:bat".into()),
        ]),
    )
    .unwrap();
    w.set_field(
        e,
        "Tile.floors",
        Value::List(vec![Value::Int(1), Value::UInt(255)]),
    )
    .unwrap();
    w.set_field(
        e,
        "Tile.heat",
        Value::List(vec![Value::Float(0.1), Value::Int(-2)]),
    )
    .unwrap();
    w.set_field(
        e,
        "Tile.exact",
        Value::List(vec![
            Value::Float(0.1 + 0.2),
            Value::Float(-0.0),
            Value::Float(1e-300),
        ]),
    )
    .unwrap();
    w.set_field(
        e,
        "Tile.path",
        Value::List(vec![
            Value::Vec2(Vec2::new(1.5, -2.25)),
            Value::Vec2(Vec2::ZERO),
        ]),
    )
    .unwrap();
    w.set_field(
        e,
        "Tile.lit",
        Value::List(vec![Value::Bool(true), Value::Bool(false)]),
    )
    .unwrap();
    w.set_field(
        e,
        "Tile.depth",
        Value::List(vec![Value::Int(-7), Value::Int(i32::MAX as i64)]),
    )
    .unwrap();
    w.end_frame();
    (w, e)
}

#[test]
fn a_list_reads_back_exactly() {
    let (w, e) = world();
    assert_eq!(
        w.get_field(e, "Tile.on").unwrap(),
        strs(&["oiled", "burning", "a:b;c", "", "é 3:x"]),
        "order, repeats, and any text kept"
    );
    assert_eq!(
        w.get_field(e, "Tile.pool").unwrap(),
        Value::List(vec![
            Value::Ref("monster:rat".into()),
            Value::Ref("monster:bat".into())
        ])
    );
    assert_eq!(
        w.get_field(e, "Tile.floors").unwrap(),
        Value::List(vec![Value::UInt(1), Value::UInt(255)])
    );
    assert_eq!(
        w.get_field(e, "Tile.heat").unwrap(),
        Value::List(vec![Value::Float(0.1f32 as f64), Value::Float(-2.0)]),
        "an f32 list holds f32s"
    );
    let Value::List(exact) = w.get_field(e, "Tile.exact").unwrap() else {
        panic!()
    };
    let bits: Vec<u64> = exact
        .iter()
        .map(|v| match v {
            Value::Float(f) => f.to_bits(),
            _ => panic!(),
        })
        .collect();
    assert_eq!(
        bits,
        vec![
            (0.1f64 + 0.2).to_bits(),
            (-0.0f64).to_bits(),
            1e-300f64.to_bits()
        ],
        "floats to the bit"
    );
    assert_eq!(
        w.get_field(e, "Tile.path").unwrap(),
        Value::List(vec![
            Value::Vec2(Vec2::new(1.5, -2.25)),
            Value::Vec2(Vec2::ZERO)
        ])
    );
    assert_eq!(
        w.get_field(e, "Tile.lit").unwrap(),
        Value::List(vec![Value::Bool(true), Value::Bool(false)])
    );
    assert_eq!(
        w.get_field(e, "Tile.depth").unwrap(),
        Value::List(vec![Value::Int(-7), Value::Int(i32::MAX as i64)])
    );
    // A new row's list is empty.
    let mut w = w;
    let f = w.spawn();
    w.insert_by_name(f, "Tile").unwrap();
    assert_eq!(w.get_field(f, "Tile.on").unwrap(), Value::List(vec![]));
}

#[test]
fn a_wrong_element_or_a_long_list_is_refused() {
    let (w, e) = world();
    let err = w
        .set_field(
            e,
            "Tile.floors",
            Value::List(vec![Value::Int(3), Value::Int(300)]),
        )
        .unwrap_err();
    assert!(
        err.message.contains("element 2") && err.message.contains("300"),
        "{}",
        err.message
    );
    let err = w
        .set_field(e, "Tile.on", Value::List(vec![Value::Bool(true)]))
        .unwrap_err();
    assert!(err.message.contains("element 1"), "{}", err.message);
    let err = w
        .set_field(e, "Tile.on", Value::Str("oiled".into()))
        .unwrap_err();
    assert!(err.message.contains("list of str"), "{}", err.message);
    let long = Value::List(vec![Value::Str("x".into()); argus_ecs::value::MAX_LIST + 1]);
    let err = w.set_field(e, "Tile.on", long).unwrap_err();
    assert!(err.message.contains("at most 1024"), "{}", err.message);
    assert_eq!(
        FieldType::parse("list of ref"),
        Some(FieldType::List(ListOf::Ref))
    );
    assert_eq!(FieldType::parse("list of entity"), None, "no entity lists");
    assert_eq!(FieldType::List(ListOf::Vec2).name(), "list of vec2");
}

#[test]
fn a_list_is_in_the_digest_and_a_snapshot() {
    let (w, e) = world();
    let before = w.digest();
    let snap = w.clone();
    w.set_field(e, "Tile.on", strs(&["burning", "oiled"]))
        .unwrap();
    assert_ne!(w.digest(), before, "the order is part of the world");
    w.set_field(
        e,
        "Tile.on",
        strs(&["oiled", "burning", "a:b;c", "", "é 3:x"]),
    )
    .unwrap();
    assert_eq!(w.digest(), before);
    assert_eq!(snap.digest(), before);
    assert_eq!(
        snap.get_field(e, "Tile.on").unwrap(),
        w.get_field(e, "Tile.on").unwrap()
    );
}

#[test]
fn a_saved_list_comes_back_in_another_string_table_and_a_changed_schema() {
    let (a, e) = world();
    let bytes = a.encode_state();
    // The same components, another string table.
    let mut b = World::new();
    for s in ["zebra", "3:yak", "5:oiled"] {
        b.strings().intern(s);
    }
    register(&mut b, false);
    b.decode_state(&bytes).unwrap();
    assert_eq!(b.digest(), a.digest());
    assert_eq!(
        b.get_field(e, "Tile.on").unwrap(),
        a.get_field(e, "Tile.on").unwrap()
    );
    // A component that changed: its rows are copied field by field.
    let mut c = World::new();
    c.strings().intern("zebra");
    register(&mut c, true);
    c.decode_state(&bytes).unwrap();
    for f in [
        "on", "pool", "floors", "heat", "exact", "path", "lit", "depth",
    ] {
        let p = format!("Tile.{f}");
        assert_eq!(
            c.get_field(e, &p).unwrap(),
            a.get_field(e, &p).unwrap(),
            "{p}"
        );
    }
}
