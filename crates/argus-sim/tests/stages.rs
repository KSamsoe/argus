//! §3 "Systems and stages": fixed stage order, `after` ordering, declared
//! access enforced through the system context, `present` read-only.

use argus_ecs::{codes, Component, Vec2, World};
use argus_sim::{codes as sim_codes, ClockBinding, Scheduler, SchedulerConfig, Stage, SystemDesc};
use std::cell::RefCell;
use std::rc::Rc;

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

fn logger(log: &Rc<RefCell<Vec<String>>>, tag: &'static str) -> argus_sim::SystemFn {
    let log = log.clone();
    Box::new(move |_ctx| {
        log.borrow_mut().push(tag.to_string());
        Ok(())
    })
}

#[test]
fn stages_run_in_fixed_order_and_after_orders_within_a_stage() {
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    let log = Rc::new(RefCell::new(Vec::new()));
    // Register out of order on purpose.
    s.add_system(
        SystemDesc::new("present.ui", Stage::Present),
        logger(&log, "present.ui"),
    )
    .unwrap();
    s.add_system(
        SystemDesc::new("late.cleanup", Stage::Late),
        logger(&log, "late.cleanup"),
    )
    .unwrap();
    s.add_system(
        SystemDesc::new("sim.damage", Stage::Sim),
        logger(&log, "sim.damage"),
    )
    .unwrap();
    s.add_system(
        SystemDesc::new("sim.hitstop", Stage::Sim).after("sim.damage"),
        logger(&log, "sim.hitstop"),
    )
    .unwrap();
    s.add_system(
        SystemDesc::new("sim.move", Stage::Sim),
        logger(&log, "sim.move"),
    )
    .unwrap();
    s.add_system(
        SystemDesc::new("input.map", Stage::Input),
        logger(&log, "input.map"),
    )
    .unwrap();
    s.add_system(
        SystemDesc::new("hier.attach", Stage::Hierarchy),
        logger(&log, "hier.attach"),
    )
    .unwrap();
    // A second 'after' that points earlier in registration order.
    s.add_system(
        SystemDesc::new("sim.first", Stage::Sim),
        logger(&log, "sim.first"),
    )
    .unwrap();
    s.add_system(
        SystemDesc::new("sim.damage2", Stage::Sim)
            .after("sim.first")
            .after("sim.hitstop"),
        logger(&log, "sim.damage2"),
    )
    .unwrap();

    s.step(&mut w, 1).unwrap();
    s.present(&mut w).unwrap();
    let got = log.borrow().clone();
    assert_eq!(
        got,
        vec![
            "input.map",
            "sim.damage",
            "sim.hitstop",
            "sim.move",
            "sim.first",
            "sim.damage2",
            "late.cleanup",
            "hier.attach",
            "present.ui",
        ]
    );
    // The listing reports the same order.
    let names: Vec<String> = s
        .systems(w.registry())
        .unwrap()
        .into_iter()
        .map(|i| i.name)
        .collect();
    assert_eq!(names, got);
}

/// A `last` system (the physics step) runs after every other system of its
/// stage, whatever the registration order, unless one orders itself after
/// it; and a system sees the events emitted earlier in the same tick.
#[test]
fn a_last_system_waits_for_the_rest_of_its_stage_and_events_are_seen_the_same_tick() {
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    let log = Rc::new(RefCell::new(Vec::new()));
    s.add_system(SystemDesc::new("physics.move", Stage::Sim).last(), {
        let log = log.clone();
        Box::new(move |ctx| {
            log.borrow_mut().push("physics.move".to_string());
            ctx.emit("body.landed", vec![]);
            Ok(())
        })
    })
    .unwrap();
    s.add_system(
        SystemDesc::new("react", Stage::Sim).after("physics.move"),
        {
            let log = log.clone();
            Box::new(move |ctx| {
                let n = ctx.events_now("body.landed").count();
                let all = ctx.events_now("").count();
                log.borrow_mut().push(format!("react saw {n} of {all}"));
                Ok(())
            })
        },
    )
    .unwrap();
    s.add_system(
        SystemDesc::new("gameplay", Stage::Sim),
        logger(&log, "gameplay"),
    )
    .unwrap();
    s.add_system(SystemDesc::new("late.read", Stage::Late), {
        let log = log.clone();
        Box::new(move |ctx| {
            log.borrow_mut().push(format!(
                "late saw {}",
                ctx.events_now("body.landed").count()
            ));
            Ok(())
        })
    })
    .unwrap();
    s.step(&mut w, 2).unwrap();
    let got = log.borrow().clone();
    assert_eq!(
        got,
        vec![
            "gameplay",
            "physics.move",
            "react saw 1 of 1",
            "late saw 1",
            // The next tick sees only its own events, not the last tick's.
            "gameplay",
            "physics.move",
            "react saw 1 of 1",
            "late saw 1",
        ]
    );
    let listed: Vec<(String, bool)> = s
        .systems(w.registry())
        .unwrap()
        .into_iter()
        .map(|i| (i.name, i.last))
        .collect();
    assert_eq!(listed[1], ("physics.move".to_string(), true));
}

