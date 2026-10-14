//! 2D staples item 0: raycasts, shape casts, and overlaps against tile
//! cells (solid, slopes, one-way platforms) and colliders (by layer, the
//! asker left out, inactive ones not there).

mod common;

use argus_ecs::{Transform, Vec2};
use argus_physics::geom::{Round, V};
use argus_physics::query::{overlap, raycast, shape_cast, Filter, Target};
use argus_physics::Collider;
use common::Rig;

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

/// A room 12 wide: a floor, a wall at x 10, a 45-degree ramp up from x 4,
/// and a one-way ledge at y 4.
fn room() -> Rig {
    Rig::new(
        "query",
        1.0,
        &[
            "..........#.",
            "..........#.",
            "..----....#.",
            "..........#.",
            "....../...#.",
            "...../#...#.",
            "..../##...#.",
            "############",
        ],
    )
}

fn collider(r: &mut Rig, x: f32, y: f32, c: Collider) -> argus_ecs::Entity {
    let e = r.world.spawn();
    r.world.insert(e, Transform::at(x, y)).unwrap();
    r.world.insert(e, c).unwrap();
    e
}

#[test]
fn a_ray_meets_the_nearest_tile_and_says_where_and_which_way_it_faces() {
    let r = room();
    let f = Filter::default();
    // Along the floor's top, right, at y 1.5: the ramp's slope at x 4.5.
    let h = raycast(
        &r.world,
        &mut r.store.borrow_mut(),
        V::new(0.5, 1.5),
        V::new(11.5, 1.5),
        &f,
        false,
    )
    .unwrap();
    let h = h[0];
    assert!(
        matches!(h.target, Target::Tile { col: 4, row: 6, .. }),
        "{h:?}"
    );
    assert!(close(h.point.x, 4.5) && close(h.point.y, 1.5), "{h:?}");
    assert!(
        close(h.normal.x, -std::f32::consts::FRAC_1_SQRT_2)
            && close(h.normal.y, std::f32::consts::FRAC_1_SQRT_2),
        "the slope faces up and left: {h:?}"
    );
    // Higher, over the ramp: the wall at x 10.
    let h = raycast(
        &r.world,
        &mut r.store.borrow_mut(),
        V::new(0.5, 4.5),
        V::new(11.5, 4.5),
        &f,
        false,
    )
    .unwrap()[0];
    assert!(
        close(h.point.x, 10.0) && h.normal == V::new(-1.0, 0.0),
        "{h:?}"
    );
    assert!(close(h.distance, 9.5) && close(h.fraction, 9.5 / 11.0));
    // Straight down onto the floor.
    let h = raycast(
        &r.world,
        &mut r.store.borrow_mut(),
        V::new(1.5, 6.5),
        V::new(1.5, -3.0),
        &f,
        false,
    )
    .unwrap()[0];
    assert!(
        close(h.point.y, 1.0) && h.normal == V::new(0.0, 1.0),
        "{h:?}"
    );
    // Every cell the line crosses, nearest first.
    let all = raycast(
        &r.world,
        &mut r.store.borrow_mut(),
        V::new(-1.0, 0.5),
        V::new(13.0, 0.5),
        &f,
        true,
    )
    .unwrap();
    assert_eq!(all.len(), 12, "the whole floor row");
    assert!(all.windows(2).all(|w| w[0].fraction <= w[1].fraction));
    // Out of the map and missing everything: none.
    let none = raycast(
        &r.world,
        &mut r.store.borrow_mut(),
        V::new(0.5, 9.0),
        V::new(9.5, 9.0),
        &f,
        false,
    )
    .unwrap();
    assert!(none.is_empty());
}

#[test]
fn a_one_way_platform_stops_only_what_comes_down_onto_it() {
    let r = room();
    let f = Filter::default();
    let down = raycast(
        &r.world,
        &mut r.store.borrow_mut(),
        V::new(3.5, 6.5),
        V::new(3.5, 2.0),
        &f,
        false,
    )
    .unwrap();
    assert!(
        close(down[0].point.y, 6.0) && matches!(down[0].target, Target::Tile { row: 2, .. }),
        "{down:?}"
    );
    let up = raycast(
        &r.world,
        &mut r.store.borrow_mut(),
        V::new(3.5, 3.0),
        V::new(3.5, 7.5),
        &f,
        false,
    )
    .unwrap();
    assert!(up.is_empty(), "up through it: {up:?}");
    let across = raycast(
        &r.world,
        &mut r.store.borrow_mut(),
        V::new(0.5, 5.5),
        V::new(9.5, 5.5),
        &f,
        false,
    )
    .unwrap();
    assert!(across.is_empty(), "across it: {across:?}");
    let ignored = Filter {
        one_way: false,
        ..Filter::default()
    };
    let through = raycast(
        &r.world,
        &mut r.store.borrow_mut(),
        V::new(3.5, 6.5),
        V::new(3.5, 2.0),
        &ignored,
        false,
    )
    .unwrap();
    assert!(through.is_empty(), "with one_way off it is not there");
}

