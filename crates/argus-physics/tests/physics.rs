//! §12 physics tiers through the systems: the kinematic controller against
//! tiles (walls, jumps, coyote time, jump buffering, corner correction,
//! step-up, slopes, one-way platforms), moving platforms, no tunnelling,
//! overlap contacts, and determinism.

mod common;

use argus_ecs::{Transform, Value, Vec2};
use argus_physics::{Collider, KinematicBody};
use common::*;

const CLOSE: f32 = 2e-3;

#[test]
fn lands_runs_and_stops_at_walls() {
    let mut r = Rig::new("walls", 1.0, &["........", "#......#", "########"]);
    let p = r.player(4.0, 1.5);
    r.settle(p, 60);
    assert!((r.feet(p) - 1.0).abs() < 1e-4, "feet {}", r.feet(p));
    assert_eq!(r.named("body.landed").len(), 1);
    // Full speed after run_speed / ground_accel = 0.1 s.
    r.input(p, 1.0, 0.0, false);
    r.tick(6);
    assert!(
        (r.body(p).velocity.x - 7.0).abs() < 1e-3,
        "{:?}",
        r.body(p).velocity
    );
    r.tick(60);
    assert!(
        (r.pos(p).x - 6.6).abs() < 1e-4,
        "stops flush at the wall: {}",
        r.pos(p).x
    );
    let b = r.body(p);
    assert_eq!((b.wall, b.velocity.x, b.grounded), (1, 0.0, true));
    let walls = r.named("body.wall");
    assert_eq!(walls.len(), 1, "one event while it presses on");
    assert_eq!(fnum(walls[0], "side"), 1.0);
    r.input(p, -1.0, 0.0, false);
    r.tick(120);
    assert!((r.pos(p).x - 1.4).abs() < 1e-4, "{}", r.pos(p).x);
    assert_eq!(r.body(p).wall, -1);
    assert!((r.feet(p) - 1.0).abs() < 1e-4);
}

fn open(name: &str, rows: usize, cols: usize) -> Rig {
    let mut art: Vec<String> = vec![".".repeat(cols); rows - 1];
    art.push("#".repeat(cols));
    let refs: Vec<&str> = art.iter().map(String::as_str).collect();
    Rig::new(name, 1.0, &refs)
}

/// Highest feet over `n` ticks.
fn apex(r: &mut Rig, p: argus_ecs::Entity, n: u64) -> f32 {
    let mut top = f32::MIN;
    for _ in 0..n {
        r.tick(1);
        top = top.max(r.feet(p));
    }
    top
}

#[test]
fn a_jump_reaches_its_declared_height_and_letting_go_cuts_it() {
    let mut r = open("jump", 9, 12);
    let p = r.player(5.0, 1.0);
    r.settle(p, 10);
    r.clear();
    r.input(p, 0.0, 0.0, true);
    let top = apex(&mut r, p, 90);
    // 2h/t = 16 take-off, 2h/t^2 = 40 gravity, 24 ticks to the top.
    assert!((top - 1.0 - 3.2).abs() < 0.01, "apex {}", top - 1.0);
    let j = r.named("body.jumped");
    assert_eq!(j.len(), 1);
    assert!(!fbool(j[0], "coyote") && !fbool(j[0], "buffered"));
    let landed = r.named("body.landed");
    assert_eq!(landed.len(), 1);
    assert!(
        fnum(landed[0], "speed") > 10.0,
        "falls faster than it rose: {}",
        fnum(landed[0], "speed")
    );
    assert!(r.body(p).grounded);

    // Let go after four ticks: a short hop.
    r.input(p, 0.0, 0.0, false);
    r.tick(1);
    r.input(p, 0.0, 0.0, true);
    r.tick(4);
    r.input(p, 0.0, 0.0, false);
    let short = apex(&mut r, p, 60) - 1.0;
    assert!(short > 1.2 && short < 1.7, "short hop {short}");
}