#[test]
fn ordering_errors_are_structured() {
    let mut s = Scheduler::new(SchedulerConfig::default());
    s.add_system(
        SystemDesc::new("a", Stage::Sim).after("b"),
        Box::new(|_| Ok(())),
    )
    .unwrap();
    s.add_system(
        SystemDesc::new("b", Stage::Sim).after("a"),
        Box::new(|_| Ok(())),
    )
    .unwrap();
    let err = s.validate().unwrap_err();
    assert_eq!(err.code, sim_codes::ORDER_CYCLE);
    assert!(err.message.contains("a") && err.message.contains("b"));

    let mut s = Scheduler::new(SchedulerConfig::default());
    s.add_system(
        SystemDesc::new("a", Stage::Sim).after("ghost"),
        Box::new(|_| Ok(())),
    )
    .unwrap();
    assert_eq!(s.validate().unwrap_err().code, sim_codes::UNKNOWN_SYSTEM);

    let mut s = Scheduler::new(SchedulerConfig::default());
    s.add_system(SystemDesc::new("a", Stage::Input), Box::new(|_| Ok(())))
        .unwrap();
    s.add_system(
        SystemDesc::new("b", Stage::Sim).after("a"),
        Box::new(|_| Ok(())),
    )
    .unwrap();
    let err = s.validate().unwrap_err();
    assert_eq!(err.code, sim_codes::ORDER_CYCLE);
    assert!(err.message.contains("stage"));

    let mut s = Scheduler::new(SchedulerConfig::default());
    s.add_system(SystemDesc::new("dup", Stage::Sim), Box::new(|_| Ok(())))
        .unwrap();
    let err = s
        .add_system(SystemDesc::new("dup", Stage::Sim), Box::new(|_| Ok(())))
        .unwrap_err();
    assert_eq!(err.code, sim_codes::DUPLICATE_SYSTEM);
}

#[test]
fn present_stage_is_read_only() {
    let mut w = World::new();
    let pos = w.register::<Pos>().unwrap();
    let mut s = Scheduler::new(SchedulerConfig::default());
    let err = s
        .add_system(
            SystemDesc::new("present.bad", Stage::Present).writes(pos),
            Box::new(|_| Ok(())),
        )
        .unwrap_err();
    assert_eq!(err.code, sim_codes::STAGE_READ_ONLY);
    // A read-only present system can query.
    let e = w.spawn();
    w.insert(e, Pos::default()).unwrap();
    s.add_system(
        SystemDesc::new("present.ok", Stage::Present).reads(pos),
        Box::new(|ctx| {
            let mut n = 0;
            ctx.query().each::<&Pos>(|_, _| n += 1)?;
            assert_eq!(n, 1);
            Ok(())
        }),
    )
    .unwrap();
    assert!(s.present(&mut w).unwrap());
}

