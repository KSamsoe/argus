//! §6 "Two clocks": continuous fixed-rate with speed, discrete on command,
//! both behind one model; mixed clocks in one scene; play/pause/stop
//! snapshot semantics (§2); sim time never wall-clock.

use argus_ecs::{codes, Component, World};
use argus_sim::{
    codes as sim_codes, Clock, ClockBinding, ClockKind, Scheduler, SchedulerConfig, Stage,
    SystemDesc,
};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
struct Counter {
    n: u32,
}

fn counting(
    world: &mut World,
    s: &mut Scheduler,
    name: &str,
    clock: ClockBinding,
    id: &str,
) -> Rc<RefCell<u32>> {
    let c = world.register::<Counter>().unwrap();
    let e = world.spawn_authored(id).unwrap();
    world.insert(e, Counter::default()).unwrap();
    let ticks = Rc::new(RefCell::new(0u32));
    let t = ticks.clone();
    let id = id.to_string();
    s.add_system(
        SystemDesc::new(name, Stage::Sim).writes(c).clock(clock),
        Box::new(move |ctx| {
            *t.borrow_mut() += 1;
            let e = ctx.entity(&id)?;
            ctx.update::<Counter, _>(e, |c| c.n += 1)
        }),
    )
    .unwrap();
    ticks
}

#[test]
fn continuous_clock_runs_speed_ticks_per_frame() {
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    let ticks = counting(&mut w, &mut s, "sim.count", ClockBinding::Driving, "c");

    // Edit mode: frames do not tick.
    let r = s.frame(&mut w).unwrap();
    assert_eq!(r.sim_ticks, 0);
    assert_eq!(*ticks.borrow(), 0);

    s.play(&w);
    assert_eq!(s.frame(&mut w).unwrap().sim_ticks, 1);
    s.set_speed(3.0).unwrap();
    assert_eq!(s.frame(&mut w).unwrap().sim_ticks, 3);
    s.set_speed(0.5).unwrap();
    let a = s.frame(&mut w).unwrap().sim_ticks;
    let b = s.frame(&mut w).unwrap().sim_ticks;
    assert_eq!(a + b, 1, "fractional speed accumulates");
    s.set_speed(0.0).unwrap();
    assert_eq!(s.frame(&mut w).unwrap().sim_ticks, 0, "speed 0 is pause");
    assert_eq!(*ticks.borrow(), 5);
    assert_eq!(s.tick(), 5);
    assert!(
        (s.time() - 5.0 / 60.0).abs() < 1e-12,
        "time is tick × timestep"
    );

    // Pause stops frames; step still works and ignores pause.
    s.set_speed(1.0).unwrap();
    s.pause();
    assert_eq!(s.frame(&mut w).unwrap().sim_ticks, 0);
    s.step(&mut w, 2).unwrap();
    assert_eq!(*ticks.borrow(), 7);
    assert!(s.is_paused());
    s.play(&w);
    assert!(!s.is_paused());
    assert_eq!(s.frame(&mut w).unwrap().sim_ticks, 1);

    assert_eq!(s.set_speed(-1.0).unwrap_err().code, codes::OUT_OF_RANGE);
    assert_eq!(s.set_speed(f64::NAN).unwrap_err().code, codes::OUT_OF_RANGE);
}

#[test]
fn a_speed_a_system_asks_for_takes_effect_at_the_end_of_its_tick() {
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let v = seen.clone();
    s.add_system(
        SystemDesc::new("sim.pace", Stage::Sim),
        Box::new(move |ctx| {
            v.borrow_mut().push(ctx.speed);
            if ctx.tick == 1 {
                ctx.emit(
                    "speed.requested",
                    vec![("x".to_string(), argus_ecs::Value::Float(4.0))],
                );
            }
            Ok(())
        }),
    )
    .unwrap();
    s.play(&w);
    assert_eq!(s.frame(&mut w).unwrap().sim_ticks, 1);
    // Tick 1 asks for 4x: the frame keeps the count it began with, the
    // clock is at 4x after the tick, and the next frame runs four.
    assert_eq!(s.frame(&mut w).unwrap().sim_ticks, 1);
    assert_eq!(s.speed(), 4.0);
    assert_eq!(s.frame(&mut w).unwrap().sim_ticks, 4);
    // What the system saw, tick by tick (a game's q:speed()).
    assert_eq!(*seen.borrow(), vec![1.0, 1.0, 4.0, 4.0, 4.0, 4.0]);
    // Headless, the same: the request is the run's, not the host's.
    let mut w2 = World::new();
    let mut s2 = Scheduler::new(SchedulerConfig::continuous(60.0));
    s2.add_system(
        SystemDesc::new("sim.pace", Stage::Sim),
        Box::new(|ctx| {
            if ctx.tick == 1 {
                ctx.emit(
                    "speed.requested",
                    vec![("x".to_string(), argus_ecs::Value::Float(4.0))],
                );
            }
            Ok(())
        }),
    )
    .unwrap();
    s2.step(&mut w2, 2).unwrap();
    assert_eq!(s2.speed(), 4.0);
}