fn ledge() -> Rig {
    Rig::new(
        "coyote",
        1.0,
        &[
            "..............",
            "..............",
            "..............",
            "#####.........",
            "#####.........",
            "##############",
        ],
    )
}

/// Run off the ledge; return after the tick on which it left the ground.
fn run_off(r: &mut Rig) -> argus_ecs::Entity {
    let p = r.player(2.0, 3.0);
    r.settle(p, 10);
    r.input(p, 1.0, 0.0, false);
    for _ in 0..120 {
        r.tick(1);
        if !r.body(p).grounded {
            return p;
        }
    }
    panic!("never left the ledge");
}

#[test]
fn coyote_time_allows_a_late_jump_and_not_a_later_one() {
    let mut r = ledge();
    let p = run_off(&mut r);
    assert!(
        r.pos(p).x - 0.4 > 5.0 - 1e-3,
        "left only when the whole box was past the edge"
    );
    r.tick(2);
    r.clear();
    r.input(p, 1.0, 0.0, true);
    r.tick(1);
    let j = r.named("body.jumped");
    assert_eq!(j.len(), 1, "three ticks after leaving, within 0.1 s");
    assert!(fbool(j[0], "coyote"));
    assert!(r.body(p).velocity.y > 10.0);

    let mut r = ledge();
    let p = run_off(&mut r);
    r.tick(7);
    r.clear();
    r.input(p, 1.0, 0.0, true);
    r.tick(3);
    assert!(
        r.named("body.jumped").is_empty(),
        "eight ticks is past coyote time"
    );
    assert!(r.body(p).velocity.y < 0.0);
}

#[test]
fn a_jump_pressed_just_before_landing_fires_on_landing() {
    let mut r = open("buffer", 12, 10);
    let p = r.player(5.0, 8.0);
    while r.feet(p) > 2.4 {
        r.tick(1);
    }
    assert!(!r.body(p).grounded);
    r.input(p, 0.0, 0.0, true);
    for _ in 0..30 {
        r.tick(1);
        if !r.named("body.jumped").is_empty() {
            break;
        }
    }
    let j = r.named("body.jumped");
    assert_eq!(j.len(), 1, "the buffered press jumps");
    assert!(fbool(j[0], "buffered") && !fbool(j[0], "coyote"));
    let landed = r.named("body.landed");
    assert_eq!(landed.len(), 1);
    assert!(j[0].0 - landed[0].0 <= 1, "the tick after landing");

    // Pressed long before landing: forgotten.
    let mut r = open("buffer-late", 12, 10);
    let p = r.player(5.0, 8.0);
    r.tick(2);
    r.input(p, 0.0, 0.0, true);
    r.settle(p, 120);
    r.tick(10);
    assert!(r.named("body.jumped").is_empty());
}

fn ceiling() -> Rig {
    Rig::new(
        "corner",
        1.0,
        &[
            "..........",
            "....###...",
            "..........",
            "..........",
            "..........",
            "##########",
        ],
    )
}

