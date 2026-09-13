//! §2 "Isolation": a system fault must not take the daemon down. A panic or
//! error is caught at the system boundary, the system is disabled, the fault
//! is an event, and the world remains intact and inspectable.

use argus_ecs::{Component, Event, World};
use argus_sim::{Scheduler, SchedulerConfig, Stage, SystemDesc};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
struct Hp {
    cur: i32,
}

fn quiet_panics() {
    // Keep the default hook from printing the deliberate panics below.
    std::panic::set_hook(Box::new(|_| {}));
}

#[test]
fn a_panicking_system_is_disabled_and_reported_and_others_keep_running() {
    quiet_panics();
    let mut w = World::new();
    let hp = w.register::<Hp>().unwrap();
    let e = w.spawn_authored("p").unwrap();
    w.insert(e, Hp { cur: 10 }).unwrap();
    let mut s = Scheduler::new(SchedulerConfig::default().seed(1));
    let ran = Rc::new(RefCell::new(0));
    let r2 = ran.clone();
    s.add_system(
        SystemDesc::new("sim.boom", Stage::Sim)
            .writes(hp)
            .source("systems/boom.luau:3"),
        Box::new(|ctx| {
            let e = ctx.entity("@p")?;
            ctx.update::<Hp, _>(e, |h| h.cur -= 1)?;
            panic!("index out of bounds: the len is 0 but the index is 4");
        }),
    )
    .unwrap();
    s.add_system(
        SystemDesc::new("sim.after", Stage::Sim).after("sim.boom"),
        Box::new(move |_| {
            *r2.borrow_mut() += 1;
            Ok(())
        }),
    )
    .unwrap();

    s.step(&mut w, 3).unwrap();
    assert_eq!(*ran.borrow(), 3, "the later system ran every tick");
    assert!(!s.is_enabled("sim.boom").unwrap());
    assert_eq!(
        w.get::<Hp>(e).unwrap().cur,
        9,
        "world intact: the partial write before the panic stands"
    );

    let faults: Vec<&Event> = w
        .events()
        .iter()
        .map(|r| &r.event)
        .filter(|e| matches!(e, Event::SystemFault { .. }))
        .collect();
    assert_eq!(faults.len(), 1, "disabled after the first fault");
    match faults[0] {
        Event::SystemFault {
            system,
            message,
            r#ref,
        } => {
            assert_eq!(system, "sim.boom");
            assert!(message.contains("index out of bounds"), "{message}");
            assert_eq!(r#ref.as_deref(), Some("systems/boom.luau:3"));
        }
        _ => unreachable!(),
    }
    assert!(w
        .events()
        .iter()
        .any(|r| matches!(&r.event, Event::SystemDisabled { system, .. } if system == "sim.boom")));
    let info = s.systems(w.registry()).unwrap();
    let boom = info.iter().find(|i| i.name == "sim.boom").unwrap();
    assert!(!boom.enabled);
    assert!(boom.disabled_reason.as_deref().unwrap().contains("panic"));

    // Re-enable: it runs (and faults) again.
    s.enable("sim.boom").unwrap();
    s.step(&mut w, 1).unwrap();
    assert_eq!(w.get::<Hp>(e).unwrap().cur, 8);
    assert!(!s.is_enabled("sim.boom").unwrap());
}

#[test]
fn an_erroring_system_is_disabled_with_the_structured_error() {
    let mut w = World::new();
    let hp = w.register::<Hp>().unwrap();
    let e = w.spawn_authored("p").unwrap();
    w.insert(e, Hp::default()).unwrap();
    let mut s = Scheduler::new(SchedulerConfig::default());
    s.add_system(
        SystemDesc::new("sim.bad", Stage::Sim).reads(hp),
        // Declared read only; the write is an access violation returned as Err.
        Box::new(|ctx| {
            let e = ctx.entity("@p")?;
            ctx.set_field(e, "Hp.cur", 5i64)
        }),
    )
    .unwrap();
    s.step(&mut w, 2).unwrap();
    assert!(!s.is_enabled("sim.bad").unwrap());
    assert_eq!(w.get::<Hp>(e).unwrap().cur, 0);
    let fault = w.events().iter().find_map(|r| match &r.event {
        Event::SystemFault { message, .. } => Some(message.clone()),
        _ => None,
    });
    assert!(
        fault.unwrap().contains("4301"),
        "message carries the error code"
    );
}

#[test]
fn disable_and_remove_by_request() {
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::default());
    let n = Rc::new(RefCell::new(0));
    let n2 = n.clone();
    s.add_system(
        SystemDesc::new("sim.a", Stage::Sim),
        Box::new(move |_| {
            *n2.borrow_mut() += 1;
            Ok(())
        }),
    )
    .unwrap();
    s.step(&mut w, 1).unwrap();
    s.disable("sim.a", "hot reload in progress").unwrap();
    s.step(&mut w, 1).unwrap();
    assert_eq!(*n.borrow(), 1);
    s.enable("sim.a").unwrap();
    s.step(&mut w, 1).unwrap();
    assert_eq!(*n.borrow(), 2);
    s.remove_system("sim.a").unwrap();
    assert_eq!(
        s.enable("sim.a").unwrap_err().code,
        argus_sim::codes::UNKNOWN_SYSTEM
    );
    s.step(&mut w, 1).unwrap();
    assert_eq!(*n.borrow(), 2);
}
