//! The dynamic tier (phase 5 item 7): §12 "Dynamic: real rigid bodies with
//! joints, from an optional Rapier module"; three tiers "chosen per entity,
//! sharing one broadphase"; §6, the same run reaches the same world, and a
//! run replayed from a keyframe is the run it replays.

use argus_ecs::{Entity, Event, GlobalTransform, Parent, Transform, Value, Vec2, World};
use argus_physics::{Collider, KinematicBody};
use argus_rigid::{Joint, RigidBody, RigidState, RigidStore, RigidWorld};
use argus_sim::{Scheduler, SchedulerConfig};
use argus_tiles::{LayerDef, MapFile, MapStore, Rect, Tilemap};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

struct Game {
    w: World,
    s: Scheduler,
    store: Rc<RefCell<RigidStore>>,
}

fn dir(tag: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("rigid-{tag}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn game(tag: &str) -> Game {
    let root = dir(tag);
    let mut w = World::new();
    w.register::<Transform>().unwrap();
    w.register::<GlobalTransform>().unwrap();
    w.register::<Parent>().unwrap();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0).seed(3));
    let tiles = Rc::new(RefCell::new(MapStore::new(&root)));
    argus_physics::install(&mut w, &mut s, tiles.clone()).unwrap();
    let store = Rc::new(RefCell::new(RigidStore::new()));
    argus_rigid::install(&mut w, &mut s, tiles, store.clone()).unwrap();
    s.play(&w);
    Game { w, s, store }
}

fn at(x: f32, y: f32) -> Transform {
    Transform {
        pos: Vec2::new(x, y),
        ..Default::default()
    }
}

fn boxed(w: f32, h: f32) -> Collider {
    Collider {
        size: Vec2::new(w, h),
        ..Default::default()
    }
}

fn body(g: &mut Game, t: Transform, c: Collider, r: RigidBody) -> Entity {
    let e = g.w.spawn();
    g.w.insert(e, t).unwrap();
    g.w.insert(e, c).unwrap();
    g.w.insert(e, r).unwrap();
    e
}

fn ground(g: &mut Game) -> Entity {
    body(
        g,
        at(0.0, -0.5),
        boxed(40.0, 1.0),
        RigidBody {
            fixed: true,
            ..Default::default()
        },
    )
}

fn pos(g: &Game, e: Entity) -> Vec2 {
    g.w.get::<Transform>(e).unwrap().pos
}