#[test]
fn corner_correction_slides_a_head_around_a_corner_and_lifts_feet_onto_a_ledge() {
    // Head overlaps the block's left corner by 0.15: slid 0.15 left, on up.
    let mut r = ceiling();
    let p = r.player(3.75, 1.0);
    r.settle(p, 10);
    r.input(p, 0.0, 0.0, true);
    let top = apex(&mut r, p, 40);
    let c = r.named("body.corner_corrected");
    assert_eq!(c.len(), 1, "{:?}", r.events);
    assert!(
        (fnum(c[0], "dx") + 0.15).abs() < 1e-3,
        "{}",
        fnum(c[0], "dx")
    );
    assert!(r.named("body.ceiling").is_empty());
    assert!(top + 1.6 > 4.5, "the head passed the block: {}", top + 1.6);

    // Overlapping by 0.35 is too much: a bonk.
    let mut r = ceiling();
    let p = r.player(3.95, 1.0);
    r.settle(p, 10);
    r.input(p, 0.0, 0.0, true);
    let top = apex(&mut r, p, 40);
    assert!(r.named("body.corner_corrected").is_empty());
    assert_eq!(r.named("body.ceiling").len(), 1);
    assert!(top + 1.6 <= 4.0 + 1e-3, "{}", top + 1.6);

    // In the air, feet 0.1 under a ledge's top: lifted onto it.
    let mut r = Rig::new(
        "ledge",
        1.0,
        &["........", "........", ".....###", "........"],
    );
    let p = r.player(4.55, 1.9);
    r.world
        .update::<KinematicBody, _>(p, |k| k.velocity = Vec2::new(7.0, 0.0))
        .unwrap();
    r.input(p, 1.0, 0.0, false);
    r.tick(1);
    let c = r.named("body.corner_corrected");
    assert_eq!(c.len(), 1, "{:?}", r.events);
    assert!((fnum(c[0], "dy") - 0.1).abs() < 1e-3);
    assert!(
        (r.feet(p) - 2.0).abs() < 1e-4,
        "on the ledge: {}",
        r.feet(p)
    );
    assert!(r.body(p).grounded);
}

#[test]
fn small_ledges_are_stepped_onto_and_taller_ones_are_walls() {
    // Quarter-unit tiles: a step of 0.25 (under step_height 0.3), then 0.5.
    let mut art: Vec<String> = vec![".".repeat(40); 7];
    art.push(format!("{}{}", ".".repeat(24), "#".repeat(16)));
    art.push(format!("{}{}", ".".repeat(24), "#".repeat(16)));
    art.push(format!("{}{}", ".".repeat(16), "#".repeat(24)));
    art.push("#".repeat(40));
    let refs: Vec<&str> = art.iter().map(String::as_str).collect();
    let mut r = Rig::new("step", 0.25, &refs);
    let p = r.player(2.0, 0.25);
    r.settle(p, 10);
    r.clear();
    r.input(p, 1.0, 0.0, false);
    let mut left_ground = false;
    for _ in 0..120 {
        r.tick(1);
        left_ground |= !r.body(p).grounded;
    }
    assert!(!left_ground, "stepping up never leaves the ground");
    assert!((r.feet(p) - 0.5).abs() < 1e-4, "on the step: {}", r.feet(p));
    assert!(
        (r.pos(p).x - 5.6).abs() < 1e-4,
        "stopped by the taller ledge: {}",
        r.pos(p).x
    );
    assert_eq!(r.body(p).wall, 1);
    assert!(r.named("body.jumped").is_empty());
}

/// Walk across, checking the feet against `expect(centre x)` every tick.
fn walk(r: &mut Rig, p: argus_ecs::Entity, dir: f32, until: f32, expect: impl Fn(f32) -> f32) {
    r.input(p, dir, 0.0, false);
    let mut last = (r.pos(p).x, r.feet(p));
    for _ in 0..400 {
        r.tick(1);
        let (x, f) = (r.pos(p).x, r.feet(p));
        let b = r.body(p);
        assert!(b.grounded, "left the ground at x {x}, feet {f}");
        assert!(
            (f - expect(x)).abs() < CLOSE,
            "at x {x}: feet {f}, expected {}",
            expect(x)
        );
        assert!(
            (f - last.1).abs() <= (x - last.0).abs() + CLOSE,
            "popped at x {x}: {} to {f}",
            last.1
        );
        assert_eq!(b.wall, 0, "walked into a wall at x {x}");
        last = (x, f);
        if (dir > 0.0 && x >= until) || (dir < 0.0 && x <= until) {
            return;
        }
    }
    panic!("never got to {until}");
}

