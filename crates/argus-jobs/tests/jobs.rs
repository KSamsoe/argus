//! The job system (phase 3 item 3, §12): tasks with requirements, claimed
//! by priority and distance, walked to over the navigation grid, worked,
//! carried; and why a worker is idle.

use argus_ecs::{Entity, Event, StrId, Transform, Value, Vec2, World};
use argus_jobs::{Job, JobKind, Worker};
use argus_nav::{NavAgent, NavCost, NavGrid, NavStore};
use argus_sim::{Scheduler, SchedulerConfig};
use std::cell::RefCell;
use std::rc::Rc;

struct Colony {
    w: World,
    s: Scheduler,
    nav: Rc<RefCell<NavStore>>,
}

fn colony(rows: &[&str]) -> Colony {
    let root = std::env::temp_dir();
    let mut w = World::new();
    w.register::<Transform>().unwrap();
    let mut s = Scheduler::new(SchedulerConfig::default());
    let nav = Rc::new(RefCell::new(NavStore::new(&root, None)));
    argus_nav::install(&mut w, &mut s, nav.clone()).unwrap();
    argus_jobs::install(&mut w, &mut s, nav.clone()).unwrap();
    let h = rows.len() as u32;
    let g = w.spawn_authored("grid").unwrap();
    w.insert(
        g,
        NavGrid {
            origin: Vec2::new(-0.5, -0.5),
            width: rows[0].len() as u32,
            height: h,
            diagonal: true,
            ..NavGrid::default()
        },
    )
    .unwrap();
    for (i, row) in rows.iter().enumerate() {
        let r = h as usize - 1 - i;
        for (c, ch) in row.chars().enumerate() {
            if ch == '#' {
                let e = w.spawn();
                w.insert(e, Transform::at(c as f32, r as f32)).unwrap();
                w.insert(e, NavCost::default()).unwrap();
            }
        }
    }
    Colony { w, s, nav }
}

fn s(w: &World, x: &str) -> StrId {
    w.strings().intern(x)
}

fn worker(c: &mut Colony, at: (f32, f32), skills: &str) -> Entity {
    let e = c.w.spawn();
    c.w.insert(e, Transform::at(at.0, at.1)).unwrap();
    let skills = s(&c.w, skills);
    c.w.insert(
        e,
        Worker {
            skills,
            ..Worker::default()
        },
    )
    .unwrap();
    c.w.insert(
        e,
        NavAgent {
            goal: Vec2::new(at.0, at.1),
            speed: 5.0,
            ..NavAgent::default()
        },
    )
    .unwrap();
    e
}

fn thing(c: &mut Colony, at: (f32, f32)) -> Entity {
    let e = c.w.spawn();
    c.w.insert(e, Transform::at(at.0, at.1)).unwrap();
    e
}

fn job(c: &mut Colony, kind: &str, priority: f32, site: Entity, work: f32) -> Entity {
    let e = c.w.spawn();
    let kind = s(&c.w, kind);
    c.w.insert(
        e,
        Job {
            kind,
            priority,
            site,
            work,
            ..Job::default()
        },
    )
    .unwrap();
    e
}

/// Game events of the ticks run since the last call, as (name, fields).
fn events(w: &mut World) -> Vec<(String, Vec<(String, Value)>)> {
    w.drain_events()
        .into_iter()
        .filter_map(|r| match r.event {
            Event::Game { name, fields } => Some((name, fields)),
            _ => None,
        })
        .collect()
}

fn field<'a>(f: &'a [(String, Value)], k: &str) -> &'a Value {
    &f.iter().find(|(n, _)| n == k).unwrap().1
}

fn text(f: &[(String, Value)], k: &str) -> String {
    match field(f, k) {
        Value::Str(s) => s.clone(),
        other => panic!("{k} is {other:?}"),
    }
}

fn idle(w: &World, e: Entity) -> String {
    w.strings()
        .resolve(w.get::<Worker>(e).unwrap().idle)
        .to_string()
}

fn pos(w: &World, e: Entity) -> [f32; 2] {
    let p = w.get::<Transform>(e).unwrap().pos;
    [p.x, p.y]
}