#[test]
fn context_enforces_declared_access_for_queries_and_structural_changes() {
    let mut w = World::new();
    let pos = w.register::<Pos>().unwrap();
    let vel = w.register::<Vel>().unwrap();
    let e = w.spawn();
    w.insert(e, Pos::default()).unwrap();
    w.insert(
        e,
        Vel {
            v: Vec2::new(1.0, 0.0),
        },
    )
    .unwrap();

    let mut s = Scheduler::new(SchedulerConfig::default());
    let seen = Rc::new(RefCell::new(Vec::new()));
    let seen2 = seen.clone();
    s.add_system(
        SystemDesc::new("sim.move", Stage::Sim)
            .reads(vel)
            .writes(pos),
        Box::new(move |ctx| {
            // Allowed.
            ctx.query()
                .each::<(&Vel, &mut Pos)>(|_, (v, p)| p.v.x += v.v.x)?;
            let e = ctx.entity("e:0")?;
            ctx.set_field(e, "Pos.v", Vec2::new(5.0, 5.0))?;
            let spawned = ctx.spawn();
            ctx.insert(spawned, Pos::default())?;
            // Not allowed: writing Vel, inserting Vel.
            seen2
                .borrow_mut()
                .push(ctx.query().each::<&mut Vel>(|_, _| {}).unwrap_err().code);
            seen2
                .borrow_mut()
                .push(ctx.insert(spawned, Vel::default()).unwrap_err().code);
            seen2
                .borrow_mut()
                .push(ctx.set_field(e, "Vel.v", Vec2::ZERO).unwrap_err().code);
            seen2
                .borrow_mut()
                .push(ctx.update::<Vel, _>(e, |_| {}).unwrap_err().code);
            Ok(())
        }),
    )
    .unwrap();
    s.step(&mut w, 1).unwrap();
    assert_eq!(*seen.borrow(), vec![codes::ACCESS_VIOLATION; 4]);
    assert_eq!(w.get::<Pos>(e).unwrap().v, Vec2::new(5.0, 5.0));
    assert_eq!(w.query().with::<Pos>().count(), 2);
    assert!(
        s.is_enabled("sim.move").unwrap(),
        "handled errors do not disable the system"
    );
}

#[test]
fn who_writes_answers_in_stage_order() {
    let mut w = World::new();
    let pos = w.register::<Pos>().unwrap();
    let vel = w.register::<Vel>().unwrap();
    let mut s = Scheduler::new(SchedulerConfig::default());
    s.add_system(
        SystemDesc::new("late.clamp", Stage::Late).writes(pos),
        Box::new(|_| Ok(())),
    )
    .unwrap();
    s.add_system(
        SystemDesc::new("sim.move", Stage::Sim)
            .reads(vel)
            .writes(pos),
        Box::new(|_| Ok(())),
    )
    .unwrap();
    s.add_system(
        SystemDesc::new("sim.ai", Stage::Sim).writes(vel),
        Box::new(|_| Ok(())),
    )
    .unwrap();
    s.add_system(
        SystemDesc::new("present.cam", Stage::Present).reads(pos),
        Box::new(|_| Ok(())),
    )
    .unwrap();
    assert_eq!(s.who_writes(pos).unwrap(), vec!["sim.move", "late.clamp"]);
    assert_eq!(s.who_writes(vel).unwrap(), vec!["sim.ai"]);
    assert_eq!(
        s.who_reads(pos).unwrap(),
        vec!["sim.move", "late.clamp", "present.cam"]
    );
    let info = s.systems(w.registry()).unwrap();
    let mv = info.iter().find(|i| i.name == "sim.move").unwrap();
    assert_eq!(mv.reads, vec!["Vel"]);
    assert_eq!(mv.writes, vec!["Pos"]);
    assert_eq!(mv.clock, ClockBinding::Driving);
}

#[test]
fn hierarchy_propagates_every_tick_and_maintenance_runs_at_frame_end() {
    use argus_ecs::{GlobalTransform, Parent, Transform};
    let mut w = World::new();
    argus_ecs::hierarchy::register(&mut w).unwrap();
    let root = w.spawn_authored("root").unwrap();
    w.insert(root, Transform::at(10.0, 0.0)).unwrap();
    w.insert(root, GlobalTransform::default()).unwrap();
    let child = w.spawn_authored("child").unwrap();
    w.insert(child, Transform::at(1.0, 0.0)).unwrap();
    w.insert(child, GlobalTransform::default()).unwrap();
    w.insert(child, Parent { entity: root }).unwrap();

    let mut s = Scheduler::new(SchedulerConfig::default());
    let tf = w.component_id_of::<Transform>().unwrap();
    s.add_system(
        SystemDesc::new("sim.drift", Stage::Sim).writes(tf),
        Box::new(|ctx| {
            let root = ctx.entity("@root")?;
            ctx.update::<Transform, _>(root, |t| t.pos.x += 1.0)
        }),
    )
    .unwrap();
    s.step(&mut w, 3).unwrap();
    assert_eq!(w.get::<GlobalTransform>(child).unwrap().pos.x, 14.0);
    assert_eq!(w.frame(), 3, "end_frame ran once per tick");
    assert_eq!(s.tick(), 3);
}