#[test]
fn slopes_are_walked_up_and_down_without_leaving_the_ground_or_popping() {
    let mut r = Rig::new(
        "slope45",
        1.0,
        &[
            "............",
            "............",
            "....../#####",
            "...../######",
            "..../#######",
            "############",
        ],
    );
    let p = r.player(2.0, 1.0);
    r.settle(p, 10);
    let ramp = |x: f32| (1.0 + (x - 4.0)).clamp(1.0, 4.0);
    walk(&mut r, p, 1.0, 9.0, ramp);
    walk(&mut r, p, -1.0, 2.0, ramp);
    assert!(r.named("body.landed").len() == 1, "never left the ground");
    assert!(r.named("body.wall").is_empty());

    let mut r = Rig::new(
        "slope22",
        1.0,
        &[
            "................",
            "................",
            "......ab########",
            "################",
        ],
    );
    let p = r.player(3.0, 1.0);
    r.settle(p, 10);
    let ramp = |x: f32| (1.0 + 0.5 * (x - 6.0)).clamp(1.0, 2.0);
    walk(&mut r, p, 1.0, 10.0, ramp);
    assert!((r.body(p).slope).abs() < 1e-6, "flat on the platform");
    walk(&mut r, p, -1.0, 7.0, ramp);
    assert!(
        (r.body(p).slope - 0.5).abs() < 1e-6,
        "on the rise: {}",
        r.body(p).slope
    );
}

#[test]
fn one_way_platforms_are_jumped_through_landed_on_and_dropped_through() {
    let mut r = Rig::new(
        "oneway",
        1.0,
        &[
            "..........",
            "..........",
            "..........",
            "...----...",
            "..........",
            "..........",
            "##########",
        ],
    );
    let p = r.player(5.0, 1.0);
    r.settle(p, 10);
    r.clear();
    r.input(p, 0.0, 0.0, true);
    for _ in 0..90 {
        r.tick(1);
    }
    assert!(r.named("body.ceiling").is_empty(), "no bonk from below");
    assert!(
        (r.feet(p) - 4.0).abs() < 1e-4,
        "landed on top: {}",
        r.feet(p)
    );
    let b = r.body(p);
    assert!(b.grounded && b.on_one_way);
    // Down and jump: through it, to the floor.
    r.input(p, 0.0, -1.0, false);
    r.tick(1);
    r.clear();
    r.input(p, 0.0, -1.0, true);
    r.tick(1);
    assert_eq!(r.named("body.dropped").len(), 1);
    assert!(r.named("body.jumped").is_empty());
    r.settle(p, 120);
    assert!(
        (r.feet(p) - 1.0).abs() < 1e-4,
        "on the floor: {}",
        r.feet(p)
    );
}

#[test]
fn moving_platforms_carry_riders_and_push_what_they_hit() {
    let mut art: Vec<String> = vec![format!("#{}#", ".".repeat(28)); 9];
    art.push("#".repeat(30));
    let refs: Vec<&str> = art.iter().map(String::as_str).collect();

    // Carried sideways, then up, then down, never leaving the platform.
    let mut r = Rig::new("carry", 1.0, &refs);
    let plat = r.solid(8.0, 3.0, 3.0, 0.5, Vec2::new(2.0, 0.0), false);
    let p = r.player(8.0, 3.6);
    r.settle(p, 30);
    assert_eq!(r.body(p).ground, plat);
    assert!((r.feet(p) - 3.25).abs() < 1e-4);
    let (px, sx) = (r.pos(p).x, r.pos(plat).x);
    r.tick(30);
    assert!((r.pos(p).x - px - 1.0).abs() < 1e-3 && (r.pos(plat).x - sx - 1.0).abs() < 1e-3);
    r.world
        .update::<KinematicBody, _>(plat, |k| k.velocity = Vec2::new(0.0, 1.5))
        .unwrap();
    r.clear();
    r.tick(30);
    assert!(
        (r.feet(p) - 4.0).abs() < 1e-3,
        "carried up with it: {}",
        r.feet(p)
    );
    r.world
        .update::<KinematicBody, _>(plat, |k| k.velocity = Vec2::new(0.0, -3.0))
        .unwrap();
    r.tick(20);
    assert!((r.feet(p) - 3.0).abs() < 1e-3, "and down: {}", r.feet(p));
    assert!(
        r.body(p).grounded && r.named("body.landed").is_empty(),
        "never left it"
    );

    // Jumping off keeps the platform's speed.
    r.world
        .update::<KinematicBody, _>(plat, |k| k.velocity = Vec2::new(2.0, 0.0))
        .unwrap();
    r.tick(1);
    r.input(p, 0.0, 0.0, true);
    r.tick(1);
    assert!(r.body(p).velocity.x > 1.5, "{:?}", r.body(p).velocity);

    // A block sliding along the floor pushes the player, then crushes it
    // against the wall.
    let mut r = Rig::new("push", 1.0, &refs);
    let p = r.player(10.0, 1.0);
    r.settle(p, 10);
    let block = r.solid(12.0, 2.0, 1.0, 2.0, Vec2::new(-4.0, 0.0), false);
    r.tick(30);
    assert!(r.pos(p).x < 10.0 - 0.5, "pushed: {}", r.pos(p).x);
    assert!(r.named("body.crushed").is_empty());
    r.tick(120);
    let c = r.named("body.crushed");
    assert!(!c.is_empty(), "squeezed against the wall");
    assert!(matches!(field(c[0], "by"), Some(Value::Entity(b)) if *b == block));
}

