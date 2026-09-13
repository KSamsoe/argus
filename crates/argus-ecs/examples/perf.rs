//! Prints the §17 numbers this crate is responsible for. Run in release:
//! `cargo run -p argus-ecs --release --example perf`

use argus_ecs::{hierarchy, propagate, Component, GlobalTransform, Parent, Transform, Vec2, World};
use std::time::Instant;

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
struct Vel {
    v: Vec2,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
struct Life {
    frames: u32,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
struct Bullet;

fn main() {
    let n = 100_000;
    let mut w = World::new();
    hierarchy::register(&mut w).unwrap();

    let t = Instant::now();
    for i in 0..n {
        let e = w.spawn();
        w.insert(e, Transform::at(i as f32, 0.0)).unwrap();
        w.insert(
            e,
            Vel {
                v: Vec2::new(1.0, 0.5),
            },
        )
        .unwrap();
        w.insert(e, Life { frames: 60 }).unwrap();
        if i % 2 == 0 {
            w.insert(e, Bullet).unwrap();
        }
    }
    println!("spawn {n} entities x 3-4 inserts   {:>8.3} ms", ms(t));

    // A small authored hierarchy alongside the swarm (Hollow-shaped: 2,000 entities).
    let root = w.spawn_authored("root").unwrap();
    w.insert(root, Transform::at(1.0, 1.0)).unwrap();
    w.insert(root, GlobalTransform::default()).unwrap();
    for i in 0..2000 {
        let e = w.spawn_authored(&format!("prop.{i}")).unwrap();
        w.insert(e, Transform::at(i as f32, 0.0)).unwrap();
        w.insert(e, GlobalTransform::default()).unwrap();
        w.insert(e, Parent { entity: root }).unwrap();
    }
    w.drain_events();

    let t = Instant::now();
    w.query()
        .each::<(&Vel, &mut Transform)>(|_, (v, p)| {
            p.pos.x += v.v.x;
            p.pos.y += v.v.y;
        })
        .unwrap();
    println!(
        "sim pass (Vel -> Transform) over {n}   {:>8.3} ms   target < 6 ms tick",
        ms(t)
    );

    let t = Instant::now();
    let mut count = 0usize;
    w.query()
        .with::<Bullet>()
        .each::<&Life>(|_, l| count += (l.frames > 30) as usize)
        .unwrap();
    println!(
        "query with filter over {n}         {:>8.3} ms   target < 2 ms",
        ms(t)
    );

    let t = Instant::now();
    propagate(&mut w).unwrap();
    println!("hierarchy propagate, 2,000 children  {:>8.3} ms", ms(t));

    let t = Instant::now();
    let snap = w.snapshot();
    println!(
        "snapshot at {} entities ({:.1} MB)   {:>8.3} ms   target < 5 ms",
        snap.entity_count(),
        snap.bytes() as f64 / 1e6,
        ms(t)
    );

    let t = Instant::now();
    w.restore(&snap);
    println!("restore                              {:>8.3} ms", ms(t));

    let t = Instant::now();
    let n_nulled = w.end_frame();
    println!(
        "end_frame maintenance ({n_nulled} nulled)   {:>8.3} ms",
        ms(t)
    );

    let t = Instant::now();
    let es = w.query().with::<Bullet>().entities();
    for e in es {
        w.despawn(e).unwrap();
    }
    println!("despawn {}                       {:>8.3} ms", n / 2, ms(t));
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}
