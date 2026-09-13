//! §6 "Randomness" and "A run is a value": named PCG32 streams seeded from
//! hash(run_seed, stream_name); adding or reordering a system does not
//! perturb other streams; same scene + seed + systems gives a bit-identical
//! world; sim state restores mid-sequence.

use argus_ecs::{Component, Value, World};
use argus_sim::{
    codes as sim_codes, stream_seed, Pcg32, Scheduler, SchedulerConfig, Stage, SystemDesc,
};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
struct Roll {
    last: u32,
    sum: u64,
}

fn build(seed: u64, with_extra: bool) -> (World, Scheduler) {
    let mut w = World::new();
    let roll = w.register::<Roll>().unwrap();
    let e = w.spawn_authored("dice").unwrap();
    w.insert(e, Roll::default()).unwrap();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0).seed(seed));
    if with_extra {
        // An unrelated random system registered *before* the dice system.
        s.add_system(
            SystemDesc::new("sim.weather", Stage::Sim).rng("weather"),
            Box::new(|ctx| {
                let _ = ctx.rng()?.next_u32();
                Ok(())
            }),
        )
        .unwrap();
    }
    s.add_system(
        SystemDesc::new("sim.dice", Stage::Sim)
            .writes(roll)
            .rng("dice"),
        Box::new(|ctx| {
            let v = ctx.rng()?.below(6) + 1;
            let e = ctx.entity("@dice")?;
            ctx.update::<Roll, _>(e, |r| {
                r.last = v;
                r.sum += v as u64;
            })
        }),
    )
    .unwrap();
    (w, s)
}

fn state(w: &World) -> Vec<(String, Value)> {
    let e = w.entity("@dice").unwrap();
    w.get_component(e, w.component_id("Roll").unwrap()).unwrap()
}

#[test]
fn same_seed_same_run() {
    let (mut a, mut sa) = build(42, false);
    let (mut b, mut sb) = build(42, false);
    sa.step(&mut a, 500).unwrap();
    sb.step(&mut b, 500).unwrap();
    assert_eq!(state(&a), state(&b));
    let sum = match &state(&a)[1].1 {
        Value::UInt(s) => *s,
        v => panic!("{v:?}"),
    };
    assert!(sum > 500 && sum < 3000, "rolls look like d6: {sum}");
}

#[test]
fn different_seed_different_run() {
    let (mut a, mut sa) = build(1, false);
    let (mut b, mut sb) = build(2, false);
    sa.step(&mut a, 200).unwrap();
    sb.step(&mut b, 200).unwrap();
    assert_ne!(state(&a), state(&b));
}

#[test]
fn adding_another_random_system_does_not_perturb_a_stream() {
    let (mut a, mut sa) = build(7, false);
    let (mut b, mut sb) = build(7, true);
    sa.step(&mut a, 300).unwrap();
    sb.step(&mut b, 300).unwrap();
    assert_eq!(
        state(&a),
        state(&b),
        "dice stream is independent of the weather stream"
    );
}

#[test]
fn stream_seeds_are_stable_and_distinct() {
    assert_eq!(stream_seed(5, "dice"), stream_seed(5, "dice"));
    assert_ne!(stream_seed(5, "dice"), stream_seed(5, "weather"));
    assert_ne!(stream_seed(5, "dice"), stream_seed(6, "dice"));
    // Pinned so a future change to the hash is a deliberate, visible break.
    assert_eq!(stream_seed(0, ""), 6964505594983346210);
}

#[test]
fn pcg32_reference_and_helpers() {
    // First outputs of PCG32 for seed 42, stream 54 (from the reference implementation).
    let mut r = Pcg32::new(42, 54);
    assert_eq!(r.next_u32(), 0xa15c02b7);
    assert_eq!(r.next_u32(), 0x7b47f409);
    assert_eq!(r.next_u32(), 0xba1d3330);

    let mut r = Pcg32::new(1, 1);
    for _ in 0..1000 {
        let f = r.next_f32();
        assert!((0.0..1.0).contains(&f));
        let v = r.range(-3, 3);
        assert!((-3..=3).contains(&v));
        assert!(r.below(10) < 10);
    }
    assert_eq!(r.below(0), 0);
    assert_eq!(r.range(5, 1), 5);
    let mut items = [1, 2, 3, 4, 5];
    r.shuffle(&mut items);
    let mut sorted = items;
    sorted.sort();
    assert_eq!(sorted, [1, 2, 3, 4, 5]);
    assert!(r.pick(&items).is_some());
    assert!(r.pick::<u8>(&[]).is_none());
    let (st, inc) = r.state();
    let mut copy = Pcg32::from_state(st, inc);
    assert_eq!(copy.next_u32(), r.next_u32());
}

#[test]
fn undeclared_randomness_is_an_error_not_hidden_state() {
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::default());
    let code = Rc::new(RefCell::new(0));
    let c2 = code.clone();
    s.add_system(
        SystemDesc::new("sim.sneaky", Stage::Sim),
        Box::new(move |ctx| {
            *c2.borrow_mut() = ctx.rng().unwrap_err().code;
            Ok(())
        }),
    )
    .unwrap();
    s.step(&mut w, 1).unwrap();
    assert_eq!(*code.borrow(), sim_codes::NO_RNG_STREAM);
}

#[test]
fn sim_state_restores_mid_sequence_for_replay() {
    let (mut w, mut s) = build(9, true);
    s.step(&mut w, 100).unwrap();
    let snap = s.snapshot(&w);
    s.step(&mut w, 50).unwrap();
    let after = state(&w);
    let tick_after = s.tick();
    s.restore(&mut w, &snap);
    assert_eq!(s.tick(), 100);
    s.step(&mut w, 50).unwrap();
    assert_eq!(
        state(&w),
        after,
        "replaying from the snapshot reproduces the same rolls"
    );
    assert_eq!(s.tick(), tick_after);
}

#[test]
fn set_seed_reseeds_every_stream() {
    let (mut w, mut s) = build(1, false);
    s.step(&mut w, 20).unwrap();
    let first = state(&w);
    // Reset the world and reseed to the same seed: same result.
    let (mut w2, mut s2) = build(99, false);
    s2.set_seed(1);
    s2.step(&mut w2, 20).unwrap();
    assert_eq!(state(&w2), first);
    assert_eq!(s2.seed(), 1);
}
