//! §17 targets that the ECS core alone can be held to. These are ignored by
//! default and meaningful only in release: run with
//! `cargo test -p argus-ecs --release -- --ignored perf`.
//!
//! - Full snapshot at 100k entities: < 5 ms.
//! - A sim-style pass over 100k entities (read Vel, write Pos): well inside
//!   the 6 ms Swarm tick budget; asserted at < 2 ms.
//! - Query over 100k entities (count + filter): < 2 ms.

use argus_ecs::{Component, Vec2, World};
use std::time::{Duration, Instant};

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
struct Life {
    frames: u32,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
struct Bullet;

const N: usize = 100_000;

fn swarm() -> World {
    let mut w = World::new();
    for i in 0..N {
        let e = w.spawn();
        w.insert(
            e,
            Pos {
                v: Vec2::new(i as f32, 0.0),
            },
        )
        .unwrap();
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
    w.drain_events();
    w
}

fn best_of(n: usize, mut f: impl FnMut()) -> Duration {
    (0..n)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed()
        })
        .min()
        .unwrap()
}

#[test]
#[ignore = "release-only perf gate; run with --release -- --ignored"]
fn perf_snapshot_100k_under_5ms() {
    let w = swarm();
    let d = best_of(5, || {
        let s = w.snapshot();
        std::hint::black_box(s.entity_count());
    });
    eprintln!("snapshot 100k: {d:?}");
    assert!(d < Duration::from_millis(5), "snapshot took {d:?}");
}

#[test]
#[ignore = "release-only perf gate; run with --release -- --ignored"]
fn perf_sim_pass_100k_under_2ms() {
    let w = swarm();
    let d = best_of(5, || {
        w.query()
            .each::<(&Vel, &mut Pos)>(|_, (v, p)| {
                p.v.x += v.v.x;
                p.v.y += v.v.y;
            })
            .unwrap();
    });
    eprintln!("sim pass 100k: {d:?}");
    assert!(d < Duration::from_millis(2), "sim pass took {d:?}");
}

#[test]
#[ignore = "release-only perf gate; run with --release -- --ignored"]
fn perf_query_100k_under_2ms() {
    let w = swarm();
    let d = best_of(5, || {
        let mut n = 0usize;
        w.query()
            .with::<Bullet>()
            .each::<&Life>(|_, l| n += (l.frames > 30) as usize)
            .unwrap();
        assert_eq!(n, N / 2);
    });
    eprintln!("query 100k: {d:?}");
    assert!(d < Duration::from_millis(2), "query took {d:?}");
}

#[test]
#[ignore = "release-only perf gate; run with --release -- --ignored"]
fn perf_spawn_and_despawn_100k() {
    let t = Instant::now();
    let mut w = swarm();
    let spawn = t.elapsed();
    let es = w.query().with::<Bullet>().entities();
    let t = Instant::now();
    for e in es {
        w.despawn(e).unwrap();
    }
    let despawn = t.elapsed();
    eprintln!("spawn 100k (4 inserts each): {spawn:?}; despawn 50k: {despawn:?}");
    assert_eq!(w.entity_count(), N / 2);
    assert!(spawn < Duration::from_millis(500), "spawn took {spawn:?}");
    assert!(
        despawn < Duration::from_millis(50),
        "despawn took {despawn:?}"
    );
}
