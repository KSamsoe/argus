//! Phase 1 item 13: every event a system emits says which system (spawns
//! and despawns too), and observers read the world at the end of every
//! tick, with the frame the tick's events carry.

use argus_ecs::{Event, Transform, World};
use argus_sim::{Scheduler, SchedulerConfig, Stage, SystemDesc};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn events_say_who_emitted_them_and_observers_see_each_tick() {
    let mut w = World::new();
    argus_ecs::hierarchy::register(&mut w).unwrap();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0).seed(1));
    let tr = w.component_id_of::<Transform>().unwrap();
    s.add_system(
        SystemDesc::new("sim.maker", Stage::Sim).writes(tr),
        Box::new(|ctx| {
            if ctx.tick == 1 {
                let e = ctx.spawn();
                ctx.insert(e, Transform::at(1.0, 0.0))?;
                ctx.emit("made", vec![]);
            }
            Ok(())
        }),
    )
    .unwrap();
    let seen: Rc<RefCell<Vec<(u64, usize)>>> = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    s.observe(Box::new(move |world, frame| {
        log.borrow_mut().push((frame, world.entity_count()))
    }));
    // Outside a system, nobody.
    w.emit(Event::Game {
        name: "outside".into(),
        fields: vec![],
    });
    s.step(&mut w, 3).unwrap();
    let events = w.drain_events();
    let by = |n: &str| {
        events
            .iter()
            .find(|r| match &r.event {
                Event::Game { name, .. } => name == n,
                Event::Spawned { .. } => n == "spawned",
                _ => false,
            })
            .map(|r| (r.frame, r.by.as_deref().map(str::to_string)))
    };
    assert_eq!(by("outside"), Some((0, None)));
    assert_eq!(by("made"), Some((1, Some("sim.maker".into()))));
    assert_eq!(
        by("spawned"),
        Some((1, Some("sim.maker".into()))),
        "the spawn inside it too"
    );
    // The observer saw every tick, with the frame its events carry, after
    // what the tick did.
    assert_eq!(*seen.borrow(), vec![(0, 0), (1, 1), (2, 1)]);
}
