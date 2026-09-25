//! Navigation at Outpost's scale (§17: "Outpost's sim tick at 10x with 200
//! colonists ... with pathfinding, jobs, and needs all running", under 1.6
//! ms). Run in release: `cargo test --release -p argus-nav --test perf --
//! --ignored --nocapture`.

use argus_ecs::{Entity, Transform, Vec2, World};
use argus_nav::search::{self, Scratch};
use argus_nav::{Grid, NavAgent, NavField, NavGrid, NavStore};
use argus_sim::{Pcg32, Scheduler, SchedulerConfig};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

/// A `size` x `size` grid with a fifth of its cells walls, the rest cost 1
/// to 3 (a colony's ground), from a fixed seed.
fn ground(size: u32, seed: u64) -> Grid {
    let mut rng = Pcg32::new(seed, 7);
    let mut g = Grid::new([0.0, 0.0], 1.0, size, size, true, 1);
    for c in g.cost.iter_mut() {
        let r = rng.next_f32();
        *c = if r < 0.2 {
            0
        } else if r < 0.3 {
            3
        } else {
            1
        };
    }
    g.cost[0] = 1;
    let last = g.len() - 1;
    g.cost[last] = 1;
    g.seal();
    g
}

fn open_cell(g: &Grid, rng: &mut Pcg32) -> u32 {
    loop {
        let i = (rng.next_f32() * g.len() as f32) as u32 % g.len() as u32;
        if g.open(i) {
            return i;
        }
    }
}

#[test]
#[ignore]
fn perf_a_star_and_a_flow_field_on_256_by_256() {
    let g = ground(256, 3);
    let mut s = Scratch::default();
    let mut rng = Pcg32::new(11, 1);
    let pairs: Vec<(u32, u32)> = (0..200)
        .map(|_| (open_cell(&g, &mut rng), open_cell(&g, &mut rng)))
        .collect();
    let t = Instant::now();
    let mut found = 0;
    let mut expanded = 0u64;
    for &(a, b) in &pairs {
        if let Some(r) = search::astar(&g, a, b, &mut s) {
            found += 1;
            expanded += r.expanded as u64;
        }
    }
    let per = t.elapsed().as_secs_f64() * 1e3 / pairs.len() as f64;
    println!(
        "A* on 256x256 (eight ways, costs 1-3, a fifth walls): {per:.3} ms a path, {found} of {} found, {} cells expanded on average",
        pairs.len(),
        expanded / pairs.len() as u64
    );
    let t = Instant::now();
    let n = 20;
    for &(from, _) in pairs.iter().take(n) {
        let f = search::flow(&g, from, 0);
        assert!(f.reached > 40_000);
    }
    let field = t.elapsed().as_secs_f64() * 1e3 / n as f64;
    println!("flow field over 256x256: {field:.3} ms");
    assert!(per < 2.0, "A* {per:.3} ms");
    assert!(field < 3.0, "field {field:.3} ms");
}

struct Colony {
    w: World,
    s: Scheduler,
    store: Rc<RefCell<NavStore>>,
    agents: Vec<Entity>,
}

fn colony(size: u32, agents: usize, crowd: bool) -> Colony {
    let root = std::env::temp_dir();
    let mut w = World::new();
    w.register::<Transform>().unwrap();
    let mut s = Scheduler::new(SchedulerConfig::default());
    let store = Rc::new(RefCell::new(NavStore::new(&root, None)));
    argus_nav::install(&mut w, &mut s, store.clone()).unwrap();
    let g = ground(size, 5);
    // The same ground as NavCost-free cells: the grid's fill, then walls
    // and mud as a baked-looking grid is too slow to spawn; the grid here
    // is plain and the costs come from a tiny set of entities, so the
    // measurement is the agents'.
    let ge = w.spawn();
    w.insert(
        ge,
        NavGrid {
            width: size,
            height: size,
            diagonal: true,
            ..NavGrid::default()
        },
    )
    .unwrap();
    let mut rng = Pcg32::new(9, 2);
    let hero = w.spawn();
    let hc = g.centre(open_cell(&g, &mut rng));
    w.insert(hero, Transform::at(hc[0], hc[1])).unwrap();
    w.insert(hero, NavField::default()).unwrap();
    let mut list = Vec::new();
    for _ in 0..agents {
        let e = w.spawn();
        let p = g.centre(open_cell(&g, &mut rng));
        let goal = g.centre(open_cell(&g, &mut rng));
        w.insert(e, Transform::at(p[0], p[1])).unwrap();
        w.insert(
            e,
            NavAgent {
                goal: Vec2::new(goal[0], goal[1]),
                field: if crowd { hero } else { Entity::NULL },
                speed: 3.0,
                ..NavAgent::default()
            },
        )
        .unwrap();
        list.push(e);
    }
    Colony {
        w,
        s,
        store,
        agents: list,
    }
}

#[test]
#[ignore]
fn perf_200_colonists_each_with_a_path_of_their_own() {
    let mut c = colony(128, 200, false);
    let t = Instant::now();
    c.s.step(&mut c.w, 1).unwrap();
    let first = t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    let ticks = 300;
    c.s.step(&mut c.w, ticks).unwrap();
    let steady = t.elapsed().as_secs_f64() * 1e3 / ticks as f64;
    let counts = c.store.borrow().counts;
    println!(
        "200 agents on 128x128, own goals: first tick {first:.3} ms ({} searches), then {steady:.3} ms a tick",
        counts.searches
    );
    assert_eq!(c.agents.len(), 200);
    assert!(first < 20.0, "first tick {first:.3} ms");
    assert!(steady < 0.5, "steady {steady:.3} ms");
}

#[test]
#[ignore]
fn perf_10000_followers_down_one_field() {
    let mut c = colony(128, 10_000, true);
    c.s.step(&mut c.w, 1).unwrap();
    let t = Instant::now();
    let ticks = 60;
    c.s.step(&mut c.w, ticks).unwrap();
    let steady = t.elapsed().as_secs_f64() * 1e3 / ticks as f64;
    println!(
        "10,000 followers of one field on 128x128: {steady:.3} ms a tick ({} fields made)",
        c.store.borrow().counts.fields
    );
    assert!(steady < 3.0, "{steady:.3} ms");
}