#[test]
fn a_worker_takes_the_highest_priority_job_then_the_nearest_and_works_it() {
    let mut c = colony(&["..........", "..........", ".........."]);
    let w0 = worker(&mut c, (0.0, 1.0), "");
    let (a, b, d) = (
        thing(&mut c, (2.0, 1.0)),
        thing(&mut c, (8.0, 1.0)),
        thing(&mut c, (6.0, 1.0)),
    );
    let low = job(&mut c, "low", 1.0, a, 0.5);
    let far = job(&mut c, "far", 5.0, b, 0.5);
    let near = job(&mut c, "near", 5.0, d, 0.5);
    c.s.step(&mut c.w, 1).unwrap();
    // Priority first; the nearer of the two at 5.
    assert_eq!(c.w.get::<Worker>(w0).unwrap().job, near);
    assert_eq!(c.w.get::<Job>(near).unwrap().worker, w0);
    assert_eq!(
        c.w.strings().resolve(c.w.get::<Job>(near).unwrap().stage),
        "going"
    );
    let ev = events(&mut c.w);
    let claimed: Vec<_> = ev.iter().filter(|e| e.0 == "job.claimed").collect();
    assert_eq!(claimed.len(), 1);
    assert_eq!(text(&claimed[0].1, "kind"), "near");
    // It walks there, works half a second, and is done; then the next.
    let mut done = Vec::new();
    for _ in 0..600 {
        c.s.step(&mut c.w, 1).unwrap();
        for (n, f) in events(&mut c.w) {
            if n == "job.done" {
                done.push(text(&f, "kind"));
                assert_eq!(*field(&f, "worker"), Value::Entity(w0));
            }
        }
        if done.len() == 3 {
            break;
        }
    }
    assert_eq!(done, ["near", "far", "low"]);
    for j in [low, far, near] {
        assert!(!c.w.is_alive(j), "a finished job is despawned");
    }
    // It stands where the last one was worked, idle, and says why.
    assert_eq!(pos(&c.w, w0), [2.0, 1.0]);
    c.s.step(&mut c.w, 2).unwrap();
    assert!(c.w.get::<Worker>(w0).unwrap().job.is_null());
    assert_eq!(idle(&c.w, w0), "no jobs are posted");
}

#[test]
fn skills_only_and_reach_decide_and_an_idle_worker_says_why_once() {
    let mut c = colony(&["....#...", "....#...", "....#..."]);
    // The other first: it takes its own job before the farmer looks.
    let other = worker(&mut c, (1.0, 0.0), "");
    let farmer = worker(&mut c, (0.0, 0.0), "farm haul");
    // Each of these is not the farmer's: a skill, another's, a wall.
    let stove = thing(&mut c, (2.0, 2.0));
    let cook = job(&mut c, "cook", 1.0, stove, 1.0);
    let need = s(&c.w, "cook");
    c.w.update::<Job, _>(cook, |j| j.requires = need).unwrap();
    let bed = thing(&mut c, (3.0, 2.0));
    let sleep = job(&mut c, "sleep", 9.0, bed, 1.0);
    c.w.update::<Job, _>(sleep, |j| j.only = other).unwrap();
    let across = thing(&mut c, (6.0, 1.0));
    let far = job(&mut c, "harvest", 1.0, across, 1.0);
    let farm = s(&c.w, "farm");
    c.w.update::<Job, _>(far, |j| j.requires = farm).unwrap();
    c.s.step(&mut c.w, 1).unwrap();
    // The other worker takes its own; the farmer none.
    assert_eq!(c.w.get::<Worker>(other).unwrap().job, sleep);
    assert!(c.w.get::<Worker>(farmer).unwrap().job.is_null());
    let why = idle(&c.w, farmer);
    assert_eq!(
        why,
        "none of the 2 open jobs is its to do: 1 need cook, 1 out of reach; 1 more taken"
    );
    let ev = events(&mut c.w);
    let idles: Vec<_> = ev.iter().filter(|e| e.0 == "job.idle").collect();
    assert_eq!(idles.len(), 1);
    assert_eq!(*field(&idles[0].1, "worker"), Value::Entity(farmer));
    assert_eq!(text(&idles[0].1, "why"), why);
    // Looking again for the same reasons says nothing new.
    c.s.step(&mut c.w, 90).unwrap();
    assert!(events(&mut c.w)
        .iter()
        .all(|e| e.0 != "job.idle" || *field(&e.1, "worker") != Value::Entity(farmer)));
    // The why, job by job, as jobs.why answers it.
    let (_, rows, text) = argus_jobs::explain(&c.w, &mut c.nav.borrow_mut(), farmer).unwrap();
    assert_eq!(text, why);
    let verdicts: Vec<(Entity, String)> = rows.iter().map(|r| (r.job, r.verdict.clone())).collect();
    assert_eq!(
        verdicts,
        [
            (sleep, format!("{other} has it")),
            (cook, "needs cook".to_string()),
            (far, "out of reach".to_string()),
        ]
    );
    // A door in the wall: now it can reach the harvest.
    let wall =
        c.w.query()
            .has(c.w.component_id_of::<NavCost>().unwrap())
            .entities()
            .into_iter()
            .find(|&e| pos(&c.w, e) == [4.0, 1.0])
            .unwrap();
    c.w.despawn(wall).unwrap();
    c.s.step(&mut c.w, 31).unwrap();
    assert_eq!(c.w.get::<Worker>(farmer).unwrap().job, far);
}

