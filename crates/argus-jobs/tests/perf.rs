//! Jobs at Outpost's scale (§17: "Outpost's sim tick at 10x, 200
//! colonists, 20k items, under 1.6 ms ... with pathfinding, jobs, and needs
//! all running"). This is the engine's share, the job system and the
//! agents it walks; Outpost's own tick is measured on the game. Run in
//! release: `cargo test --release -p argus-jobs --test perf -- --ignored
//! --nocapture`.

use argus_ecs::{Entity, Transform, Vec2, World};
use argus_jobs::{Job, Worker};
use argus_nav::{NavAgent, NavCost, NavGrid, NavStore};
use argus_sim::{Pcg32, Scheduler, SchedulerConfig};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

#[test]
#[ignore]
fn perf_200_workers_hauling_among_20000_items() {
    let size = 96u32;
    let mut w = World::new();
    w.register::<Transform>().unwrap();
    let mut s = Scheduler::new(SchedulerConfig::default());
    let nav = Rc::new(RefCell::new(NavStore::new(&std::env::temp_dir(), None)));
    argus_nav::install(&mut w, &mut s, nav.clone()).unwrap();
    argus_jobs::install(&mut w, &mut s, nav.clone()).unwrap();
    let g = w.spawn();
    w.insert(
        g,
        NavGrid {
            origin: Vec2::new(-0.5, -0.5),
            width: size,
            height: size,
            diagonal: true,
            ..NavGrid::default()
        },
    )
    .unwrap();
    let mut rng = Pcg32::new(5, 1);
    let at = |rng: &mut Pcg32| {
        Vec2::new(
            (rng.next_f32() * (size - 1) as f32).floor(),
            (rng.next_f32() * (size - 1) as f32).floor(),
        )
    };
    // Buildings: walls here and there (a twentieth of the ground).
    for _ in 0..(size * size / 20) {
        let e = w.spawn();
        let p = at(&mut rng);
        w.insert(e, Transform::at(p.x, p.y)).unwrap();
        w.insert(e, NavCost::default()).unwrap();
    }
    // Twenty thousand items lying about or stored.
    let mut items = Vec::new();
    for _ in 0..20_000 {
        let e = w.spawn();
        let p = at(&mut rng);
        w.insert(e, Transform::at(p.x, p.y)).unwrap();
        items.push(e);
    }
    let pile = w.spawn();
    w.insert(pile, Transform::at(48.0, 48.0)).unwrap();
    let (haul, farm) = (w.strings().intern("haul"), w.strings().intern("farm haul"));
    for i in 0..200 {
        let e = w.spawn();
        let p = at(&mut rng);
        w.insert(e, Transform::at(p.x, p.y)).unwrap();
        w.insert(
            e,
            Worker {
                skills: farm,
                rate: 1.0 + (i % 3) as f32 * 0.25,
                ..Worker::default()
            },
        )
        .unwrap();
        w.insert(
            e,
            NavAgent {
                goal: p,
                speed: 4.0,
                ..NavAgent::default()
            },
        )
        .unwrap();
    }
    // Keep four hundred haul jobs posted, as a colony's game systems would.
    let jid = w.component_id_of::<Job>().unwrap();
    let mut next = 0usize;
    let mut post = |w: &mut World, rng: &mut Pcg32| {
        let open = w.query().has(jid).entities().len();
        for _ in open..400 {
            let item = items[next % items.len()];
            next += 1;
            let e = w.spawn();
            w.insert(
                e,
                Job {
                    kind: haul,
                    priority: (rng.next_f32() * 3.0).floor(),
                    site: item,
                    to: pile,
                    carry: item,
                    work: 0.5,
                    ..Job::default()
                },
            )
            .unwrap();
        }
    };
    // Settle: the first claims and paths.
    let t = Instant::now();
    post(&mut w, &mut rng);
    s.step(&mut w, 1).unwrap();
    let first = t.elapsed().as_secs_f64() * 1000.0;
    for _ in 0..299 {
        post(&mut w, &mut rng);
        s.step(&mut w, 1).unwrap();
        w.drain_events();
    }
    // Measure: 1,200 ticks, jobs finishing and posted all along.
    let mut done = 0usize;
    let mut worst = 0.0f64;
    let t = Instant::now();
    for _ in 0..1200 {
        post(&mut w, &mut rng);
        let t1 = Instant::now();
        s.step(&mut w, 1).unwrap();
        worst = worst.max(t1.elapsed().as_secs_f64() * 1000.0);
        done += w
            .drain_events()
            .iter()
            .filter(
                |r| matches!(&r.event, argus_ecs::Event::Game { name, .. } if name == "job.done"),
            )
            .count();
    }
    let per = t.elapsed().as_secs_f64() * 1000.0 / 1200.0;
    let busy = w
        .query()
        .has(w.component_id_of::<Worker>().unwrap())
        .entities()
        .iter()
        .filter(|&&e| !w.get::<Worker>(e).unwrap().job.is_null())
        .count();
    let c = nav.borrow().counts;
    println!(
        "200 workers, 20,000 items, 400 open jobs: first tick {first:.2} ms; {per:.3} ms a tick (worst {worst:.2}), posting included; {done} jobs done in 1,200 ticks; {busy} busy at the end; {} searches",
        c.searches
    );
    assert!(done > 100, "{done}");
    assert!(per < 1.0, "{per} ms a tick");
    let _: Entity = pile;
}
