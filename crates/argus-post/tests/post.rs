//! §13 (phase 2 item 6): hit-flash, screen shake, and hit-stop on a game
//! event, as world state on the sim's ticks: a stop freezes the game and not
//! the effects; a scale lets one tick in 1/scale through; a replay is the
//! same.

use argus_ecs::{Component, Result, Vec2, World};
use argus_post::{PostFlash, PostHitStop, PostShake};
use argus_sim::{Scheduler, SchedulerConfig, Stage, SystemDesc};

/// Moves on every tick it runs.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
#[argus(doc = "Counts the ticks the game ran")]
struct Ran {
    #[argus(doc = "Ticks run")]
    n: u32,
}

/// A world with a camera carrying the three, a game system that counts the
/// ticks it runs, and hits at the given ticks (with an amount).
fn setup(
    hits: &'static [(u64, f64)],
    on: &str,
    stop: PostHitStop,
) -> (World, Scheduler, argus_ecs::Entity) {
    let mut w = World::new();
    w.register::<Ran>().unwrap();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0).seed(1));
    argus_post::install(&mut w, &mut s).unwrap();
    let cam = w.spawn_authored("camera").unwrap();
    let on_id = w.strings().intern(on);
    w.insert(
        cam,
        PostFlash {
            on: on_id,
            ..Default::default()
        },
    )
    .unwrap();
    w.insert(
        cam,
        PostShake {
            on: on_id,
            ..Default::default()
        },
    )
    .unwrap();
    w.insert(cam, PostHitStop { on: on_id, ..stop }).unwrap();
    w.insert(cam, Ran::default()).unwrap();
    let ran = w.component_id("Ran").unwrap();
    // An empty `on` listens for hit, so that is what the game emits.
    let name = if on.is_empty() {
        argus_post::DEFAULT_EVENT
    } else {
        on
    }
    .to_string();
    s.add_system(
        SystemDesc::new("sim.game", Stage::Sim).writes(ran),
        Box::new(move |ctx| -> Result<()> {
            ctx.update::<Ran, _>(cam, |r| r.n += 1)?;
            for (t, a) in hits {
                if ctx.tick == *t {
                    ctx.emit(
                        name.clone(),
                        vec![("amount".into(), argus_ecs::Value::Float(*a))],
                    );
                }
            }
            Ok(())
        }),
    )
    .unwrap();
    (w, s, cam)
}

fn step(w: &mut World, s: &mut Scheduler, n: u64) {
    s.step(w, n).unwrap();
    let _ = w.drain_events();
}

#[test]
fn a_hit_stops_the_game_and_not_the_flash_or_the_shake() {
    let (mut w, mut s, cam) = setup(
        &[(5, 1.0)],
        "",
        PostHitStop {
            frames: 3,
            ..Default::default()
        },
    );
    step(&mut w, &mut s, 5);
    assert_eq!(w.get::<Ran>(cam).unwrap().n, 5);
    // The hit's tick: the flash and the shake start, and the stop is armed.
    step(&mut w, &mut s, 1);
    let f = w.get::<PostFlash>(cam).unwrap();
    assert!((f.amount - 0.6).abs() < 1e-6, "{f:?}");
    let sh = w.get::<PostShake>(cam).unwrap();
    assert!(
        (sh.level - 0.4).abs() < 1e-6 && sh.offset != Vec2::ZERO,
        "{sh:?}"
    );
    assert_eq!(w.get::<PostHitStop>(cam).unwrap().left, 3);
    // Three frozen ticks: the game does not run; the effects fade on.
    for k in 1..=3 {
        step(&mut w, &mut s, 1);
        assert!(s.frozen(), "tick {k} of the stop");
        assert_eq!(w.get::<Ran>(cam).unwrap().n, 6, "the game stood still");
        let f = w.get::<PostFlash>(cam).unwrap();
        assert!(
            (f.amount - (0.6 - 0.1 * k as f32)).abs() < 1e-5,
            "flash at {k}: {f:?}"
        );
    }
    step(&mut w, &mut s, 1);
    assert!(!s.frozen());
    assert_eq!(w.get::<Ran>(cam).unwrap().n, 7, "and runs again");
    // The shake settles to nothing.
    step(&mut w, &mut s, 30);
    let sh = w.get::<PostShake>(cam).unwrap();
    assert_eq!((sh.level, sh.offset), (0.0, Vec2::ZERO));
    assert_eq!(w.get::<PostFlash>(cam).unwrap().amount, 0.0);
}

#[test]
fn a_scale_lets_one_tick_in_so_many_through_and_amount_scales_the_hit() {
    let (mut w, mut s, cam) = setup(
        &[(2, 0.5)],
        "boom",
        PostHitStop {
            frames: 8,
            scale: 0.25,
            ..Default::default()
        },
    );
    step(&mut w, &mut s, 3);
    let f = w.get::<PostFlash>(cam).unwrap();
    assert!(
        (f.amount - 0.3).abs() < 1e-6,
        "half a hit, half the peak: {f:?}"
    );
    let sh = w.get::<PostShake>(cam).unwrap();
    assert!((sh.level - 0.2).abs() < 1e-6);
    // Eight slow ticks at a quarter: two of them run.
    step(&mut w, &mut s, 8);
    assert_eq!(w.get::<Ran>(cam).unwrap().n, 3 + 2);
    step(&mut w, &mut s, 4);
    assert_eq!(w.get::<Ran>(cam).unwrap().n, 9);
    // Another event name does nothing.
    let (mut w, mut s, cam) = setup(&[(2, 1.0)], "other", PostHitStop::default());
    w.update::<PostFlash, _>(cam, |f| f.on = argus_ecs::StrId::EMPTY)
        .unwrap();
    step(&mut w, &mut s, 4);
    assert_eq!(
        w.get::<PostFlash>(cam).unwrap().amount,
        0.0,
        "listens for hit, heard other"
    );
}

#[test]
fn the_same_run_shakes_the_same_way() {
    let run = || {
        let (mut w, mut s, cam) = setup(
            &[(3, 1.0), (20, 0.7)],
            "",
            PostHitStop {
                frames: 2,
                ..Default::default()
            },
        );
        let mut offs = Vec::new();
        for _ in 0..40 {
            step(&mut w, &mut s, 1);
            offs.push(w.get::<PostShake>(cam).unwrap().offset);
        }
        (offs, w.digest())
    };
    assert_eq!(run(), run());
}