#[test]
fn a_job_with_a_second_leg_carries_its_cargo_there() {
    let mut c = colony(&["........", "........", "........"]);
    let hauler = worker(&mut c, (0.0, 0.0), "");
    let item = thing(&mut c, (3.0, 0.0));
    let pile = thing(&mut c, (7.0, 2.0));
    let haul = job(&mut c, "haul", 1.0, item, 0.25);
    c.w.update::<Job, _>(haul, |j| {
        j.to = pile;
        j.carry = item;
    })
    .unwrap();
    let mut seen = Vec::new();
    for _ in 0..400 {
        c.s.step(&mut c.w, 1).unwrap();
        for (n, f) in events(&mut c.w) {
            if n.starts_with("job.") && n != "job.idle" {
                seen.push(n.clone());
            }
            if n == "job.done" {
                assert_eq!(*field(&f, "carry"), Value::Entity(item));
                assert_eq!(*field(&f, "to"), Value::Entity(pile));
            }
            if n == "job.carrying" {
                // Picked up where it lay.
                assert_eq!(pos(&c.w, item), [3.0, 0.0]);
            }
        }
        if seen.last().is_some_and(|n| n == "job.done") {
            break;
        }
    }
    assert_eq!(
        seen,
        ["job.claimed", "job.started", "job.carrying", "job.done"]
    );
    // What it carried arrived with it.
    assert_eq!(pos(&c.w, hauler), [7.0, 2.0]);
    assert_eq!(pos(&c.w, item), [7.0, 2.0]);
}

#[test]
fn a_job_only_for_a_worker_that_outranks_its_own_takes_it_off_that() {
    let mut c = colony(&["..........", ".........."]);
    let w0 = worker(&mut c, (0.0, 0.0), "");
    let item = thing(&mut c, (2.0, 0.0));
    let pile = thing(&mut c, (9.0, 0.0));
    let haul = job(&mut c, "haul", 1.0, item, 0.1);
    c.w.update::<Job, _>(haul, |j| {
        j.to = pile;
        j.carry = item;
    })
    .unwrap();
    // Carrying toward the pile ...
    for _ in 0..200 {
        c.s.step(&mut c.w, 1).unwrap();
        if c.w.strings().resolve(c.w.get::<Job>(haul).unwrap().stage) == "carrying"
            && pos(&c.w, w0)[0] > 4.0
        {
            break;
        }
    }
    events(&mut c.w);
    let dropped_at = pos(&c.w, w0);
    // ... it gets hungry.
    let table = thing(&mut c, (0.0, 1.0));
    let eat = job(&mut c, "eat", 10.0, table, 0.1);
    c.w.update::<Job, _>(eat, |j| j.only = w0).unwrap();
    c.s.step(&mut c.w, 1).unwrap();
    let ev = events(&mut c.w);
    let rel = ev.iter().find(|e| e.0 == "job.released").unwrap();
    assert_eq!(*field(&rel.1, "job"), Value::Entity(haul));
    assert_eq!(text(&rel.1, "why"), "a job only for it outranks it");
    assert_eq!(c.w.get::<Worker>(w0).unwrap().job, eat);
    // The haul starts again from where the cargo was left.
    let h = c.w.get::<Job>(haul).unwrap();
    assert!(h.worker.is_null());
    assert_eq!(h.site, item);
    let left = pos(&c.w, item);
    assert!(
        (left[0] - dropped_at[0]).abs() < 0.2,
        "{left:?} {dropped_at:?}"
    );
    // After the meal it picks the haul up again and finishes it.
    let mut kinds = Vec::new();
    for _ in 0..600 {
        c.s.step(&mut c.w, 1).unwrap();
        for (n, f) in events(&mut c.w) {
            if n == "job.done" {
                kinds.push(text(&f, "kind"));
            }
        }
        if kinds.len() == 2 {
            break;
        }
    }
    assert_eq!(kinds, ["eat", "haul"]);
    assert_eq!(pos(&c.w, item), [9.0, 0.0]);
}