#[test]
fn discrete_clock_only_advances_on_command() {
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::discrete());
    let ticks = counting(&mut w, &mut s, "sim.turn", ClockBinding::Driving, "d");
    let presented = Rc::new(RefCell::new(0));
    let p = presented.clone();
    s.add_system(
        SystemDesc::new("present.anim", Stage::Present),
        Box::new(move |_| {
            *p.borrow_mut() += 1;
            Ok(())
        }),
    )
    .unwrap();

    s.play(&w);
    for _ in 0..10 {
        let r = s.frame(&mut w).unwrap();
        assert_eq!(r.sim_ticks, 0, "frames never tick a discrete sim");
        assert!(r.presented);
    }
    assert_eq!(*ticks.borrow(), 0);
    assert_eq!(*presented.borrow(), 10, "present still runs between turns");

    s.advance(&mut w, 1).unwrap();
    assert_eq!(*ticks.borrow(), 1);
    s.step(&mut w, 2).unwrap();
    assert_eq!(*ticks.borrow(), 3);
    assert_eq!(s.tick(), 3);
    assert_eq!(s.time(), 3.0, "one turn is one time unit");
    assert_eq!(s.set_speed(2.0).unwrap_err().code, sim_codes::WRONG_CLOCK);
    let e = w.entity("@d").unwrap();
    assert_eq!(w.get::<Counter>(e).unwrap().n, 3);
}

#[test]
fn a_scene_can_mix_clocks() {
    // Tactics-shaped: discrete drives sim, an animation system binds to continuous.
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::discrete());
    let turns = counting(&mut w, &mut s, "sim.turn", ClockBinding::Driving, "t");
    let anim = counting(&mut w, &mut s, "sim.anim", ClockBinding::Continuous, "a");
    s.play(&w);
    // Discrete-driven scenes never tick on frames, so continuous-bound
    // systems run only when stepped on their own clock.
    s.frame(&mut w).unwrap();
    assert_eq!((*turns.borrow(), *anim.borrow()), (0, 0));
    s.advance(&mut w, 2).unwrap();
    assert_eq!((*turns.borrow(), *anim.borrow()), (2, 0));

    // Continuous-driven scene hosting a discrete subsystem: advance still works.
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    let cont = counting(&mut w, &mut s, "sim.cont", ClockBinding::Driving, "c");
    let disc = counting(&mut w, &mut s, "sim.disc", ClockBinding::Discrete, "d");
    s.play(&w);
    s.frame(&mut w).unwrap();
    s.frame(&mut w).unwrap();
    assert_eq!((*cont.borrow(), *disc.borrow()), (2, 0));
    s.advance(&mut w, 1).unwrap();
    assert_eq!((*cont.borrow(), *disc.borrow()), (2, 1));
    assert_eq!(s.clock(ClockKind::Discrete).tick, 1);
    assert_eq!(s.clock(ClockKind::Continuous).tick, 2);
}

#[test]
fn play_snapshots_and_stop_restores() {
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    counting(&mut w, &mut s, "sim.count", ClockBinding::Driving, "c");
    let e = w.entity("@c").unwrap();
    assert!(!s.is_playing());
    s.play(&w);
    assert!(s.is_playing());
    for _ in 0..5 {
        s.frame(&mut w).unwrap();
    }
    let spawned = w.spawn();
    assert_eq!(w.get::<Counter>(e).unwrap().n, 5);
    assert_eq!(s.tick(), 5);
    s.stop(&mut w);
    assert!(!s.is_playing());
    assert_eq!(
        w.get::<Counter>(e).unwrap().n,
        0,
        "world back to edit state"
    );
    assert!(!w.is_alive(spawned));
    assert_eq!(s.tick(), 0, "clock back too");
    // Frames in edit mode do nothing to the sim.
    s.frame(&mut w).unwrap();
    assert_eq!(w.get::<Counter>(e).unwrap().n, 0);
    // Playing again takes a fresh snapshot; a second play while playing does not.
    s.play(&w);
    s.frame(&mut w).unwrap();
    s.play(&w);
    s.stop(&mut w);
    assert_eq!(w.get::<Counter>(e).unwrap().n, 0);
}