#[test]
fn fast_bodies_stop_at_walls_instead_of_passing_through() {
    let mut r = Rig::new(
        "tunnel",
        1.0,
        &[
            "..........#.....",
            "..........#.....",
            "................",
            "################",
        ],
    );
    let e = r.world.spawn();
    r.world.insert(e, Transform::at(2.0, 3.5)).unwrap();
    r.world
        .insert(
            e,
            Collider {
                size: Vec2::new(0.5, 0.5),
                ..Default::default()
            },
        )
        .unwrap();
    r.world
        .insert(
            e,
            KinematicBody {
                velocity: Vec2::new(600.0, 0.0),
                ..Default::default()
            },
        )
        .unwrap();
    r.tick(1);
    assert!((r.pos(e).x - 9.75).abs() < 1e-4, "{}", r.pos(e).x);
    assert_eq!(r.body(e).wall, 1);
    r.world
        .update::<KinematicBody, _>(e, |k| k.velocity = Vec2::new(0.0, -600.0))
        .unwrap();
    r.tick(1);
    assert!(
        (r.pos(e).y - 1.25).abs() < 1e-4,
        "on the floor: {}",
        r.pos(e).y
    );
    assert!(r.body(e).grounded);
}

#[test]
fn overlaps_begin_and_end_once_and_respect_layers() {
    let mut r = open("overlap", 3, 4);
    let trigger = r.world.spawn();
    r.world.insert(trigger, Transform::at(10.0, 10.0)).unwrap();
    r.world
        .insert(
            trigger,
            Collider {
                size: Vec2::new(2.0, 2.0),
                ..Default::default()
            },
        )
        .unwrap();
    let bullet = r.world.spawn();
    r.world.insert(bullet, Transform::at(6.0, 10.0)).unwrap();
    r.world
        .insert(
            bullet,
            Collider {
                radius: 0.25,
                ..Default::default()
            },
        )
        .unwrap();
    let ghost = r.world.spawn();
    r.world.insert(ghost, Transform::at(6.0, 10.0)).unwrap();
    r.world
        .insert(
            ghost,
            Collider {
                radius: 0.25,
                layer: 2,
                mask: 2,
                ..Default::default()
            },
        )
        .unwrap();
    let mut begun_at = None;
    for x in 6..=13 {
        for e in [bullet, ghost] {
            r.world
                .update::<Transform, _>(e, |t| t.pos.x = x as f32)
                .unwrap();
        }
        r.tick(1);
        if begun_at.is_none() && !r.named("overlap.begin").is_empty() {
            begun_at = Some(x);
        }
    }
    let b = r.named("overlap.begin");
    let e = r.named("overlap.end");
    assert_eq!((b.len(), e.len()), (1, 1), "{:?}", r.events);
    assert_eq!(begun_at, Some(9), "8.75 touches, 9 overlaps");
    let pair = |ev: &Ev| (field(ev, "entity").cloned(), field(ev, "other").cloned());
    assert_eq!(
        pair(b[0]),
        (Some(Value::Entity(trigger)), Some(Value::Entity(bullet)))
    );
    assert_eq!(pair(e[0]), pair(b[0]));
}