#[test]
fn a_claim_is_undone_when_its_worker_goes_and_a_job_is_cancelled_when_its_site_does() {
    let mut c = colony(&["........"]);
    let w0 = worker(&mut c, (0.0, 0.0), "");
    let w1 = worker(&mut c, (7.0, 0.0), "");
    let a = thing(&mut c, (1.0, 0.0));
    let b = thing(&mut c, (6.0, 0.0));
    let ja = job(&mut c, "a", 1.0, a, 5.0);
    let jb = job(&mut c, "b", 1.0, b, 5.0);
    c.s.step(&mut c.w, 1).unwrap();
    assert_eq!(c.w.get::<Job>(ja).unwrap().worker, w0);
    assert_eq!(c.w.get::<Job>(jb).unwrap().worker, w1);
    events(&mut c.w);
    c.w.despawn(w0).unwrap();
    c.w.despawn(b).unwrap();
    c.s.step(&mut c.w, 1).unwrap();
    let ev = events(&mut c.w);
    let rel = ev.iter().find(|e| e.0 == "job.released").unwrap();
    assert_eq!(*field(&rel.1, "job"), Value::Entity(ja));
    assert_eq!(text(&rel.1, "why"), "its worker is gone");
    let can = ev.iter().find(|e| e.0 == "job.cancelled").unwrap();
    assert_eq!(*field(&can.1, "job"), Value::Entity(jb));
    assert_eq!(text(&can.1, "why"), "its site is gone");
    assert!(!c.w.is_alive(jb));
    // The survivor, free again, takes the job the other left.
    assert_eq!(c.w.get::<Worker>(w1).unwrap().job, ja);
}

#[test]
fn a_job_kind_adds_its_priority_skill_and_work() {
    let mut c = colony(&["......"]);
    let cook = worker(&mut c, (0.0, 0.0), "cook");
    let k = c.w.spawn();
    let (name, skill) = (s(&c.w, "meal"), s(&c.w, "cook"));
    c.w.insert(
        k,
        JobKind {
            name,
            priority: 20.0,
            skill,
            work: 2.0,
        },
    )
    .unwrap();
    let a = thing(&mut c, (1.0, 0.0));
    let b = thing(&mut c, (5.0, 0.0));
    let chore = job(&mut c, "chore", 10.0, a, 0.5);
    let meal = job(&mut c, "meal", 0.0, b, 0.0);
    c.s.step(&mut c.w, 1).unwrap();
    // The kind's 20 beats the chore's own 10, farther though it is.
    assert_eq!(c.w.get::<Worker>(cook).unwrap().job, meal);
    // Its kind's two seconds of work: 5 away at 5 a second (60 ticks), then 120.
    let mut ticks = 0;
    while c.w.is_alive(meal) {
        c.s.step(&mut c.w, 1).unwrap();
        ticks += 1;
        assert!(ticks < 400);
    }
    assert!((178..=184).contains(&ticks), "{ticks}");
    // Without the skill, the kind's jobs are not its to do.
    let other = worker(&mut c, (0.0, 0.0), "haul");
    let meal2 = job(&mut c, "meal", 0.0, b, 0.0);
    let _ = (chore, meal2);
    c.s.step(&mut c.w, 1).unwrap();
    assert_eq!(c.w.get::<Worker>(other).unwrap().job, chore);
}

#[test]
fn jobs_go_the_same_way_after_a_seek_with_a_fresh_store() {
    let rows = ["..........", "..####....", ".........."];
    let mut c = colony(&rows);
    let mut workers = Vec::new();
    for i in 0..3 {
        workers.push(worker(&mut c, (i as f32, 0.0), ""));
    }
    for i in 0..8 {
        let at = thing(&mut c, ((i * 3 % 10) as f32, (i % 3) as f32));
        let pile = thing(&mut c, (9.0, 2.0));
        let j = job(&mut c, "haul", (i % 3) as f32, at, 0.5);
        c.w.update::<Job, _>(j, |x| {
            x.to = pile;
            x.carry = at;
        })
        .unwrap();
    }
    c.s.step(&mut c.w, 30).unwrap();
    let kept = c.w.snapshot();
    c.s.step(&mut c.w, 150).unwrap();
    let want: Vec<([f32; 2], Worker)> = workers
        .iter()
        .map(|&e| (pos(&c.w, e), c.w.get::<Worker>(e).unwrap()))
        .collect();
    let mut d = colony(&rows);
    d.w.restore(&kept);
    d.s.step(&mut d.w, 150).unwrap();
    let got: Vec<([f32; 2], Worker)> = workers
        .iter()
        .map(|&e| (pos(&d.w, e), d.w.get::<Worker>(e).unwrap()))
        .collect();
    assert_eq!(got, want);
    assert_eq!(d.w.digest(), c.w.digest());
}