#[test]
fn run_until_variants() {
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    let c = w.register::<Counter>().unwrap();
    let e = w.spawn_authored("c").unwrap();
    w.insert(e, Counter::default()).unwrap();
    s.add_system(
        SystemDesc::new("sim.count", Stage::Sim).writes(c),
        Box::new(|ctx| {
            let e = ctx.entity("@c")?;
            let n = ctx.update::<Counter, _>(e, |c| {
                c.n += 1;
                c.n
            })?;
            if n == 7 {
                ctx.emit("wave.cleared", vec![("wave".into(), 1i64.into())]);
            }
            Ok(())
        }),
    )
    .unwrap();

    let r = s
        .run_until(&mut w, 1000, |w| {
            w.get::<Counter>(w.entity("@c").unwrap()).unwrap().n >= 3
        })
        .unwrap();
    assert_eq!(
        r,
        argus_sim::RunUntil {
            ticks: 3,
            hit: true
        }
    );

    let r = s.run_until_event(&mut w, "wave.cleared", 1000).unwrap();
    assert_eq!(
        r,
        argus_sim::RunUntil {
            ticks: 4,
            hit: true
        }
    );
    assert_eq!(s.tick(), 7);

    let r = s.run_until_event(&mut w, "never", 10).unwrap();
    assert_eq!(
        r,
        argus_sim::RunUntil {
            ticks: 10,
            hit: false
        }
    );

    let r = s.run_until_time(&mut w, 1.0, 10_000).unwrap();
    assert!(r.hit);
    assert_eq!(s.tick(), 60);
    assert!((s.time() - 1.0).abs() < 1e-9);
}

#[test]
fn clock_specs_round_trip() {
    let c = Clock::parse("continuous@60").unwrap();
    assert_eq!(c.kind, ClockKind::Continuous);
    assert!((c.hz() - 60.0).abs() < 1e-9);
    assert_eq!(c.to_string(), "continuous@60");
    assert_eq!(Clock::parse("discrete").unwrap().to_string(), "discrete");
    assert_eq!(
        Clock::parse("continuous@0").unwrap_err().code,
        sim_codes::BAD_CLOCK_SPEC
    );
    assert_eq!(
        Clock::parse("hourly").unwrap_err().code,
        sim_codes::BAD_CLOCK_SPEC
    );
}

#[test]
fn context_time_is_sim_time() {
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::continuous(10.0));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let s2 = seen.clone();
    s.add_system(
        SystemDesc::new("sim.t", Stage::Sim),
        Box::new(move |ctx| {
            s2.borrow_mut()
                .push((ctx.tick, ctx.time, ctx.dt, ctx.clock));
            Ok(())
        }),
    )
    .unwrap();
    s.step(&mut w, 3).unwrap();
    let v = seen.borrow();
    assert_eq!(v[0], (0, 0.0, 0.1, ClockKind::Continuous));
    assert_eq!(v[2].0, 2);
    assert!((v[2].1 - 0.2).abs() < 1e-12);
}

#[test]
fn a_frozen_tick_takes_no_turn_and_a_seek_keeps_the_turns_true() {
    // Issue 36: the wheel's, a typed character's, or a menu's tick is
    // frozen in the input stage and takes no turn, though the tick count
    // moves. The scheduler keeps the latest turns' ticks, as the timeline
    // stands after a seek.
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::discrete());
    let frozen = Rc::new(RefCell::new(vec![3u64, 4, 5, 6]));
    let f = frozen.clone();
    s.add_system(
        SystemDesc::new("input.actions", Stage::Input),
        Box::new(move |ctx| {
            if f.borrow().contains(&ctx.tick) {
                ctx.freeze();
            }
            Ok(())
        }),
    )
    .unwrap();
    s.advance(&mut w, 5).unwrap();
    let snap = s.snapshot(&w);
    s.advance(&mut w, 5).unwrap();
    assert_eq!(s.tick(), 10);
    // Ticks 0, 1, 2, 7, 8, 9 were turns.
    assert_eq!(s.turns_back(1), Some(9));
    assert_eq!(s.turns_back(4), Some(2));
    assert_eq!(s.turns_back(6), Some(0));
    assert_eq!(s.turns_back(7), None);
    assert_eq!(s.turns_since(3), 3);

    // Back to tick 5 and on again with nothing frozen: 5 to 9 are turns now.
    s.restore(&mut w, &snap);
    frozen.borrow_mut().clear();
    assert_eq!(s.turns_since(0), 3, "only what came before tick 5");
    s.advance(&mut w, 5).unwrap();
    assert_eq!(s.turns_since(3), 5);
    assert_eq!(s.turns_back(6), Some(2));
    // A continuous clock has no turns.
    assert_eq!(
        Scheduler::new(SchedulerConfig::default()).turns_back(1),
        None
    );
}