#[test]
fn a_player_on_a_platform_touches_it_without_overlapping_it() {
    let mut r = open("touch", 10, 20);
    r.solid(8.0, 3.0, 3.0, 0.5, Vec2::ZERO, false);
    let p = r.player(8.0, 3.5);
    r.settle(p, 30);
    r.tick(30);
    assert!(
        r.named("overlap.begin").is_empty(),
        "{:?}",
        r.named("overlap.begin")
    );
}

#[test]
fn the_same_inputs_give_the_same_run() {
    let run = || {
        let mut r = Rig::new(
            "determinism",
            1.0,
            &[
                "................",
                "......-----.....",
                "................",
                "..../####.......",
                "################",
            ],
        );
        let p = r.player(2.0, 1.0);
        let mut out = Vec::new();
        for t in 0..400u32 {
            let x = [1.0, 1.0, 0.0, -1.0][(t / 37 % 4) as usize];
            let jump = t % 53 < 20;
            r.input(p, x, 0.0, jump);
            r.tick(1);
            let (q, k) = (r.pos(p), r.body(p));
            out.push((
                q.x.to_bits(),
                q.y.to_bits(),
                k.velocity.x.to_bits(),
                k.velocity.y.to_bits(),
                k.grounded,
            ));
        }
        (out, r.events.len())
    };
    assert_eq!(run(), run());
}

#[test]
fn a_one_way_platform_body_is_jumped_through_and_ridden() {
    let mut r = open("oneway-body", 10, 20);
    let plat = r.solid(8.0, 3.5, 3.0, 0.5, Vec2::ZERO, true);
    let p = r.player(8.0, 1.0);
    r.settle(p, 10);
    r.clear();
    r.input(p, 0.0, 0.0, true);
    r.tick(90);
    assert!(
        r.named("body.ceiling").is_empty(),
        "no bonk on its underside"
    );
    assert!((r.feet(p) - 3.75).abs() < 1e-4, "on top: {}", r.feet(p));
    let b = r.body(p);
    assert!(b.grounded && b.on_one_way && b.ground == plat);
    // It sinks; the rider goes down with it.
    r.world
        .update::<KinematicBody, _>(plat, |k| k.velocity = Vec2::new(0.0, -1.0))
        .unwrap();
    r.tick(30);
    assert!((r.feet(p) - 3.25).abs() < 1e-3, "{}", r.feet(p));
    assert!(r.body(p).grounded);
}

#[test]
fn a_step_with_no_headroom_is_a_wall() {
    // Quarter tiles: a 0.25 step under a ceiling 1.75 above the step, lower
    // than the player's 1.6 plus the step.
    let mut art: Vec<String> = vec![".".repeat(40); 4];
    art.push(format!("{}{}", ".".repeat(16), "#".repeat(24)));
    art.extend(std::iter::repeat_n(".".repeat(40), 6));
    art.push(format!("{}{}", ".".repeat(16), "#".repeat(24)));
    art.push("#".repeat(40));
    let refs: Vec<&str> = art.iter().map(String::as_str).collect();
    let mut r = Rig::new("headroom", 0.25, &refs);
    let p = r.player(2.0, 0.25);
    r.settle(p, 10);
    r.input(p, 1.0, 0.0, false);
    r.tick(90);
    assert!(
        (r.pos(p).x - 3.6).abs() < 1e-4,
        "stopped at the step: {}",
        r.pos(p).x
    );
    assert!((r.feet(p) - 0.25).abs() < 1e-4);
    assert_eq!(r.body(p).wall, 1);
}

