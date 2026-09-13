//! §5 "Profiling as a query": every system timed every tick; prof.systems
//! gives mean and p95 in stage order; prof.worst gives the slowest ticks with
//! a per-system breakdown and entity counts by archetype.

use argus_ecs::{Component, World};
use argus_sim::{ClockKind, Scheduler, SchedulerConfig, Stage, SystemDesc};

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
struct Tag {
    n: u32,
}

#[test]
fn every_system_is_timed_every_tick() {
    let mut w = World::new();
    let tag = w.register::<Tag>().unwrap();
    for _ in 0..10 {
        let e = w.spawn();
        w.insert(e, Tag::default()).unwrap();
    }
    let mut s = Scheduler::new(SchedulerConfig {
        prof_ticks: 8,
        ..SchedulerConfig::continuous(60.0)
    });
    s.add_system(
        SystemDesc::new("sim.fast", Stage::Sim).reads(tag),
        Box::new(|_| Ok(())),
    )
    .unwrap();
    s.add_system(
        SystemDesc::new("sim.slow", Stage::Sim).after("sim.fast"),
        Box::new(|_| {
            // Deterministic busy work rather than sleep, so the number is real CPU time.
            let mut x = 0u64;
            for i in 0..20_000u64 {
                x = x.wrapping_mul(31).wrapping_add(i);
            }
            std::hint::black_box(x);
            Ok(())
        }),
    )
    .unwrap();
    s.add_system(
        SystemDesc::new("late.tidy", Stage::Late),
        Box::new(|_| Ok(())),
    )
    .unwrap();
    s.add_system(
        SystemDesc::new("present.ui", Stage::Present),
        Box::new(|_| Ok(())),
    )
    .unwrap();

    s.step(&mut w, 20).unwrap();
    assert_eq!(s.prof_len(), 8, "ring keeps the configured window");

    let profs = s.prof_systems(600).unwrap();
    let names: Vec<&str> = profs.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "sim.fast",
            "sim.slow",
            "late.tidy",
            "hierarchy.propagate",
            "present.ui"
        ]
    );
    let slow = &profs[1];
    let fast = &profs[0];
    assert_eq!(slow.samples, 8);
    assert!(slow.p95_ms >= slow.mean_ms * 0.5);
    assert!(slow.max_ms >= slow.p95_ms);
    assert!(
        slow.mean_ms >= fast.mean_ms,
        "busy system costs at least as much as an empty one"
    );
    assert_eq!(profs[4].samples, 0, "present is not part of a tick");

    let last = s.prof_last().unwrap();
    assert_eq!(last.tick, 19);
    assert_eq!(last.clock, ClockKind::Continuous);
    assert_eq!(last.systems.len(), 4);
    assert_eq!(last.archetypes, vec![(1, 10)]);

    let worst = s.prof_worst(3);
    assert_eq!(worst.len(), 3);
    assert!(worst[0].total >= worst[1].total && worst[1].total >= worst[2].total);
    assert!(s.prof_tick_p95_ms(600) >= 0.0);
}

#[test]
fn full_snapshot_carries_world_and_sim_state() {
    let mut w = World::new();
    let tag = w.register::<Tag>().unwrap();
    let e = w.spawn_authored("e").unwrap();
    w.insert(e, Tag::default()).unwrap();
    let mut s = Scheduler::new(SchedulerConfig::continuous(30.0).seed(3));
    s.add_system(
        SystemDesc::new("sim.inc", Stage::Sim).writes(tag).rng("r"),
        Box::new(|ctx| {
            let bump = ctx.rng()?.below(3);
            let e = ctx.entity("@e")?;
            ctx.update::<Tag, _>(e, |t| t.n += bump)
        }),
    )
    .unwrap();
    s.play(&w);
    s.set_speed(2.0).unwrap();
    s.step(&mut w, 10).unwrap();
    let snap = s.snapshot(&w);
    assert_eq!(snap.sim.continuous.tick, 10);
    assert_eq!(snap.sim.seed, 3);
    assert_eq!(snap.sim.rng.len(), 1);
    assert!(snap.sim.playing);
    let n10 = w.get::<Tag>(e).unwrap().n;

    s.step(&mut w, 5).unwrap();
    let n15 = w.get::<Tag>(e).unwrap().n;
    s.restore(&mut w, &snap);
    assert_eq!(w.get::<Tag>(e).unwrap().n, n10);
    assert_eq!(s.tick(), 10);
    assert_eq!(s.speed(), 2.0);
    s.step(&mut w, 5).unwrap();
    assert_eq!(w.get::<Tag>(e).unwrap().n, n15, "same rolls after restore");
}
