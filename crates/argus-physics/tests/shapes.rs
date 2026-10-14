//! 2D staples item 2: capsule and polygon colliders. The overlap tier and
//! the queries meet them by their shapes, not their boxes; a kinematic
//! body moves as the box around its shape.

mod common;

use argus_ecs::{Entity, Transform, Value, Vec2};
use argus_physics::geom::V;
use argus_physics::query::{raycast, Filter, Target};
use argus_physics::{CharacterController, Collider, KinematicBody, ShapeKind};
use common::*;

fn open(name: &str) -> Rig {
    let mut art: Vec<String> = vec![".".repeat(40); 19];
    art.push("#".repeat(40));
    let refs: Vec<&str> = art.iter().map(String::as_str).collect();
    Rig::new(name, 1.0, &refs)
}

fn spawn(r: &mut Rig, x: f32, y: f32, c: Collider) -> Entity {
    let e = r.world.spawn();
    r.world.insert(e, Transform::at(x, y)).unwrap();
    r.world.insert(e, c).unwrap();
    e
}

/// A triangle at (10, 10): corners (-2, -1), (2, -1), (0, 2) around it.
fn triangle(r: &mut Rig) -> Entity {
    let e = spawn(r, 10.0, 10.0, Collider::default());
    let pts = [(-2.0, -1.0), (2.0, -1.0), (0.0, 2.0)]
        .iter()
        .map(|&(x, y)| Value::Vec2(Vec2::new(x, y)))
        .collect();
    r.world
        .set_field(e, "Collider.points", Value::List(pts))
        .unwrap();
    e
}

/// A capsule at (20, 10), radius 0.5 and 3 tall: a segment from y 9 to 11.
fn capsule(r: &mut Rig) -> Entity {
    spawn(
        r,
        20.0,
        10.0,
        Collider {
            radius: 0.5,
            height: 3.0,
            ..Default::default()
        },
    )
}

#[test]
fn shapes_are_what_their_fields_say() {
    let mut r = open("shapes-kind");
    let t = triangle(&mut r);
    let c = capsule(&mut r);
    let get = |r: &Rig, e| r.world.get::<Collider>(e).unwrap();
    assert_eq!(get(&r, t).kind(), ShapeKind::Polygon);
    assert_eq!(get(&r, c).kind(), ShapeKind::Capsule);
    assert_eq!(get(&r, t).corners(&r.world).len(), 3);
    let short = Collider {
        radius: 0.5,
        height: 1.0,
        ..Default::default()
    };
    assert_eq!(short.kind(), ShapeKind::Circle, "no taller than its width");
    assert_eq!(Collider::default().kind(), ShapeKind::Box);
    // The box a body moves as: the triangle's, centred half a unit up.
    let b = get(&r, t).boxed(&r.world);
    assert_eq!(
        (b.size, b.offset),
        (Vec2::new(4.0, 3.0), Vec2::new(0.0, 0.5))
    );
}

#[test]
fn the_overlap_tier_meets_a_capsule_and_a_polygon_by_their_shapes() {
    let mut r = open("shapes-overlap");
    let tri = triangle(&mut r);
    let cap = capsule(&mut r);
    let probe = spawn(
        &mut r,
        0.0,
        0.0,
        Collider {
            radius: 0.05,
            ..Default::default()
        },
    );
    let at = |r: &mut Rig, x: f32, y: f32| -> (bool, bool) {
        r.world
            .update::<Transform, _>(probe, |t| t.pos = Vec2::new(x, y))
            .unwrap();
        r.clear();
        r.tick(1);
        let touched = |r: &Rig, e: Entity| {
            r.named("overlap.begin").iter().any(|ev| {
                field(ev, "entity") == Some(&Value::Entity(e))
                    || field(ev, "other") == Some(&Value::Entity(e))
            })
        };
        (touched(r, tri), touched(r, cap))
    };
    // Inside the triangle's box, outside the triangle: its right edge is
    // at x 10.4 at this height.
    assert_eq!(at(&mut r, 11.6, 11.4), (false, false));
    assert_eq!(at(&mut r, 10.0, 11.0), (true, false), "inside it");
    // Inside the capsule's box, past its rounded top: 0.64 from the top
    // of the segment, more than its radius and the probe's.
    assert_eq!(at(&mut r, 20.45, 11.45), (false, false));
    assert_eq!(at(&mut r, 20.3, 11.3), (false, true), "inside its end");
}

#[test]
fn queries_meet_a_capsule_and_a_polygon() {
    let mut r = open("shapes-query");
    let tri = triangle(&mut r);
    let cap = capsule(&mut r);
    let f = Filter {
        tiles: false,
        ..Filter::default()
    };
    // Down onto the triangle's left face, from (8, 9) to (10, 12).
    let hits = raycast(
        &r.world,
        &mut r.store.borrow_mut(),
        V::new(9.0, 20.0),
        V::new(9.0, 0.0),
        &f,
        false,
    )
    .unwrap();
    let h = hits[0];
    assert_eq!(h.target, Target::Entity(tri));
    assert!((h.point.y - 10.5).abs() < 1e-4, "{h:?}");
    let n = V::new(-3.0, 2.0).unit();
    assert!((h.normal - n).len() < 1e-4, "{h:?}");
    // Across to the capsule's side.
    let hits = raycast(
        &r.world,
        &mut r.store.borrow_mut(),
        V::new(15.0, 10.0),
        V::new(25.0, 10.0),
        &f,
        false,
    )
    .unwrap();
    assert_eq!(hits[0].target, Target::Entity(cap));
    assert!((hits[0].point.x - 19.5).abs() < 1e-4, "{:?}", hits[0]);
    assert!((hits[0].normal - V::new(-1.0, 0.0)).len() < 1e-4);
}

#[test]
fn a_kinematic_capsule_moves_as_the_box_around_it() {
    let mut r = open("shapes-body");
    let e = spawn(
        &mut r,
        5.0,
        6.0,
        Collider {
            radius: 0.4,
            height: 1.6,
            ..Default::default()
        },
    );
    r.world.insert(e, KinematicBody::default()).unwrap();
    r.world.insert(e, CharacterController::default()).unwrap();
    r.tick(120);
    // The floor's top is y 1; the capsule's box is 1.6 tall about its centre.
    let y = r.pos(e).y;
    assert!((y - 1.8).abs() < 1e-3, "{y}");
    assert!(r.body(e).grounded);
}
