//! §3 "systems are functions that run over queries" and "each system
//! declares the components it reads and writes"; §7 "a system may only touch
//! components it declared" is enforced, not recommended.

use argus_ecs::{codes, Access, Component, Entity, FieldType, SchemaBuilder, Value, Vec2, World};

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
struct Pos {
    v: Vec2,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
struct Vel {
    v: Vec2,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
struct Hp {
    cur: i32,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
struct Enemy;

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
struct Frozen;

fn world() -> (World, Vec<Entity>) {
    let mut w = World::new();
    let mut es = Vec::new();
    for i in 0..6 {
        let e = w.spawn();
        w.insert(
            e,
            Pos {
                v: Vec2::new(i as f32, 0.0),
            },
        )
        .unwrap();
        if i % 2 == 0 {
            w.insert(
                e,
                Vel {
                    v: Vec2::new(1.0, 0.0),
                },
            )
            .unwrap();
        }
        if i >= 3 {
            w.insert(e, Enemy).unwrap();
        }
        if i == 5 {
            w.insert(e, Frozen).unwrap();
        }
        es.push(e);
    }
    (w, es)
}

#[test]
fn filters_by_has_and_not() {
    let (w, es) = world();
    assert_eq!(w.query().with::<Pos>().count(), 6);
    assert_eq!(w.query().with::<Vel>().count(), 3);
    assert_eq!(w.query().with::<Enemy>().count(), 3);
    assert_eq!(w.query().with::<Enemy>().without::<Frozen>().count(), 2);
    assert_eq!(w.query().with::<Vel>().with::<Enemy>().count(), 1);
    let mut got = w.query().with::<Vel>().with::<Enemy>().entities();
    got.sort();
    assert_eq!(got, vec![es[4]]);
}

#[test]
fn typed_iteration_reads_and_writes_columns() {
    let (w, _) = world();
    w.query()
        .each::<(&Vel, &mut Pos)>(|_, (vel, pos)| {
            pos.v.x += vel.v.x;
        })
        .unwrap();
    let mut xs: Vec<f32> = Vec::new();
    w.query().each::<&Pos>(|_, p| xs.push(p.v.x)).unwrap();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(xs, vec![1.0, 1.0, 3.0, 3.0, 5.0, 5.0]);
}

#[test]
fn iteration_visits_each_entity_once_across_archetypes() {
    let (w, es) = world();
    let mut seen = Vec::new();
    w.query().each::<&Pos>(|e, _| seen.push(e)).unwrap();
    seen.sort();
    let mut expect = es.clone();
    expect.sort();
    assert_eq!(seen, expect);
}

#[test]
fn unregistered_component_matches_nothing() {
    let (w, _) = world();
    assert_eq!(w.query().with::<Hp>().count(), 0);
    assert!(w.query().each::<&Hp>(|_, _| {}).is_err());
}

#[test]
fn declared_access_is_enforced() {
    let (w, _) = world();
    let reg = w.registry();
    let pos = reg.id_of::<Pos>().unwrap();
    let vel = reg.id_of::<Vel>().unwrap();

    // Declared correctly: works.
    let ok = Access::new().read(vel).write(pos);
    w.query()
        .access(ok.clone())
        .each::<(&Vel, &mut Pos)>(|_, (v, p)| p.v.x += v.v.x)
        .unwrap();

    // Writing a component only declared as read: error names the component.
    let read_only = Access::new().read(vel).read(pos);
    let err = w
        .query()
        .access(read_only)
        .each::<(&Vel, &mut Pos)>(|_, _| {})
        .unwrap_err();
    assert_eq!(err.code, codes::ACCESS_VIOLATION);
    assert!(err.message.contains("writes Pos"), "{}", err.message);
    assert!(err.hint.contains("writes"), "{}", err.hint);

    // Reading something never declared.
    let err = w
        .query()
        .access(Access::new().write(pos))
        .each::<(&Vel, &mut Pos)>(|_, _| {})
        .unwrap_err();
    assert_eq!(err.code, codes::ACCESS_VIOLATION);
    assert!(err.message.contains("reads Vel"));

    // Writing implies reading.
    w.query()
        .access(Access::new().write(pos))
        .each::<&Pos>(|_, _| {})
        .unwrap();

    // From names, as a Luau system would declare it.
    let a = Access::from_names(reg, &["Vel"], &["Pos"]).unwrap();
    assert_eq!(a, ok);
    assert_eq!(
        Access::from_names(reg, &["Nope"], &[]).unwrap_err().code,
        codes::UNKNOWN_COMPONENT
    );
}

#[test]
fn fetching_the_same_column_twice_is_a_borrow_error() {
    let (w, _) = world();
    let err = w.query().each::<(&mut Pos, &Pos)>(|_, _| {}).unwrap_err();
    assert_eq!(err.code, codes::BORROW_CONFLICT);
    // Two shared views are fine.
    w.query()
        .each::<(&Pos, &Pos)>(|_, (a, b)| assert_eq!(a, b))
        .unwrap();
    // And the world is usable afterwards: the failed borrow was released.
    w.query().each::<&mut Pos>(|_, p| p.v.y = 1.0).unwrap();
}

#[test]
fn archetype_views_give_typed_and_dynamic_access() {
    let (w, _) = world();
    let pos = w.component_id_of::<Pos>().unwrap();
    let mut rows = 0;
    w.query()
        .with::<Pos>()
        .for_each_view(|view| {
            let col = view.column::<Pos>()?;
            let dynv = view.dyn_column(pos)?;
            assert_eq!(col.len(), dynv.len());
            for row in 0..view.len() {
                assert_eq!(dynv.get(row, "v")?, Value::Vec2(col[row].v));
                assert!(
                    dynv.set(row, "v", Value::Vec2(Vec2::ZERO)).is_err(),
                    "read-only view refuses writes"
                );
            }
            rows += view.len();
            Ok(())
        })
        .unwrap();
    assert_eq!(rows, 6);

    // Dynamic writes with range checks through a writable view.
    let mut w = w;
    let lua = w
        .register_schema(
            SchemaBuilder::new("Mana")
                .field_full(
                    "cur",
                    FieldType::I32,
                    Value::Int(5),
                    Some((0.0, 10.0)),
                    "",
                    true,
                )
                .build()
                .unwrap(),
        )
        .unwrap();
    let e = w.spawn();
    w.insert_default(e, lua).unwrap();
    w.query()
        .has(lua)
        .for_each_view(|view| {
            let col = view.dyn_column_mut(lua)?;
            assert_eq!(col.get(0, "cur")?, Value::Int(5));
            col.set(0, "cur", 9i64)?;
            let err = col.set(0, "cur", 11i64).unwrap_err();
            assert_eq!(err.code, codes::OUT_OF_RANGE);
            assert_eq!(col.schema().name, "Mana");
            Ok(())
        })
        .unwrap();
    assert_eq!(w.get_field(e, "Mana.cur").unwrap(), Value::Int(9));

    // Access applies to dynamic columns too.
    let err = w
        .query()
        .has(lua)
        .access(Access::new().read(lua))
        .for_each_view(|view| view.dyn_column_mut(lua).map(|_| ()))
        .unwrap_err();
    assert_eq!(err.code, codes::ACCESS_VIOLATION);
}

#[test]
fn queries_see_structural_changes_made_between_passes() {
    let (mut w, es) = world();
    w.despawn(es[0]).unwrap();
    w.remove_type::<Vel>(es[2]).unwrap();
    assert_eq!(w.query().with::<Vel>().count(), 1);
    let e = w.spawn();
    w.insert(e, Vel::default()).unwrap();
    assert_eq!(w.query().with::<Vel>().count(), 2);
    assert_eq!(w.query().with::<Vel>().without::<Pos>().count(), 1);
}
