//! §17 targets the scheduler owns. Release only:
//! `cargo test -p argus-sim --release --test perf -- --ignored --nocapture`
//!
//! - Outpost headless fast-forward: > 2,000 ticks/s. Asserted with a
//!   Swarm-shaped world (50k entities, three systems) at > 2,000 ticks/s,
//!   which is far above what the Outpost row needs from the scheduler itself.
//! - Delve turn resolution, 500 monsters: < 2 ms per turn.
//! - Scheduler overhead per tick with no work: < 20 µs.

use argus_ecs::{Component, Vec2, World};
use argus_sim::{Scheduler, SchedulerConfig, Stage, SystemDesc};
use std::time::{Duration, Instant};

// The gates run with the counting allocator installed, as argusd has it, so
// the numbers include prof.alloc's cost.
#[global_allocator]
static ALLOC: argus_sim::alloc::Counting = argus_sim::alloc::Counting;

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
#[repr(C)]
struct Monster {
    hp: i32,
    target: u32,
}

fn swarm(n: usize) -> (World, Scheduler) {
    let mut w = World::new();
    let pos = w.register::<Pos>().unwrap();
    let vel = w.register::<Vel>().unwrap();
    let life = w.register::<Life>().unwrap();
    for i in 0..n {
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
        w.insert(e, Life { frames: 1_000_000 }).unwrap();
    }
    w.drain_events();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0).seed(1));
    s.add_system(
        SystemDesc::new("sim.move", Stage::Sim)
            .reads(vel)
            .writes(pos),
        Box::new(|ctx| {
            ctx.query().each::<(&Vel, &mut Pos)>(|_, (v, p)| {
                p.v.x += v.v.x;
                p.v.y += v.v.y;
            })
        }),
    )
    .unwrap();
    s.add_system(
        SystemDesc::new("sim.age", Stage::Sim).writes(life),
        Box::new(|ctx| ctx.query().each::<&mut Life>(|_, l| l.frames -= 1)),
    )
    .unwrap();
    s.add_system(
        SystemDesc::new("late.count", Stage::Late).reads(life),
        Box::new(|ctx| {
            let mut n = 0usize;
            ctx.query()
                .each::<&Life>(|_, l| n += (l.frames == 0) as usize)?;
            std::hint::black_box(n);
            Ok(())
        }),
    )
    .unwrap();
    (w, s)
}

#[test]
#[ignore = "release-only perf gate; run with --release -- --ignored"]
fn perf_headless_fast_forward_over_2000_ticks_per_second() {
    let (mut w, mut s) = swarm(50_000);
    let t = Instant::now();
    s.step(&mut w, 2_000).unwrap();
    let el = t.elapsed();
    let tps = 2_000.0 / el.as_secs_f64();
    eprintln!(
        "50k-entity swarm, 3 systems: {tps:.0} ticks/s ({el:?} for 2000 ticks); tick p95 {:.3} ms",
        s.prof_tick_p95_ms(600)
    );
    assert!(tps > 2_000.0, "{tps} ticks/s");
}

#[test]
#[ignore = "release-only perf gate; run with --release -- --ignored"]
fn perf_delve_turn_500_monsters_under_2ms() {
    let mut w = World::new();
    let m = w.register::<Monster>().unwrap();
    let pos = w.register::<Pos>().unwrap();
    for i in 0..500u32 {
        let e = w.spawn();
        w.insert(e, Monster { hp: 10, target: i }).unwrap();
        w.insert(e, Pos::default()).unwrap();
    }
    let mut s = Scheduler::new(SchedulerConfig::discrete().seed(2));
    s.add_system(
        SystemDesc::new("sim.monsters", Stage::Sim)
            .writes(m)
            .writes(pos)
            .rng("monsters"),
        Box::new(|ctx| {
            // Each monster picks a direction and takes a step, with a hit chance.
            let mut moves = Vec::with_capacity(512);
            ctx.query()
                .each::<&Monster>(|e, mo| moves.push((e, mo.target)))?;
            for (e, _) in moves {
                let dir = ctx.rng()?.below(4);
                let hit = ctx.rng()?.chance(0.1);
                ctx.update::<Pos, _>(e, |p| match dir {
                    0 => p.v.x += 1.0,
                    1 => p.v.x -= 1.0,
                    2 => p.v.y += 1.0,
                    _ => p.v.y -= 1.0,
                })?;
                if hit {
                    ctx.update::<Monster, _>(e, |m| m.hp -= 1)?;
                }
            }
            Ok(())
        }),
    )
    .unwrap();
    s.advance(&mut w, 50).unwrap();
    let p95 = s.prof_tick_p95_ms(50);
    eprintln!("Delve turn, 500 monsters: p95 {p95:.3} ms");
    assert!(p95 < 2.0, "turn p95 {p95} ms");
}

#[test]
#[ignore = "release-only perf gate; run with --release -- --ignored"]
fn perf_empty_tick_overhead() {
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    for i in 0..10 {
        s.add_system(
            SystemDesc::new(format!("sim.s{i}"), Stage::Sim),
            Box::new(|_| Ok(())),
        )
        .unwrap();
    }
    s.step(&mut w, 100).unwrap();
    let t = Instant::now();
    s.step(&mut w, 10_000).unwrap();
    let per = t.elapsed() / 10_000;
    eprintln!("empty tick with 10 systems: {per:?}");
    assert!(per < Duration::from_micros(20), "{per:?} per tick");
}
