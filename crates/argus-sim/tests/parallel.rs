//! Parallel stage scheduling (phase 5 item 5): §3 "Each system declares the
//! components it reads and writes. The scheduler uses this for
//! parallelism"; §17 "one thread for sim unless the scheduler finds
//! parallel stages"; §6, the world at tick N is the same with one thread or
//! many.

use argus_ecs::{Component, ComponentId, Event, FieldType, SchemaBuilder, Value, World};
use argus_sim::{codes as sim_codes, ParCtx, Scheduler, SchedulerConfig, Stage, SystemDesc};
use std::time::Duration;

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
struct Seed {
    v: u32,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
struct A {
    v: u64,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
struct B {
    v: u64,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
struct C {
    v: u64,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
struct D {
    v: u64,
}

struct Ids {
    seed: ComponentId,
    a: ComponentId,
    b: ComponentId,
    c: ComponentId,
    d: ComponentId,
    label: ComponentId,
}

fn world(n: usize) -> (World, Ids) {
    let mut w = World::new();
    let ids = Ids {
        seed: w.register::<Seed>().unwrap(),
        a: w.register::<A>().unwrap(),
        b: w.register::<B>().unwrap(),
        c: w.register::<C>().unwrap(),
        d: w.register::<D>().unwrap(),
        label: w
            .register_schema(
                SchemaBuilder::new("Label")
                    .field("s", FieldType::Str)
                    .build()
                    .unwrap(),
            )
            .unwrap(),
    };
    for i in 0..n {
        let e = w.spawn();
        w.insert(e, Seed { v: i as u32 }).unwrap();
        w.insert(e, A::default()).unwrap();
        w.insert(e, B::default()).unwrap();
        w.insert(e, C::default()).unwrap();
        w.insert(e, D::default()).unwrap();
    }
    (w, ids)
}

/// Work enough to measure: a hash folded `rounds` times.
fn churn(x: u64, rounds: u32) -> u64 {
    let mut h = x;
    for _ in 0..rounds {
        h = (h ^ (h >> 31)).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
    h
}

/// A parallel system that writes one component from Seed and its stream,
/// and now and then emits, spawns, and despawns.
fn writer(
    s: &mut Scheduler,
    name: &str,
    ids: &Ids,
    out: ComponentId,
    put: fn(&mut ParCtx<'_>, u32, u64) -> argus_ecs::Result<()>,
) {
    let desc = SystemDesc::new(name, Stage::Sim)
        .reads(ids.seed)
        .writes(out)
        .spawns(ids.label)
        .rng(name.to_string());
    let label = ids.label;
    s.add_parallel_system(
        desc,
        Box::new(move |ctx| {
            let roll = ctx.rng()?.next_u32();
            let tick = ctx.tick;
            put(ctx, roll, tick)?;
            if roll % 7 == 0 {
                let name = format!("{}.rolled", ctx.name);
                ctx.emit(name, vec![("roll".into(), Value::UInt(roll as u64))]);
            }
            if roll % 5 == 0 {
                ctx.spawn(&[label], vec![("Label.s", Value::Str(format!("{roll}")))])?;
            }
            Ok(())
        }),
    )
    .unwrap();
}

fn heavy(s: &mut Scheduler, ids: &Ids) {
    macro_rules! put {
        ($t:ident) => {
            |ctx: &mut ParCtx<'_>, roll: u32, tick: u64| {
                ctx.query().each::<(&Seed, &mut $t)>(|_, (seed, out)| {
                    out.v = churn(out.v ^ seed.v as u64 ^ roll as u64 ^ tick, ROUNDS);
                })
            }
        };
    }
    const ROUNDS: u32 = 64;
    writer(s, "sim.a", ids, ids.a, put!(A));
    writer(s, "sim.b", ids, ids.b, put!(B));
    writer(s, "sim.c", ids, ids.c, put!(C));
    writer(s, "sim.d", ids, ids.d, put!(D));
}

fn run(threads: usize, floor: Duration, ticks: u64) -> (Vec<u64>, Vec<String>, Vec<String>, bool) {
    let (mut w, ids) = world(4000);
    let mut s = Scheduler::new(SchedulerConfig {
        threads,
        ..SchedulerConfig::continuous(60.0).seed(7)
    });
    s.set_parallel_floor(floor);
    heavy(&mut s, &ids);
    // A serial system after the batch, reading what it wrote and despawning
    // some of what it spawned.
    let (a, b, label) = (ids.a, ids.b, ids.label);
    s.add_system(
        SystemDesc::new("sim.after", Stage::Sim)
            .reads(a)
            .reads(b)
            .writes(label)
            .after("sim.d"),
        Box::new(move |ctx| {
            let mut sum = 0u64;
            ctx.query()
                .each::<(&A, &B)>(|_, (a, b)| sum = sum.wrapping_add(a.v ^ b.v))?;
            let labelled = ctx.query().has(label).entities();
            for e in labelled
                .into_iter()
                .filter(|e| e.index() as u64 % 3 == sum % 3)
            {
                ctx.despawn(e)?;
            }
            ctx.emit("after.sum", vec![("sum".into(), Value::UInt(sum))]);
            Ok(())
        }),
    )
    .unwrap();
    let mut digests = Vec::new();
    let mut events = Vec::new();
    s.play(&w);
    for _ in 0..ticks {
        s.step(&mut w, 1).unwrap();
        digests.push(w.digest());
        for r in w.drain_events() {
            events.push(format!("{} {:?} {:?}", r.frame, r.by, r.event));
        }
    }
    let threaded = s.batches().unwrap().iter().any(|b| b.threaded);
    (digests, events, w.strings().all(), threaded)
}

#[test]
fn systems_that_do_not_conflict_share_a_batch_and_ones_that_do_do_not() {
    let (w, ids) = world(1);
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    let body = || -> argus_sim::ParSystemFn { Box::new(|_| Ok(())) };
    let sim = |n: &str| SystemDesc::new(n, Stage::Sim);
    // a and b both read Seed and write their own: one batch.
    s.add_parallel_system(sim("p.a").reads(ids.seed).writes(ids.a), body())
        .unwrap();
    s.add_parallel_system(sim("p.b").reads(ids.seed).writes(ids.b), body())
        .unwrap();
    // c reads A, which a writes: a new batch.
    s.add_parallel_system(sim("p.c").reads(ids.a).writes(ids.c), body())
        .unwrap();
    // d orders itself after c: a new batch, though they do not conflict.
    s.add_parallel_system(sim("p.d").writes(ids.d).after("p.c"), body())
        .unwrap();
    // A serial system ends a batch.
    s.add_system(sim("s.e").reads(ids.seed), Box::new(|_| Ok(())))
        .unwrap();
    s.add_parallel_system(sim("p.f").reads(ids.seed), body())
        .unwrap();
    s.add_parallel_system(sim("p.g").reads(ids.seed), body())
        .unwrap();
    // In another stage: its own batch.
    s.add_parallel_system(SystemDesc::new("p.h", Stage::Late).reads(ids.seed), body())
        .unwrap();
    let infos = s.systems(w.registry()).unwrap();
    let batch = |n: &str| infos.iter().find(|i| i.name == n).unwrap().batch;
    assert_eq!(batch("p.a"), Some(0));
    assert_eq!(batch("p.b"), Some(0));
    assert_eq!(batch("p.c"), Some(1));
    assert_eq!(batch("p.d"), Some(2));
    assert_eq!(batch("s.e"), None);
    assert_eq!(batch("p.f"), Some(3));
    assert_eq!(batch("p.g"), Some(3));
    assert_eq!(batch("p.h"), Some(4));
    assert!(infos.iter().find(|i| i.name == "p.a").unwrap().parallel);
    assert!(!infos.iter().find(|i| i.name == "s.e").unwrap().parallel);
    let batches = s.batches().unwrap();
    assert_eq!(batches.len(), 5);
    assert_eq!(batches[0].systems, vec!["p.a", "p.b"]);
    assert_eq!(batches[4].stage, Stage::Late);
}

#[test]
fn a_batch_is_the_same_on_one_thread_or_many() {
    let one = run(1, Duration::ZERO, 40);
    let four = run(4, Duration::ZERO, 40);
    assert!(!one.3, "one thread never threads");
    assert!(four.3, "four threads with no floor thread every batch");
    assert_eq!(one.0, four.0, "the world at every tick");
    assert_eq!(one.1, four.1, "every event, its frame, and who emitted it");
    assert_eq!(one.2, four.2, "the string table, id for id");
    assert!(one.1.iter().any(|e| e.contains("sim.c.rolled")));
    // And a third run threads again to the same world.
    assert_eq!(run(3, Duration::ZERO, 40).0, one.0);
}

#[test]
fn a_parallel_system_s_events_and_spawns_land_after_its_batch_in_run_order() {
    let (mut w, ids) = world(3);
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    s.set_parallel_floor(Duration::ZERO);
    s.set_threads(2);
    let label = ids.label;
    s.add_parallel_system(
        SystemDesc::new("p.first", Stage::Sim).spawns(label),
        Box::new(move |ctx| {
            ctx.emit("first.one", vec![]);
            ctx.spawn(&[label], vec![("Label.s", Value::Str("brand new".into()))])?;
            ctx.emit("first.two", vec![]);
            Ok(())
        }),
    )
    .unwrap();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let saw = seen.clone();
    s.add_parallel_system(
        SystemDesc::new("p.second", Stage::Sim).reads(ids.seed),
        Box::new(move |ctx| {
            // Nothing of the first one's yet: not its events, not its spawn.
            let events = ctx.events_now("first.one").count();
            let labels = ctx.query().has(label).count();
            saw.lock().unwrap().push((events, labels));
            ctx.emit("second.one", vec![]);
            Ok(())
        }),
    )
    .unwrap();
    s.play(&w);
    s.step(&mut w, 1).unwrap();
    assert_eq!(s.batches().unwrap()[0].systems.len(), 2);
    assert_eq!(*seen.lock().unwrap(), vec![(0, 0)]);
    let names: Vec<(String, Option<String>)> = w
        .events()
        .iter()
        .filter_map(|r| match &r.event {
            Event::Game { name, .. } => Some((name.clone(), r.by.as_deref().map(str::to_string))),
            _ => None,
        })
        .collect();
    assert_eq!(
        names,
        vec![
            ("first.one".to_string(), Some("p.first".to_string())),
            ("first.two".to_string(), Some("p.first".to_string())),
            ("second.one".to_string(), Some("p.second".to_string())),
        ]
    );
    let spawned = w.query().has(label).entities();
    assert_eq!(spawned.len(), 1);
    assert_eq!(
        w.get_field(spawned[0], "Label.s").unwrap(),
        Value::Str("brand new".into()),
        "a spawn's new string is written after the batch"
    );
}

#[test]
fn a_parallel_system_that_writes_a_new_string_or_outside_its_declaration_is_stopped() {
    let (mut w, ids) = world(2);
    let e = w.query().has(ids.seed).entities()[0];
    w.insert_default(e, ids.label).unwrap();
    w.set_field(e, "Label.s", "old").unwrap();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    s.add_parallel_system(
        SystemDesc::new("p.strings", Stage::Sim).writes(ids.label),
        Box::new(move |ctx| {
            ctx.set_field(e, "Label.s", "old")?;
            ctx.set_field(e, "Label.s", "never seen")
        }),
    )
    .unwrap();
    s.add_parallel_system(
        SystemDesc::new("p.reach", Stage::Sim).reads(ids.seed),
        Box::new(move |ctx| ctx.update::<A, _>(e, |a| a.v = 9)),
    )
    .unwrap();
    s.play(&w);
    s.step(&mut w, 1).unwrap();
    assert!(!s.is_enabled("p.strings").unwrap());
    assert!(!s.is_enabled("p.reach").unwrap());
    let faults: Vec<String> = w
        .events()
        .iter()
        .filter_map(|r| match &r.event {
            Event::SystemFault {
                system, message, ..
            } => Some(format!("{system}: {message}")),
            _ => None,
        })
        .collect();
    assert_eq!(faults.len(), 2, "{faults:?}");
    assert!(faults[0].contains("never seen"), "{faults:?}");
    assert!(
        faults[1].contains("writes A without declaring it"),
        "{faults:?}"
    );
    assert_eq!(w.get_field(e, "Label.s").unwrap(), Value::Str("old".into()));
    assert_eq!(w.get::<A>(e).unwrap().v, 0);
    assert!(!w.strings().is_sealed(), "unsealed after the batch");
    assert!(
        w.strings().try_intern("never seen").is_ok(),
        "and a serial write may add it"
    );
}

#[test]
fn a_parallel_system_may_not_be_exclusive() {
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    let err = s
        .add_parallel_system(
            SystemDesc::new("p.loader", Stage::Sim).exclusive(),
            Box::new(|_| Ok(())),
        )
        .unwrap_err();
    assert_eq!(err.code, sim_codes::NOT_PARALLEL);
}

#[test]
fn a_threaded_batch_shows_its_threads_in_a_capture() {
    let (mut w, ids) = world(500);
    let mut s = Scheduler::new(SchedulerConfig {
        threads: 2,
        ..SchedulerConfig::continuous(60.0)
    });
    s.set_parallel_floor(Duration::ZERO);
    heavy(&mut s, &ids);
    s.play(&w);
    s.capture_start(2);
    s.step(&mut w, 2).unwrap();
    let cap = s.capture_take().unwrap();
    let lanes: Vec<u32> = cap.ticks[1].systems.iter().map(|x| x.lane).collect();
    assert!(lanes.contains(&0) && lanes.contains(&1), "{lanes:?}");
    assert!(cap.chrome_trace().contains("\"name\":\"batch 1\""));
    // Every system was timed, wherever it ran.
    let prof = s.prof_systems(2).unwrap();
    for n in ["sim.a", "sim.b", "sim.c", "sim.d"] {
        assert!(prof.iter().any(|p| p.name == n && p.samples == 2), "{n}");
    }
}

#[test]
fn a_parallel_system_runs_at_load_and_in_present() {
    let (mut w, ids) = world(1);
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    let a = ids.a;
    s.add_parallel_system(
        SystemDesc::new("p.load", Stage::Sim).writes(a).on_load(),
        Box::new(|ctx| {
            ctx.query().each::<(&mut A,)>(|_, (a,)| a.v += 1)?;
            Ok(())
        }),
    )
    .unwrap();
    s.add_parallel_system(
        SystemDesc::new("p.look", Stage::Present).reads(a),
        Box::new(|ctx| {
            ctx.emit("looked", vec![]);
            Ok(())
        }),
    )
    .unwrap();
    assert_eq!(s.run_load(&mut w).unwrap(), 1);
    let e = w.query().has(a).entities()[0];
    assert_eq!(w.get::<A>(e).unwrap().v, 1);
    assert!(s.present(&mut w).unwrap());
    assert!(w
        .events()
        .iter()
        .any(|r| matches!(&r.event, Event::Game { name, .. } if name == "looked")));
}