fn game_events(g: &Game, name: &str) -> Vec<Vec<(String, Value)>> {
    g.w.events()
        .iter()
        .filter_map(|r| match &r.event {
            Event::Game { name: n, fields } if n == name => Some(fields.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_box_falls_lands_and_sleeps_and_the_landing_is_an_event() {
    let mut g = game("land");
    let floor = ground(&mut g);
    let crate_ = body(&mut g, at(0.0, 5.0), boxed(1.0, 1.0), RigidBody::default());
    let mut began = false;
    for _ in 0..240 {
        g.s.step(&mut g.w, 1).unwrap();
        began |= !game_events(&g, "contact.begin").is_empty();
        g.w.drain_events();
    }
    let p = pos(&g, crate_);
    assert!((p.y - 0.5).abs() < 0.02, "rests on the floor: {p:?}");
    assert!(p.x.abs() < 1e-3);
    assert!(began, "the landing was a contact.begin");
    let r: RigidBody = g.w.get(crate_).unwrap();
    assert!(r.sleeping, "and it fell asleep");
    assert_eq!(
        pos(&g, floor),
        Vec2::new(0.0, -0.5),
        "a fixed body never moves"
    );
}

#[test]
fn a_stack_stands_and_a_hinge_swings() {
    let mut g = game("stack");
    ground(&mut g);
    let boxes: Vec<Entity> = (0..6)
        .map(|i| {
            body(
                &mut g,
                at(0.0, 0.5 + i as f32 * 1.01),
                boxed(1.0, 1.0),
                RigidBody::default(),
            )
        })
        .collect();
    // A pendulum: a ball hinged to a point in the world, out to the side.
    let bob = body(
        &mut g,
        at(10.0, 6.0),
        Collider {
            radius: 0.25,
            ..Default::default()
        },
        RigidBody::default(),
    );
    let j = g.w.spawn();
    let kind = g.w.strings().intern("revolute");
    g.w.insert(
        j,
        Joint {
            a: bob,
            b: Entity::NULL,
            kind,
            anchor_a: Vec2::new(-3.0, 0.0),
            anchor_b: Vec2::new(7.0, 6.0),
            ..Default::default()
        },
    )
    .unwrap();
    let mut xs = Vec::new();
    for _ in 0..300 {
        g.s.step(&mut g.w, 1).unwrap();
        let p = pos(&g, bob);
        let r = ((p.x - 7.0).powi(2) + (p.y - 6.0).powi(2)).sqrt();
        assert!((r - 3.0).abs() < 0.05, "the bob stays on its circle: {r}");
        xs.push(p.x);
    }
    let top = pos(&g, boxes[5]);
    // Six boxes high, each contact a hair soft: within 2 cm of 5.5.
    assert!(
        top.x.abs() < 0.05 && (top.y - 5.5).abs() < 0.02,
        "the stack stands: {top:?}"
    );
    assert!(
        xs.iter().any(|&x| x < 7.0) && xs.iter().any(|&x| x > 9.0),
        "it swings"
    );
}

/// A scene with everything moving: a stack, a pendulum, a thrown ball.
fn busy(tag: &str) -> Game {
    let mut g = game(tag);
    ground(&mut g);
    for i in 0..5 {
        body(
            &mut g,
            at(i as f32 * 0.3, 1.0 + i as f32 * 1.1),
            boxed(1.0, 1.0),
            RigidBody::default(),
        );
    }
    let ball = body(
        &mut g,
        at(-6.0, 3.0),
        Collider {
            radius: 0.4,
            ..Default::default()
        },
        RigidBody {
            velocity: Vec2::new(8.0, 2.0),
            restitution: 0.6,
            ..Default::default()
        },
    );
    let kind = g.w.strings().intern("spring");
    let j = g.w.spawn();
    g.w.insert(
        j,
        Joint {
            a: ball,
            b: Entity::NULL,
            kind,
            anchor_b: Vec2::new(-6.0, 6.0),
            length: 2.0,
            stiffness: 30.0,
            damping: 0.5,
            ..Default::default()
        },
    )
    .unwrap();
    g
}

#[test]
fn the_same_run_reaches_the_same_world() {
    let mut a = busy("same-a");
    let mut b = busy("same-b");
    for t in 0..240 {
        a.s.step(&mut a.w, 1).unwrap();
        b.s.step(&mut b.w, 1).unwrap();
        assert_eq!(a.w.digest(), b.w.digest(), "tick {t}");
    }
}

#[test]
fn a_run_replayed_from_a_keyframe_is_the_run_it_replays() {
    let mut g = busy("keyframe");
    g.s.step(&mut g.w, 90).unwrap();
    let snap = g.s.snapshot(&g.w);
    let state: RigidState = g.store.borrow().state();
    let bytes = state.encode();
    let mut later = Vec::new();
    for _ in 0..120 {
        g.s.step(&mut g.w, 1).unwrap();
        later.push(g.w.digest());
    }
    // From the state in memory (a seek) and from its bytes (a replay file).
    for (how, st) in [
        ("memory", state.clone()),
        ("bytes", RigidState::decode(&bytes).unwrap()),
    ] {
        g.s.restore(&mut g.w, &snap);
        g.store.borrow_mut().restore(&st);
        for (t, want) in later.iter().enumerate() {
            g.s.step(&mut g.w, 1).unwrap();
            assert_eq!(g.w.digest(), *want, "{how}: tick {t} after the keyframe");
        }
    }
    // Without Rapier's own state the run parts, which is why keyframes
    // carry it.
    g.s.restore(&mut g.w, &snap);
    *g.store.borrow_mut() = RigidStore::new();
    let mut parted = false;
    for want in &later {
        g.s.step(&mut g.w, 1).unwrap();
        parted |= g.w.digest() != *want;
    }
    assert!(parted);
}

#[test]
fn a_throw_and_a_teleport_are_gameplays_to_make() {
    let mut g = game("throw");
    ground(&mut g);
    let b = body(&mut g, at(0.0, 0.5), boxed(1.0, 1.0), RigidBody::default());
    g.s.step(&mut g.w, 30).unwrap();
    g.w.update::<RigidBody, _>(b, |r| r.velocity = Vec2::new(0.0, 10.0))
        .unwrap();
    g.s.step(&mut g.w, 10).unwrap();
    // Ten ticks up at 10 under 9.81: 0.5 + 10/6 - 9.81/72.
    assert!(
        (pos(&g, b).y - 2.03).abs() < 0.05,
        "thrown up: {:?}",
        pos(&g, b)
    );
    g.w.update::<Transform, _>(b, |t| t.pos = Vec2::new(12.0, 8.0))
        .unwrap();
    g.s.step(&mut g.w, 1).unwrap();
    let p = pos(&g, b);
    // Teleported, still rising as it was.
    assert!(
        (p.x - 12.0).abs() < 1e-3 && (p.y - 8.0).abs() < 0.3,
        "teleported: {p:?}"
    );
}

#[test]
fn a_weak_joint_breaks_under_a_load() {
    let mut g = game("break");
    let weight = body(
        &mut g,
        at(0.0, 4.0),
        boxed(1.0, 1.0),
        RigidBody {
            density: 50.0,
            ..Default::default()
        },
    );
    let kind = g.w.strings().intern("rope");
    let j = g.w.spawn();
    g.w.insert(
        j,
        Joint {
            a: weight,
            b: Entity::NULL,
            kind,
            anchor_b: Vec2::new(0.0, 6.0),
            length: 2.0,
            break_force: 200.0,
            ..Default::default()
        },
    )
    .unwrap();
    let mut broke = Vec::new();
    for _ in 0..120 {
        g.s.step(&mut g.w, 1).unwrap();
        broke.extend(game_events(&g, "joint.broken"));
        g.w.drain_events();
    }
    assert_eq!(broke.len(), 1, "{broke:?}");
    assert!(g.w.get::<Joint>(j).unwrap().broken);
    assert!(pos(&g, weight).y < 0.0, "and it fell");
}

#[test]
fn a_moving_platform_of_the_kinematic_tier_pushes_a_box() {
    let mut g = game("push");
    ground(&mut g);
    let b = body(&mut g, at(3.0, 0.5), boxed(1.0, 1.0), RigidBody::default());
    let wall = g.w.spawn();
    g.w.insert(wall, at(0.0, 1.0)).unwrap();
    g.w.insert(wall, boxed(1.0, 2.0)).unwrap();
    g.w.insert(
        wall,
        KinematicBody {
            velocity: Vec2::new(3.0, 0.0),
            solid: true,
            ..Default::default()
        },
    )
    .unwrap();
    g.s.step(&mut g.w, 120).unwrap();
    let w = pos(&g, wall).x;
    let p = pos(&g, b).x;
    assert!(w > 5.0, "the solid moved on: {w}");
    assert!(
        p > w + 0.9,
        "and pushed the box ahead of it: box {p}, wall {w}"
    );
}

#[test]
fn a_box_lands_on_a_tilemaps_solid_tiles() {
    let mut g = game("tiles");
    let root = dir("tiles");
    let rel = "maps/floor.tilemap.toml";
    let mut m = MapFile::create(
        &root,
        rel,
        20,
        10,
        1.0,
        16,
        2,
        vec![LayerDef {
            name: "ground".into(),
            tileset: None,
            z: 0.0,
            collision: true,
        }],
    )
    .unwrap();
    let li = m.layer("ground").unwrap();
    // The bottom row, and a step two tiles high at columns 12 and up.
    m.fill(li, Rect::new(0, 9, 20, 1), 1).unwrap();
    m.fill(li, Rect::new(12, 7, 8, 2), 1).unwrap();
    m.save().unwrap();
    // A fresh game on the same root, which opens the map.
    let store = Rc::new(RefCell::new(RigidStore::new()));
    let tiles = Rc::new(RefCell::new(MapStore::new(&root)));
    let mut w = World::new();
    w.register::<Transform>().unwrap();
    w.register::<GlobalTransform>().unwrap();
    w.register::<Parent>().unwrap();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    argus_physics::install(&mut w, &mut s, tiles.clone()).unwrap();
    argus_rigid::install(&mut w, &mut s, tiles, store).unwrap();
    g.w = w;
    g.s = s;
    g.s.play(&g.w);
    let map = g.w.spawn();
    let file = g.w.strings().intern(rel);
    g.w.insert(map, at(0.0, 10.0)).unwrap();
    g.w.insert(
        map,
        Tilemap {
            map: file,
            visible: true,
        },
    )
    .unwrap();
    // Rows go down from the map's origin at y 10: the bottom row's top is
    // at y 1, the step's at y 3.
    let low = body(&mut g, at(5.0, 6.0), boxed(1.0, 1.0), RigidBody::default());
    let high = body(&mut g, at(15.0, 6.0), boxed(1.0, 1.0), RigidBody::default());
    let mut hit_map = false;
    for _ in 0..180 {
        g.s.step(&mut g.w, 1).unwrap();
        hit_map |= game_events(&g, "contact.begin")
            .iter()
            .any(|f| f.iter().any(|(_, v)| *v == Value::Entity(map)));
        g.w.drain_events();
    }
    assert!((pos(&g, low).y - 1.5).abs() < 0.02, "{:?}", pos(&g, low));
    assert!((pos(&g, high).y - 3.5).abs() < 0.02, "{:?}", pos(&g, high));
    assert!(hit_map, "the landing names the map");
}

#[test]
fn a_scene_without_rigid_bodies_pays_nothing() {
    let mut g = game("none");
    let e = g.w.spawn();
    g.w.insert(e, at(0.0, 0.0)).unwrap();
    g.w.insert(e, RigidWorld::default()).unwrap();
    g.s.step(&mut g.w, 10).unwrap();
    assert!(g.store.borrow().now().is_empty());
    let p = g.s.prof_systems(10).unwrap();
    let row = p.iter().find(|r| r.name == "physics.rigid").unwrap();
    assert!(row.mean_ms < 0.05, "{row:?}");
}

#[test]
fn rigid_bodies_are_in_the_shared_broadphase_and_the_overlap_pass() {
    let mut g = game("shared");
    ground(&mut g);
    let b = body(&mut g, at(0.0, 3.0), boxed(1.0, 1.0), RigidBody::default());
    // A trigger zone the box falls into: an overlap-tier collider.
    let zone = g.w.spawn();
    g.w.insert(zone, at(0.0, 1.0)).unwrap();
    g.w.insert(zone, boxed(2.0, 1.0)).unwrap();
    let mut overlapped = false;
    for _ in 0..90 {
        g.s.step(&mut g.w, 1).unwrap();
        overlapped |= !game_events(&g, "overlap.begin").is_empty();
        g.w.drain_events();
    }
    assert!(overlapped, "the overlap pass saw the rigid body");
    let found: Vec<Entity> = g.w.within(0.0, 0.5, 0.2).into_iter().map(|x| x.0).collect();
    assert!(found.contains(&b), "within finds it where the step left it");
    let order: Vec<String> =
        g.s.systems(&g.w.registry().clone())
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .filter(|n| n.starts_with("physics."))
            .collect();
    assert_eq!(
        order,
        vec![
            "physics.control",
            "physics.move",
            "physics.rigid",
            "physics.overlap"
        ]
    );
}

/// What the module costs at a physics toy's scale: 500 boxes dropped in a
/// grid into a walled pit, measured once they have piled up. Release only.
#[test]
#[ignore = "release-only perf gate; run with --release -- --ignored"]
fn perf_five_hundred_boxes_in_a_pile() {
    let mut g = game("perf");
    ground(&mut g);
    for x in [-8.5f32, 8.5] {
        body(
            &mut g,
            at(x, 10.0),
            boxed(1.0, 20.0),
            RigidBody {
                fixed: true,
                ..Default::default()
            },
        );
    }
    for i in 0..500 {
        let (c, r) = (i % 16, i / 16);
        body(
            &mut g,
            at(-7.5 + c as f32, 1.0 + r as f32 * 1.05),
            boxed(0.9, 0.9),
            RigidBody::default(),
        );
    }
    let run = |g: &mut Game, n: usize| {
        let mut ms = Vec::new();
        for _ in 0..n {
            let t = std::time::Instant::now();
            g.s.step(&mut g.w, 1).unwrap();
            ms.push(t.elapsed().as_secs_f64() * 1e3);
        }
        ms.sort_by(f64::total_cmp);
        (
            ms.iter().sum::<f64>() / ms.len() as f64,
            ms[ms.len() * 95 / 100],
        )
    };
    // Every box awake: the drop and the first collisions.
    let (fall_mean, fall_p95) = run(&mut g, 120);
    g.s.step(&mut g.w, 180).unwrap();
    let (mean, p95) = run(&mut g, 120);
    println!("500 boxes falling: tick mean {fall_mean:.3} ms, p95 {fall_p95:.3} ms");
    let awake =
        g.w.query()
            .has(g.w.component_id("RigidBody").unwrap())
            .entities()
            .into_iter()
            .filter(|&e| !g.w.get::<RigidBody>(e).unwrap().sleeping)
            .count();
    println!("500 boxes piled: tick mean {mean:.3} ms, p95 {p95:.3} ms, {awake} awake");
    assert!(
        p95 < 8.0 && fall_p95 < 16.0,
        "tick p95 {p95} ms piled, {fall_p95} ms falling"
    );
}