/// 1,000 characters running and jumping over a 256 x 64 map with slopes
/// and one-way platforms, and 20,000 overlap circles against 200 boxes.
/// Release only: `cargo test --release -p argus-physics -- --ignored`.
#[test]
#[ignore]
fn perf_a_thousand_characters_and_twenty_thousand_contacts() {
    let mut art: Vec<String> = Vec::new();
    for row in 0..64 {
        let s: String = (0..256)
            .map(|c| match row {
                63 => '#',
                r if r % 8 == 5 && (c / 12) % 3 == 1 => '-',
                62 if c % 20 == 10 => '/',
                62 if c % 20 == 11 => '#',
                62 if c % 20 == 12 => '\\',
                _ => '.',
            })
            .collect();
        art.push(s);
    }
    let refs: Vec<&str> = art.iter().map(String::as_str).collect();
    let mut r = Rig::new("perf", 1.0, &refs);
    let players: Vec<_> = (0..1000)
        .map(|i| r.player(1.0 + (i % 250) as f32, 2.0 + (i / 250) as f32 * 10.0))
        .collect();
    let mut worst = std::time::Duration::ZERO;
    let mut total = std::time::Duration::ZERO;
    for t in 0..300u32 {
        for (i, &p) in players.iter().enumerate() {
            let x = if (t / 60 + i as u32).is_multiple_of(2) {
                1.0
            } else {
                -1.0
            };
            r.input(p, x, 0.0, (t + i as u32) % 45 < 10);
        }
        let s = std::time::Instant::now();
        r.sched.step(&mut r.world, 1).unwrap();
        let d = s.elapsed();
        r.world.drain_events();
        if t >= 10 {
            worst = worst.max(d);
            total += d;
        }
    }
    r.healthy();
    let mean_bodies = total / 290;
    println!("1,000 characters: mean {mean_bodies:?}, worst {worst:?} per tick");

    let mut r = open("perf-overlap", 3, 4);
    for i in 0..200 {
        let e = r.world.spawn();
        r.world
            .insert(
                e,
                Transform::at((i % 20) as f32 * 10.0, (i / 20) as f32 * 10.0),
            )
            .unwrap();
        r.world
            .insert(
                e,
                Collider {
                    size: Vec2::new(2.0, 2.0),
                    ..Default::default()
                },
            )
            .unwrap();
    }
    let bullets: Vec<_> = (0..20_000)
        .map(|i| {
            let e = r.world.spawn();
            r.world
                .insert(e, Transform::at((i % 200) as f32, (i / 200) as f32))
                .unwrap();
            r.world
                .insert(
                    e,
                    Collider {
                        radius: 0.2,
                        ..Default::default()
                    },
                )
                .unwrap();
            e
        })
        .collect();
    let mut worst_o = std::time::Duration::ZERO;
    let mut total_o = std::time::Duration::ZERO;
    let mut contacts = 0usize;
    for t in 0..120u32 {
        for (i, &b) in bullets.iter().enumerate() {
            let x = ((i % 200) as f32 + t as f32 * 0.25) % 200.0;
            r.world
                .update::<Transform, _>(b, |tr| tr.pos.x = x)
                .unwrap();
        }
        let s = std::time::Instant::now();
        r.sched.step(&mut r.world, 1).unwrap();
        let d = s.elapsed();
        contacts += r.world.drain_events().len();
        if t >= 10 {
            worst_o = worst_o.max(d);
            total_o += d;
        }
    }
    r.healthy();
    let mean_o = total_o / 110;
    println!("20,000 overlap circles: mean {mean_o:?}, worst {worst_o:?} per tick, {contacts} contact events");
    assert!(mean_bodies.as_secs_f64() < 0.004, "{mean_bodies:?}");
    assert!(mean_o.as_secs_f64() < 0.008, "{mean_o:?}");
}
