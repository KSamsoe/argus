//! A world shared by threads (phase 5 item 5): what a parallel batch of
//! systems relies on. The scheduler keeps the batch's writes apart; these
//! pin what the world itself promises: it may be shared, a column's borrow
//! flag still refuses aliasing across threads, and a sealed string table
//! refuses new strings.

use argus_ecs::{codes, Component, FieldType, SchemaBuilder, Value, Vec2, World};

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

fn shared<T: Send + Sync>() {}

#[test]
fn a_world_may_be_shared_between_threads() {
    shared::<World>();
}

#[test]
fn two_threads_write_different_columns_and_read_one() {
    let mut w = World::new();
    w.register::<Pos>().unwrap();
    w.register::<Vel>().unwrap();
    let mut es = Vec::new();
    for i in 0..1000 {
        let e = w.spawn();
        w.insert(
            e,
            Vel {
                v: Vec2::new(i as f32, 1.0),
            },
        )
        .unwrap();
        w.insert(e, Pos::default()).unwrap();
        es.push(e);
    }
    let w = &w;
    std::thread::scope(|s| {
        // One writes Pos from Vel; the other reads Vel too. Both read Vel
        // at once, which the flag allows.
        s.spawn(|| {
            w.query()
                .each::<(&Vel, &mut Pos)>(|_, (v, p)| p.v = v.v)
                .unwrap();
        });
        s.spawn(|| {
            let mut sum = 0.0;
            w.query().each::<(&Vel,)>(|_, (v,)| sum += v.v.x).unwrap();
            assert_eq!(sum, (0..1000).sum::<i32>() as f32);
        });
    });
    assert_eq!(w.get::<Pos>(es[7]).unwrap().v, Vec2::new(7.0, 1.0));
}

#[test]
fn a_column_held_on_one_thread_is_refused_on_another() {
    let mut w = World::new();
    w.register::<Pos>().unwrap();
    let e = w.spawn();
    w.insert(e, Pos::default()).unwrap();
    let w = &w;
    let held = std::sync::Barrier::new(2);
    let tried = std::sync::Barrier::new(2);
    std::thread::scope(|s| {
        s.spawn(|| {
            w.query()
                .for_each_view(|v| {
                    let _cols = v.column_mut::<Pos>()?;
                    held.wait();
                    tried.wait();
                    Ok(())
                })
                .unwrap();
        });
        s.spawn(|| {
            held.wait();
            let err = w.get::<Pos>(e).unwrap_err();
            assert_eq!(err.code, codes::BORROW_CONFLICT);
            tried.wait();
        });
    });
    // Released on both sides.
    w.update::<Pos, _>(e, |p| p.v.x = 1.0).unwrap();
}

#[test]
fn a_sealed_table_keeps_old_strings_and_refuses_new_ones() {
    let mut w = World::new();
    let name = w
        .register_schema(
            SchemaBuilder::new("Name")
                .field("s", FieldType::Str)
                .build()
                .unwrap(),
        )
        .unwrap();
    let e = w.spawn();
    w.insert_default(e, name).unwrap();
    w.set_field(e, "Name.s", "Ada").unwrap();
    w.strings().seal();
    w.set_field(e, "Name.s", "").unwrap();
    w.set_field(e, "Name.s", "Ada").unwrap();
    let err = w.set_field(e, "Name.s", "Grace").unwrap_err();
    assert_eq!(err.code, codes::SEALED_STRINGS);
    assert_eq!(w.get_field(e, "Name.s").unwrap(), Value::Str("Ada".into()));
    assert!(w.strings().lookup("Grace").is_none());
    w.strings().unseal();
    w.set_field(e, "Name.s", "Grace").unwrap();
    assert!(w.strings().lookup("Grace").is_some());
}
