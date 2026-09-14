//! Data tables and bulk chunks (§4 "Data tables", "Bulk authored data").

mod common;

use argus_ecs::{codes as ecs, Value};
use argus_scene::{codes, Bulk, Defs, Project, Rect};
use common::*;

#[test]
fn def_rows_are_keyed_typed_and_queryable() {
    let t = temp("defs");
    project(t.path());
    let p = open(t.path());
    let items = p.defs.table("items").unwrap();
    assert_eq!(items.prefix, "item");
    assert_eq!(items.columns(), ["id", "name", "weight"]);
    assert_eq!(items.rows.len(), 2);
    let brass = items.get("key.brass").unwrap();
    assert_eq!(
        items.value(brass, "id"),
        Some(&Value::Str("key.brass".into()))
    );
    assert_eq!(
        items.value(brass, "name"),
        Some(&Value::Str("Brass key".into()))
    );
    assert_eq!(items.value(brass, "weight"), Some(&Value::Float(0.1)));
    assert_eq!(brass.at, "defs/items.toml:1");
    let bread = items.get("bread").unwrap();
    assert_eq!(
        items.value(bread, "weight"),
        Some(&Value::Float(1.0)),
        "schema default"
    );

    // world.defs {table, where}
    let rows = items
        .query(&[("name".to_string(), Value::Str("Bread".into()))])
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].key, "bread");
    let rows = items
        .query(&[("weight".to_string(), Value::Int(1))])
        .unwrap();
    assert_eq!(rows.len(), 1, "where values coerce to the column type");
    let e = items
        .query(&[("mass".to_string(), Value::Int(1))])
        .unwrap_err();
    assert_eq!(e.code, ecs::UNKNOWN_FIELD);

    let (rt, row) = p.defs.resolve("recipe:bake").unwrap();
    assert_eq!(
        rt.value(row, "output"),
        Some(&Value::Ref("item:bread".into()))
    );
    assert_eq!(rt.value(row, "count"), Some(&Value::UInt(2)));
    assert!(p.defs.is_def_ref("item:x"));
    assert!(!p.defs.is_def_ref("assets/x.png"));
    assert!(p.defs.resolve("item:nope").is_none());
}

#[test]
fn def_problems_are_located_and_the_rest_loads() {
    let t = temp("defs-bad");
    project(t.path());
    write(t.path(), "defs/unknown.toml", "[\"a\"]\nx = 1\n");
    write(
        t.path(),
        "defs/items.toml",
        "[\"a\"]\nname = \"A\"\nmass = 1\n\n[\"b\"]\nweight = 500\n\n[\"c\"]\nid = \"d\"\n\n[\"ok\"]\nname = \"Ok\"\n",
    );
    write(
        t.path(),
        "defs/recipes.toml",
        "[\"bake\"]\noutput = \"item:ghost\"\n",
    );
    let p = Project::open(t.path(), def_schemas()).unwrap();
    let issues = p.issues();
    let by = |code: u32| {
        issues
            .iter()
            .filter(|e| e.code == code)
            .cloned()
            .collect::<Vec<_>>()
    };
    let no_schema = by(codes::BAD_DEF);
    assert!(
        no_schema
            .iter()
            .any(|e| e.message == "defs/unknown.toml has no schema"
                && e.hint.contains("def \"unknown\"")),
        "{no_schema:?}"
    );
    assert!(
        no_schema
            .iter()
            .any(|e| e.message.contains("disagrees with the row header")
                && e.r#ref.as_deref() == Some("defs/items.toml:8")),
        "{no_schema:?}"
    );
    let uf = by(ecs::UNKNOWN_FIELD);
    assert_eq!(uf[0].r#ref.as_deref(), Some("defs/items.toml:3"));
    let range = by(ecs::OUT_OF_RANGE);
    assert!(
        range[0]
            .message
            .contains("items.weight must be in range [0, 100]; got 500"),
        "{}",
        range[0]
    );
    assert!(
        range[0].message.contains("(in defs/items.toml:6)"),
        "{}",
        range[0]
    );
    let missing = by(codes::MISSING_REFERENCE);
    assert!(
        missing[0].message.contains("\"item:ghost\""),
        "{}",
        missing[0]
    );
    assert_eq!(missing[0].r#ref.as_deref(), Some("defs/recipes.toml:1"));
    // The good row is there.
    assert_eq!(p.defs.table("items").unwrap().rows.len(), 1);
    assert_eq!(p.defs.table("items").unwrap().rows[0].key, "ok");
    assert_eq!(issues.len(), 5, "{issues:?}");

    let empty = Defs::empty();
    assert!(empty.tables().next().is_none());
}

#[test]
fn bulk_chunks_round_trip_and_check_their_hash() {
    let t = temp("bulk");
    project(t.path());
    let rel = "scenes/hollow/rooms/cavern-04/tiles.map.toml";
    let mut b = Bulk::create(t.path(), rel, "tiles", 8, 4, &["ground", "props"], 2).unwrap();
    assert_eq!(b.index.chunk, "tiles.map.bin");
    assert!(!b.is_dirty());
    b.set("ground", Rect::new(1, 1, 2, 2), &[7, 8, 9, 10])
        .unwrap();
    b.set("props", Rect::new(7, 3, 1, 1), &[65535]).unwrap();
    assert!(b.is_dirty());
    b.save().unwrap();

    // A hand-written comment in the index survives a save.
    let text = read(t.path(), rel);
    write(t.path(), rel, &format!("# the cavern floor\n{text}"));
    let mut b = Bulk::open(t.path(), rel).unwrap();
    assert_eq!(
        b.get("ground", Rect::new(0, 0, 4, 3)).unwrap(),
        [0, 0, 0, 0, 0, 7, 8, 0, 0, 9, 10, 0]
    );
    assert_eq!(b.get("props", Rect::new(7, 3, 1, 1)).unwrap(), [65535]);
    b.set("ground", Rect::new(0, 0, 1, 1), &[1]).unwrap();
    b.save().unwrap();
    assert!(read(t.path(), rel).starts_with("# the cavern floor"));

    let e = b.get("ground", Rect::new(7, 0, 2, 1)).unwrap_err();
    assert_eq!(e.code, ecs::OUT_OF_RANGE);
    let e = b.set("props", Rect::new(0, 0, 1, 1), &[70000]).unwrap_err();
    assert_eq!(e.code, ecs::OUT_OF_RANGE);
    let e = b.get("sky", Rect::new(0, 0, 1, 1)).unwrap_err();
    assert!(e.hint.contains("ground, props"));

    // Tampering with the chunk is caught on open.
    let bin = "scenes/hollow/rooms/cavern-04/tiles.map.bin";
    let mut bytes = std::fs::read(t.path().join(bin)).unwrap();
    bytes[0] ^= 1;
    std::fs::write(t.path().join(bin), &bytes).unwrap();
    let e = Bulk::open(t.path(), rel).unwrap_err();
    assert_eq!(e.code, codes::BULK_MISMATCH);
    assert!(e.message.contains("chunk hash is fnv1a64:"), "{e}");
    bytes.pop();
    std::fs::write(t.path().join(bin), &bytes).unwrap();
    let e = Bulk::open(t.path(), rel).unwrap_err();
    assert!(
        e.message.contains("is 127 bytes; the index implies 128"),
        "{e}"
    );

    // The room lists its bulk files.
    let p = open(t.path());
    let w = world();
    let s = argus_scene::Scene::open(&p, "hollow", w.registry()).unwrap();
    assert_eq!(s.bulk_files("cavern-04"), [rel]);
    assert!(s.issues().is_empty(), "{:?}", s.issues());
}
