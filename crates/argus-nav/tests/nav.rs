//! Navigation (phase 3 item 2, §12): grids from a tilemap and blocking
//! entities, A* for individuals, flow fields for crowds, baked grids, and
//! the agents that walk them.

use argus_ecs::{Entity, Event, Transform, Value, Vec2, World};
use argus_nav::grid::STRAIGHT;
use argus_nav::search::{self, Scratch};
use argus_nav::{bake, Grid, NavAgent, NavCost, NavField, NavGrid, NavStore};
use argus_sim::{Scheduler, SchedulerConfig};
use argus_tiles::{LayerDef, MapFile, MapStore, Rect, Tilemap};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

struct Nav {
    w: World,
    s: Scheduler,
    store: Rc<RefCell<NavStore>>,
    tiles: Rc<RefCell<MapStore>>,
    root: Temp,
}

/// A temp directory removed on drop, used as its path.
struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for Temp {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn temp(name: &str) -> Temp {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("argus-nav-{name}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    Temp(d)
}

fn nav(name: &str) -> Nav {
    let root = temp(name);
    let mut w = World::new();
    w.register::<Transform>().unwrap();
    argus_tiles::register(&mut w).unwrap();
    let mut s = Scheduler::new(SchedulerConfig::default());
    let tiles = Rc::new(RefCell::new(MapStore::new(&root)));
    let store = Rc::new(RefCell::new(NavStore::new(&root, Some(tiles.clone()))));
    argus_nav::install(&mut w, &mut s, store.clone()).unwrap();
    Nav {
        w,
        s,
        store,
        tiles,
        root,
    }
}

/// A map from rows (top row first): `#` a wall entity, anything else open.
/// The grid's cell (c, r) has its centre at (c, r).
fn map(n: &mut Nav, rows: &[&str], diagonal: bool) -> Entity {
    let h = rows.len() as u32;
    let wd = rows[0].len() as u32;
    let g = n.w.spawn_authored("grid").unwrap();
    n.w.insert(
        g,
        NavGrid {
            origin: Vec2::new(-0.5, -0.5),
            width: wd,
            height: h,
            diagonal,
            ..NavGrid::default()
        },
    )
    .unwrap();
    for (i, row) in rows.iter().enumerate() {
        let r = h as usize - 1 - i;
        for (c, ch) in row.chars().enumerate() {
            if ch == '#' {
                let e = n.w.spawn();
                n.w.insert(e, Transform::at(c as f32, r as f32)).unwrap();
                n.w.insert(e, NavCost::default()).unwrap();
            }
        }
    }
    g
}

fn cell(g: &Grid, c: i64, r: i64) -> u32 {
    g.index(c, r).unwrap()
}

#[test]
fn a_star_goes_around_walls_and_never_cuts_a_corner() {
    let mut n = nav("astar");
    let ge = map(
        &mut n,
        &[".......", ".#####.", ".....#.", "####.#.", "......."],
        false,
    );
    let g = n.store.borrow_mut().grid(&n.w, ge, false).unwrap();
    assert_eq!((g.width, g.height), (7, 5));
    assert_eq!(g.open_cells(), 35 - 11);
    let mut s = Scratch::default();
    // From the bottom-left corner to the middle of the pocket at (2, 2).
    let r = search::astar(&g, cell(&g, 0, 0), cell(&g, 2, 2), &mut s).unwrap();
    let cells: Vec<(i64, i64)> = r.cells.iter().map(|&i| g.col_row(i)).collect();
    assert_eq!(cells.first(), Some(&(0, 0)));
    assert_eq!(cells.last(), Some(&(2, 2)));
    // Along the bottom, up the gap at column 4, and left into the pocket.
    assert!(cells.contains(&(4, 1)), "{cells:?}");
    assert!(cells.iter().all(|&(c, r)| g.open(cell(&g, c, r))));
    assert_eq!(r.cost, (cells.len() as u32 - 1) * STRAIGHT);
    // Every step is one cell to the side (four ways).
    for w in cells.windows(2) {
        assert_eq!((w[0].0 - w[1].0).abs() + (w[0].1 - w[1].1).abs(), 1);
    }
    // A blocked goal has no path; a start in a wall walks out.
    assert!(search::astar(&g, cell(&g, 0, 0), cell(&g, 1, 3), &mut s).is_none());
    assert!(search::astar(&g, cell(&g, 1, 3), cell(&g, 0, 4), &mut s).is_some());

    // Eight ways: a diagonal step, but never past a wall's corner.
    let mut d = nav("diag");
    let ge = map(&mut d, &["...", "#..", ".#."], true);
    let g = d.store.borrow_mut().grid(&d.w, ge, false).unwrap();
    let r = search::astar(&g, cell(&g, 0, 0), cell(&g, 1, 1), &mut s);
    assert!(
        r.is_none(),
        "(0, 0) is closed in by two walls' corners: {r:?}"
    );
    let r = search::astar(&g, cell(&g, 1, 1), cell(&g, 2, 2), &mut s).unwrap();
    assert_eq!((r.cells.len(), r.cost), (2, 14), "one diagonal step");
    // (1, 1) to (2, 0) would pass the wall at (1, 0): around instead.
    let r = search::astar(&g, cell(&g, 1, 1), cell(&g, 2, 0), &mut s).unwrap();
    assert_eq!((r.cells.len(), r.cost), (3, 20), "{:?}", r.cells);
}

#[test]
fn a_flow_field_is_every_cells_steps_to_the_target_and_downhill_follows_it() {
    let mut n = nav("flow");
    let ge = map(
        &mut n,
        &[".......", ".#####.", ".....#.", "####.#.", "......."],
        false,
    );
    let g = n.store.borrow_mut().grid(&n.w, ge, false).unwrap();
    let target = cell(&g, 2, 2);
    let f = search::flow(&g, target, 0);
    assert_eq!(f.reached as usize, g.open_cells());
    // Every open cell's value is its A* cost to the target.
    let mut s = Scratch::default();
    for i in 0..g.len() as u32 {
        if !g.open(i) {
            assert_eq!(f.cost(i), None);
            continue;
        }
        let r = search::astar(&g, i, target, &mut s).unwrap();
        assert_eq!(f.cost(i), Some(r.cost), "cell {:?}", g.col_row(i));
    }
    // Downhill from the far corner walks there, a cost step at a time.
    let mut at = cell(&g, 6, 4);
    let mut n_steps = 0;
    while let Some(next) = search::downhill(&g, &f, at) {
        assert_eq!(f.cost(next).unwrap() + STRAIGHT, f.cost(at).unwrap());
        at = next;
        n_steps += 1;
    }
    assert_eq!(at, target);
    assert_eq!(
        n_steps * STRAIGHT,
        search::astar(&g, cell(&g, 6, 4), target, &mut s)
            .unwrap()
            .cost
    );
    // A range: the far side is not reached.
    let near = search::flow(&g, target, 3 * STRAIGHT);
    assert!(near.cost(cell(&g, 0, 2)).is_some());
    assert!(near.cost(cell(&g, 6, 4)).is_none());
}

#[test]
fn a_grid_is_rebuilt_when_what_it_covers_changes_and_only_then() {
    let mut n = nav("rebuild");
    let ge = map(&mut n, &["....", "....", "...."], false);
    let builds = || n.store.borrow().counts.builds;
    let g1 = n.store.borrow_mut().grid(&n.w, ge, false).unwrap();
    assert_eq!(builds(), 1);
    assert_eq!(g1.open_cells(), 12);
    // Nothing changed: the same grid, not checked again within the frame.
    n.s.step(&mut n.w, 1).unwrap();
    let g2 = n.store.borrow_mut().grid(&n.w, ge, false).unwrap();
    assert!(Rc::ptr_eq(&g1, &g2));
    assert_eq!(builds(), 1);
    // A wall spawned: rebuilt at once (the world's structure moved).
    let wall = n.w.spawn();
    n.w.insert(wall, Transform::at(1.0, 1.0)).unwrap();
    n.w.insert(wall, NavCost::default()).unwrap();
    let g3 = n.store.borrow_mut().grid(&n.w, ge, false).unwrap();
    assert_eq!(g3.open_cells(), 11);
    assert_ne!(g3.hash, g1.hash);
    // Moved: seen the next frame (a field change within a frame is not).
    n.w.update::<Transform, _>(wall, |t| t.pos = Vec2::new(2.0, 1.0))
        .unwrap();
    n.s.step(&mut n.w, 1).unwrap();
    let g4 = n.store.borrow_mut().grid(&n.w, ge, false).unwrap();
    assert!(!g4.open(cell(&g4, 2, 1)) && g4.open(cell(&g4, 1, 1)));
    // A cost of 3 over a box of two cells: slower going, not a wall.
    n.w.update::<NavCost, _>(wall, |c| {
        c.cost = 3;
        c.size = Vec2::new(2.0, 1.0);
    })
    .unwrap();
    let g5 = n.store.borrow_mut().grid(&n.w, ge, true).unwrap();
    assert_eq!(g5.cost[cell(&g5, 1, 1) as usize], 3);
    assert_eq!(g5.cost[cell(&g5, 2, 1) as usize], 3);
    assert_eq!(g5.cost[cell(&g5, 3, 1) as usize], 1);
    assert_eq!(g5.rows(0, 0, 3, 2, 1), ["....", ".33.", "...."]);
    // A block wins where a cost and a block overlap.
    let block = n.w.spawn();
    n.w.insert(block, Transform::at(1.0, 1.0)).unwrap();
    n.w.insert(block, NavCost::default()).unwrap();
    let g6 = n.store.borrow_mut().grid(&n.w, ge, false).unwrap();
    assert_eq!(g6.rows(0, 0, 3, 2, 1), ["....", ".#3.", "...."]);
    // An open map's cells can come from entities instead (fill 0).
    n.w.update::<NavGrid, _>(ge, |g| g.fill = 0).unwrap();
    let g7 = n.store.borrow_mut().grid(&n.w, ge, true).unwrap();
    assert_eq!(g7.rows(0, 0, 3, 2, 1), ["####", "##3#", "####"]);
}

#[test]
fn a_grid_covers_a_tilemap_and_follows_its_edits() {
    let mut n = nav("tiles");
    let mut m = MapFile::create(
        &n.root,
        "level.tilemap.toml",
        6,
        4,
        2.0,
        8,
        1,
        vec![LayerDef {
            name: "ground".into(),
            tileset: None,
            z: 0.0,
            collision: true,
        }],
    )
    .unwrap();
    // A wall down column 2 (tile rows go down), open at the bottom row.
    m.fill(0, Rect::new(2, 0, 1, 3), 1).unwrap();
    m.save().unwrap();
    let tm = n.w.spawn_authored("level").unwrap();
    n.w.insert(tm, Transform::at(0.0, 8.0)).unwrap();
    let file = n.w.strings().intern("level.tilemap.toml");
    n.w.insert(
        tm,
        Tilemap {
            map: file,
            visible: true,
        },
    )
    .unwrap();
    let ge = n.w.spawn_authored("nav").unwrap();
    n.w.insert(
        ge,
        NavGrid {
            tilemap: tm,
            ..NavGrid::default()
        },
    )
    .unwrap();
    let g = n.store.borrow_mut().grid(&n.w, ge, false).unwrap();
    assert_eq!((g.width, g.height, g.cell), (6, 4, 2.0));
    assert_eq!(g.origin, [0.0, 0.0], "the tilemap's bottom-left corner");
    assert_eq!(
        g.rows(0, 0, 5, 3, 1),
        ["..#...", "..#...", "..#...", "......"]
    );
    // A path from the left to the right goes under the wall.
    let walk = n
        .store
        .borrow_mut()
        .walk(&n.w, [1.0, 7.0], [11.0, 7.0], Entity::NULL, false)
        .unwrap()
        .unwrap();
    let route = walk.route.unwrap();
    assert!(route.cells.iter().any(|&c| g.col_row(c) == (2, 0)));
    assert_eq!(
        walk.points.last(),
        Some(&[11.0, 7.0]),
        "the goal itself last"
    );
    // The map edited (as edit.tiles does): the gap is closed.
    n.tiles
        .borrow_mut()
        .open("level.tilemap.toml")
        .unwrap()
        .fill(0, Rect::new(2, 3, 1, 1), 1)
        .unwrap();
    let g2 = n.store.borrow_mut().grid(&n.w, ge, true).unwrap();
    assert_eq!(g2.rows(2, 0, 2, 3, 1), ["#", "#", "#", "#"]);
    let walk = n
        .store
        .borrow_mut()
        .walk(&n.w, [1.0, 7.0], [11.0, 7.0], Entity::NULL, true)
        .unwrap()
        .unwrap();
    assert!(walk.route.is_none(), "walled off");
}

#[test]
fn a_baked_grid_reads_back_as_it_was_and_refuses_a_changed_chunk() {
    let mut n = nav("bake");
    let ge = map(&mut n, &["..#.", ".##.", "...."], true);
    let g = n.store.borrow_mut().grid(&n.w, ge, false).unwrap();
    let w = bake::write(&n.root, "scenes/a/grid.navgrid.toml", &g, "@grid").unwrap();
    assert_eq!(w.chunk, "scenes/a/grid.navgrid.bin");
    assert_eq!((w.bytes, w.open), (12, 9));
    let index = std::fs::read_to_string(n.root.join(&w.index)).unwrap();
    assert!(
        index.contains("width = 4") && index.contains("origin = [-0.5, -0.5]"),
        "{index}"
    );
    let back = bake::read(&n.root, &w.index).unwrap();
    assert_eq!(back, *g);
    assert!(bake::is_index("scenes/a/grid.navgrid.toml"));
    // A NavGrid starts from it: the walls are gone as entities and stay.
    let walls: Vec<Entity> =
        n.w.query()
            .has(n.w.component_id_of::<NavCost>().unwrap())
            .entities();
    for e in walls {
        n.w.despawn(e).unwrap();
    }
    let baked = n.w.strings().intern(&w.index);
    n.w.update::<NavGrid, _>(ge, |g| g.baked = baked).unwrap();
    let g2 = n.store.borrow_mut().grid(&n.w, ge, false).unwrap();
    assert_eq!(g2.cost, g.cost);
    // A chunk that does not match its index is refused.
    let mut bytes = std::fs::read(n.root.join(&w.chunk)).unwrap();
    bytes[0] = 7;
    std::fs::write(n.root.join(&w.chunk), bytes).unwrap();
    let e = bake::read(&n.root, &w.index).unwrap_err();
    assert_eq!(e.code, argus_nav::codes::BAD_BAKE);
}

fn events(w: &mut World, name: &str) -> Vec<Vec<(String, Value)>> {
    w.drain_events()
        .into_iter()
        .filter_map(|r| match r.event {
            Event::Game { name: n, fields } if n == name => Some(fields),
            _ => None,
        })
        .collect()
}

fn state(w: &World, e: Entity) -> String {
    w.strings()
        .resolve(w.get::<NavAgent>(e).unwrap().state)
        .to_string()
}

fn pos(w: &World, e: Entity) -> [f32; 2] {
    let p = w.get::<Transform>(e).unwrap().pos;
    [p.x, p.y]
}

#[test]
fn an_agent_walks_around_the_walls_to_its_goal_and_says_so() {
    let mut n = nav("agent");
    map(
        &mut n,
        &[".......", ".#####.", ".....#.", "####.#.", "......."],
        false,
    );
    let a = n.w.spawn();
    n.w.insert(a, Transform::at(0.0, 0.0)).unwrap();
    n.w.insert(
        a,
        NavAgent {
            goal: Vec2::new(2.0, 2.0),
            speed: 6.0,
            ..NavAgent::default()
        },
    )
    .unwrap();
    n.s.step(&mut n.w, 1).unwrap();
    assert_eq!(state(&n.w, a), "moving");
    let first = n.w.get::<NavAgent>(a).unwrap();
    assert_eq!(first.next, Vec2::new(1.0, 0.0), "the first waypoint");
    assert!(first.remaining > 7.0, "{}", first.remaining);
    // It never stands in a wall on the way.
    let g = n
        .store
        .borrow_mut()
        .grid(&n.w, n.w.entity("grid").unwrap(), false)
        .unwrap();
    let mut ticks = 1;
    while state(&n.w, a) != "arrived" && ticks < 600 {
        n.s.step(&mut n.w, 1).unwrap();
        let p = pos(&n.w, a);
        assert!(g.open(g.locate(p).unwrap()), "in a wall at {p:?}");
        ticks += 1;
    }
    assert_eq!(state(&n.w, a), "arrived");
    let p = pos(&n.w, a);
    assert!(
        (p[0] - 2.0).abs() < 0.05 && (p[1] - 2.0).abs() < 0.05,
        "{p:?}"
    );
    // The tick after, it stands on the goal itself.
    n.s.step(&mut n.w, 1).unwrap();
    assert_eq!(pos(&n.w, a), [2.0, 2.0]);
    // Eight cells (four along, two up through the gap, two back) at six a
    // second: 80 ticks.
    assert!((78..84).contains(&ticks), "{ticks}");
    assert_eq!(events(&mut n.w, "nav.arrived").len(), 1);
    // A new goal: off again; a walled-in one: blocked, once.
    n.w.update::<NavAgent, _>(a, |x| x.goal = Vec2::new(1.0, 3.0))
        .unwrap();
    n.s.step(&mut n.w, 3).unwrap();
    assert_eq!(state(&n.w, a), "blocked");
    let blocked = events(&mut n.w, "nav.blocked");
    assert_eq!(blocked.len(), 1);
    assert!(blocked[0]
        .iter()
        .any(|(k, v)| k == "why" && *v == Value::Str("no path to the goal".into())));
    n.w.update::<NavAgent, _>(a, |x| x.goal = Vec2::new(6.0, 4.0))
        .unwrap();
    n.s.step(&mut n.w, 1).unwrap();
    assert_eq!(state(&n.w, a), "moving");
    // Speed 0: the path is found and the game moves it; the agent keeps
    // its place along the path.
    n.w.update::<NavAgent, _>(a, |x| x.speed = 0.0).unwrap();
    let before = pos(&n.w, a);
    n.s.step(&mut n.w, 5).unwrap();
    assert_eq!(pos(&n.w, a), before);
}

#[test]
fn a_crowd_goes_down_one_field_to_the_entity_it_is_on() {
    let mut n = nav("crowd");
    map(
        &mut n,
        &["..........", "..######..", "..........", ".........."],
        true,
    );
    let hero = n.w.spawn();
    n.w.insert(hero, Transform::at(5.0, 3.0)).unwrap();
    n.w.insert(hero, NavField::default()).unwrap();
    let mut crowd = Vec::new();
    for i in 0..10 {
        let e = n.w.spawn();
        n.w.insert(e, Transform::at(i as f32, 0.0)).unwrap();
        n.w.insert(
            e,
            NavAgent {
                field: hero,
                speed: 5.0,
                ..NavAgent::default()
            },
        )
        .unwrap();
        crowd.push(e);
    }
    n.s.step(&mut n.w, 240).unwrap();
    for &e in &crowd {
        assert_eq!(state(&n.w, e), "arrived", "{e}");
        assert_eq!(pos(&n.w, e), [5.0, 3.0]);
    }
    assert_eq!(
        n.store.borrow().counts.fields,
        1,
        "one field for the whole crowd"
    );
    // The hero moves a cell: the field follows.
    n.w.update::<Transform, _>(hero, |t| t.pos = Vec2::new(9.0, 3.0))
        .unwrap();
    n.s.step(&mut n.w, 120).unwrap();
    assert!(crowd.iter().all(|&e| pos(&n.w, e) == [9.0, 3.0]));
    assert_eq!(n.store.borrow().counts.fields, 2);
}

#[test]
fn agents_walk_the_same_way_after_a_seek_with_a_fresh_store() {
    // The original run.
    let mut n = nav("seek");
    let rows = [
        "............",
        ".##########.",
        "............",
        "#########.##",
        "............",
    ];
    map(&mut n, &rows, true);
    let mut agents = Vec::new();
    for (i, goal) in [(11.0, 4.0), (0.0, 4.0), (6.0, 2.0)]
        .into_iter()
        .enumerate()
    {
        let e = n.w.spawn();
        n.w.insert(e, Transform::at(i as f32 * 3.0, 0.0)).unwrap();
        n.w.insert(
            e,
            NavAgent {
                goal: Vec2::new(goal.0, goal.1),
                speed: 3.0,
                ..NavAgent::default()
            },
        )
        .unwrap();
        agents.push(e);
    }
    n.s.step(&mut n.w, 40).unwrap();
    // A keyframe here, then on to 120.
    let kept = n.w.snapshot();
    n.s.step(&mut n.w, 80).unwrap();
    let want: Vec<[f32; 2]> = agents.iter().map(|&e| pos(&n.w, e)).collect();
    // The same world from the keyframe, with a store that never saw it.
    let mut m = nav("seek2");
    m.w.restore(&kept);
    m.s.step(&mut m.w, 80).unwrap();
    let got: Vec<[f32; 2]> = agents.iter().map(|&e| pos(&m.w, e)).collect();
    assert_eq!(got, want);
    for &e in &agents {
        assert_eq!(
            m.w.get::<NavAgent>(e).unwrap(),
            n.w.get::<NavAgent>(e).unwrap()
        );
    }
}