#[test]
fn colliders_count_by_layer_the_asker_is_left_out_and_inactive_ones_are_gone() {
    let mut r = room();
    let me = collider(
        &mut r,
        1.0,
        3.0,
        Collider {
            size: Vec2::new(1.0, 1.0),
            ..Collider::default()
        },
    );
    let enemy = collider(
        &mut r,
        6.0,
        3.0,
        Collider {
            radius: 0.5,
            layer: 2,
            ..Collider::default()
        },
    );
    let ghost = collider(
        &mut r,
        4.0,
        3.0,
        Collider {
            size: Vec2::new(1.0, 1.0),
            layer: 4,
            ..Collider::default()
        },
    );
    let from = V::new(1.0, 3.0);
    let to = V::new(9.0, 3.0);
    let f = Filter {
        ignore: vec![me],
        tiles: false,
        ..Filter::default()
    };
    let h = raycast(&r.world, &mut r.store.borrow_mut(), from, to, &f, false).unwrap();
    assert_eq!(h[0].target, Target::Entity(ghost), "{h:?}");
    assert!(close(h[0].point.x, 3.5));
    let only_enemies = Filter {
        mask: 2,
        ..f.clone()
    };
    let h = raycast(
        &r.world,
        &mut r.store.borrow_mut(),
        from,
        to,
        &only_enemies,
        false,
    )
    .unwrap();
    assert_eq!(h[0].target, Target::Entity(enemy));
    assert!(
        close(h[0].point.x, 5.5) && close(h[0].normal.x, -1.0),
        "{h:?}"
    );
    // Starting inside the asker without ignoring it: met at once, from inside.
    let h = raycast(
        &r.world,
        &mut r.store.borrow_mut(),
        from,
        to,
        &Filter {
            tiles: false,
            ..Filter::default()
        },
        false,
    )
    .unwrap();
    assert_eq!(h[0].target, Target::Entity(me));
    assert_eq!((h[0].fraction, h[0].normal), (0.0, V::default()));
    // Switched off, the ghost is not there.
    r.world.set_active(ghost, false).unwrap();
    let h = raycast(&r.world, &mut r.store.borrow_mut(), from, to, &f, false).unwrap();
    assert_eq!(h[0].target, Target::Entity(enemy));
}

#[test]
fn a_moving_shape_stops_where_it_first_touches_and_overlaps_say_what_is_there() {
    let mut r = room();
    let f = Filter::default();
    // A circle of radius 0.5 dropped from y 6 at x 1.5 lands on the floor.
    let ball = Round::circle(V::default(), 0.5);
    let h = shape_cast(
        &r.world,
        &mut r.store.borrow_mut(),
        &ball,
        V::new(1.5, 6.0),
        V::new(1.5, -2.0),
        &f,
    )
    .unwrap()
    .unwrap();
    assert!(
        close(h.position.y, 1.5) && close(h.point.y, 1.0) && h.normal == V::new(0.0, 1.0),
        "{h:?}"
    );
    // A box 1 wide pushed right at y 4.5 stops with its face at the wall.
    let crate_ = Round::rect(V::default(), 0.5, 0.5);
    let h = shape_cast(
        &r.world,
        &mut r.store.borrow_mut(),
        &crate_,
        V::new(7.0, 4.5),
        V::new(11.0, 4.5),
        &f,
    )
    .unwrap()
    .unwrap();
    assert!(
        close(h.position.x, 9.5) && h.normal == V::new(-1.0, 0.0),
        "{h:?}"
    );
    // Clear all the way: none.
    let clear = shape_cast(
        &r.world,
        &mut r.store.borrow_mut(),
        &ball,
        V::new(7.5, 5.0),
        V::new(7.5, 1.6),
        &f,
    )
    .unwrap();
    assert!(clear.is_none(), "{clear:?}");
    // A box dropped onto the one-way ledge lands on it; one rising through it does not.
    let h = shape_cast(
        &r.world,
        &mut r.store.borrow_mut(),
        &crate_,
        V::new(3.0, 7.0),
        V::new(3.0, 3.0),
        &f,
    )
    .unwrap()
    .unwrap();
    assert!(close(h.position.y, 6.5), "{h:?}");
    assert!(shape_cast(
        &r.world,
        &mut r.store.borrow_mut(),
        &crate_,
        V::new(3.0, 3.0),
        V::new(3.0, 7.0),
        &f
    )
    .unwrap()
    .is_none());

    // Overlaps: a circle on the floor's corner meets the cells it reaches
    // into, the one-way ledge never.
    let e = collider(
        &mut r,
        8.0,
        1.5,
        Collider {
            size: Vec2::new(1.0, 1.0),
            ..Collider::default()
        },
    );
    let o = overlap(
        &r.world,
        &mut r.store.borrow_mut(),
        &Round::circle(V::default(), 0.6),
        V::new(8.0, 1.5),
        &f,
    )
    .unwrap();
    let ents: Vec<_> = o
        .iter()
        .filter(|h| matches!(h.target, Target::Entity(_)))
        .collect();
    assert_eq!(ents.len(), 1);
    assert_eq!(ents[0].target, Target::Entity(e));
    assert!(
        o.iter()
            .any(|h| matches!(h.target, Target::Tile { col: 8, row: 7, .. })),
        "the floor under it: {o:?}"
    );
    let ledge = overlap(
        &r.world,
        &mut r.store.borrow_mut(),
        &Round::circle(V::default(), 0.4),
        V::new(3.5, 5.5),
        &f,
    )
    .unwrap();
    assert!(ledge.is_empty(), "inside the one-way ledge: {ledge:?}");
    // A point is a radius-0 circle.
    let wall = overlap(
        &r.world,
        &mut r.store.borrow_mut(),
        &Round::circle(V::default(), 0.0),
        V::new(10.5, 3.5),
        &f,
    )
    .unwrap();
    assert!(
        matches!(wall[0].target, Target::Tile { col: 10, .. }),
        "{wall:?}"
    );
}
