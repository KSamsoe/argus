//! Phase 2 item 0: the keyframe ring's shape (§6 thinning, stretched to
//! meet §17's seek target) and its byte budget.

use argus_daemon::ring::{Ring, Sources, DENSE_SECONDS, HOUR};
use argus_ecs::World;
use argus_particles::ParticleStore;
use argus_sim::{Scheduler, SchedulerConfig};
use std::cell::RefCell;
use std::rc::Rc;

/// A keyframe of a small world, restamped at `second` of sim time.
fn keyframe_at(
    base: &argus_daemon::ring::Keyframe,
    second: u64,
    barrier: bool,
) -> argus_daemon::ring::Keyframe {
    let mut k = base.clone();
    k.tick = second * 60;
    k.time = second as f64;
    k.barrier = barrier;
    k
}

fn base() -> argus_daemon::ring::Keyframe {
    let mut w = World::new();
    argus_ecs::hierarchy::register(&mut w).unwrap();
    for _ in 0..50 {
        w.spawn();
    }
    let s = Scheduler::new(SchedulerConfig::continuous(60.0).seed(1));
    let src = Sources {
        input: None,
        particles: Rc::new(RefCell::new(ParticleStore::default())),
        scene: None,
        contacts: Rc::new(RefCell::new(argus_physics::Contacts::default())),
    };
    src.capture(&w, &s, false)
}

#[test]
fn two_hours_of_keyframes_thin_by_age() {
    let b = base();
    let mut r = Ring::new(60, usize::MAX);
    r.push(keyframe_at(&b, 0, true)); // the load
    for s in 1..=7200 {
        r.push(keyframe_at(&b, s, false));
    }
    let ticks = r.ticks();
    let ages: Vec<f64> = ticks.iter().map(|t| 7200.0 - *t as f64 / 60.0).collect();
    let dense = ages.iter().filter(|a| **a <= DENSE_SECONDS).count();
    let hourly = ages
        .iter()
        .filter(|a| **a > DENSE_SECONDS && **a <= HOUR)
        .count();
    let old = ages.iter().filter(|a| **a > HOUR).count();
    assert_eq!(dense, 301, "every second of the last five minutes");
    assert!(
        (54..=56).contains(&hourly),
        "one a minute to an hour: {hourly}"
    );
    assert!((6..=7).contains(&old), "one per ten minutes beyond: {old}");
    assert_eq!(ticks[0], 0, "the load's keyframe stays");
    // The replay any seek needs is bounded by its band.
    let gap = |t: u64| t - r.at_or_before(t).unwrap().tick;
    for s in (7200 - 300)..=7200 {
        assert!(
            gap(s * 60 + 59) < 60 * 2,
            "a second or less in the last five minutes"
        );
    }
    for s in (7200 - 3600)..(7200 - 300) {
        assert!(gap(s * 60) <= 60 * 61, "a minute or less in the last hour");
    }
    for s in 0..(7200 - 3600) {
        assert!(gap(s * 60) <= 60 * 601, "ten minutes or less beyond");
    }
}

#[test]
fn over_budget_the_oldest_plain_keyframes_go_then_the_floor_moves() {
    let b = base();
    let each = b.bytes;
    let mut r = Ring::new(60, each * 10);
    r.push(keyframe_at(&b, 0, true));
    r.push(keyframe_at(&b, 5, true)); // an edit at 5 s
    for s in 1..=30 {
        if s != 5 {
            r.push(keyframe_at(&b, s, false));
        }
    }
    assert!(r.bytes() <= each * 10);
    let ticks = r.ticks();
    assert!(
        ticks.contains(&0) && ticks.contains(&300),
        "barriers stay: {ticks:?}"
    );
    assert_eq!(*ticks.last().unwrap(), 30 * 60, "the newest stays");
    assert_eq!(
        r.floor, 0,
        "every frame still reachable, with longer replays"
    );
    // Barriers alone over the budget: the oldest go and the floor moves up,
    // since a replay may not cross a change between ticks.
    for s in 31..=45 {
        r.push(keyframe_at(&b, s, true));
    }
    assert!(r.bytes() <= each * 10);
    assert!(r.floor > 0, "{}", r.floor);
    assert_eq!(r.ticks()[0], r.floor);
}

#[test]
fn a_branch_drops_what_came_after_it() {
    let b = base();
    let mut r = Ring::new(60, usize::MAX);
    for s in 0..=20 {
        r.push(keyframe_at(&b, s, s == 0));
    }
    assert_eq!(r.end, 1200);
    r.prune_after(600);
    assert_eq!(r.end, 600);
    assert_eq!(*r.ticks().last().unwrap(), 600);
    // A replayed stretch pushes the same keyframes again; they are not kept
    // twice.
    let n = r.len();
    r.push(keyframe_at(&b, 5, false));
    assert_eq!(r.len(), n);
}
