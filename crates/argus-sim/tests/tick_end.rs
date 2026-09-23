//! Phase 2 item 0, what the keyframe ring needs from the scheduler: a hook
//! when a tick has fully ended, quiet ticks for a seek's replay, and which
//! systems are switched off as part of the sim state.

use argus_ecs::World;
use argus_sim::{Scheduler, SchedulerConfig, Stage, SystemDesc};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn the_tick_end_hook_sees_every_counted_tick_and_quiet_ticks_are_unseen() {
    let mut w = World::new();
    argus_ecs::hierarchy::register(&mut w).unwrap();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0).seed(1));
    let ran = Rc::new(RefCell::new(0u32));
    let r = ran.clone();
    s.add_system(
        SystemDesc::new("sim.count", Stage::Sim),
        Box::new(move |_| {
            *r.borrow_mut() += 1;
            Ok(())
        }),
    )
    .unwrap();
    let ends: Rc<RefCell<Vec<(u64, u64)>>> = Rc::new(RefCell::new(Vec::new()));
    let e = ends.clone();
    s.on_tick_end(Box::new(move |world, sched| {
        e.borrow_mut().push((sched.tick(), world.frame()));
    }));
    let seen = Rc::new(RefCell::new(0u32));
    let o = seen.clone();
    s.observe(Box::new(move |_, _| *o.borrow_mut() += 1));

    s.step(&mut w, 3).unwrap();
    assert_eq!(
        *ends.borrow(),
        vec![(1, 1), (2, 2), (3, 3)],
        "after each tick, the clock and the frame already count it"
    );
    let profiled = s.prof_systems(600).unwrap()[0].samples;

    s.set_quiet(true);
    s.step(&mut w, 2).unwrap();
    s.set_quiet(false);
    assert_eq!(*ran.borrow(), 5, "quiet ticks still run the systems");
    assert_eq!(ends.borrow().len(), 3, "but not the hook");
    assert_eq!(*seen.borrow(), 3, "or the observers");
    assert_eq!(
        s.prof_systems(600).unwrap()[0].samples,
        profiled,
        "or the profiler"
    );
    assert_eq!(s.tick(), 5);
}

#[test]
fn which_systems_are_off_is_part_of_the_sim_state() {
    let mut w = World::new();
    argus_ecs::hierarchy::register(&mut w).unwrap();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0).seed(1));
    s.add_system(
        SystemDesc::new("sim.fragile", Stage::Sim),
        Box::new(|ctx| {
            if ctx.tick == 3 {
                return Err(argus_ecs::ArgusError::new(1, "broke"));
            }
            Ok(())
        }),
    )
    .unwrap();
    s.step(&mut w, 2).unwrap();
    let before = s.snapshot(&w);
    s.step(&mut w, 3).unwrap();
    assert!(
        !s.is_enabled("sim.fragile").unwrap(),
        "the fault switched it off"
    );
    s.restore(&mut w, &before);
    assert!(
        s.is_enabled("sim.fragile").unwrap(),
        "at tick 2 it was still on"
    );
    s.step(&mut w, 2).unwrap();
    assert!(
        !s.is_enabled("sim.fragile").unwrap(),
        "and it faults again at 3"
    );
}
