//! §13 Particles and VFX: emitter pools as a structure of arrays, driven by
//! effect assets with rate, bursts, lifetimes, and curves over life,
//! deterministic from the run's seed.

use argus_assets::effect::parse_emitter;
use argus_ecs::{Entity, Event, Transform, Value, Vec2, World};
use argus_particles::{lut_index, Emitter, ParticleStore, LUT};
use argus_sim::{Scheduler, SchedulerConfig, Stage, SystemDesc};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

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

/// An empty `argus-<name>-<pid>` in the temp directory.
fn temp(name: &str) -> Temp {
    let dir = std::env::temp_dir().join(format!("argus-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Temp(dir)
}

type Fire = Rc<RefCell<Vec<(Entity, i64, Option<Vec2>)>>>;

struct Rig {
    world: World,
    sched: Scheduler,
    store: Rc<RefCell<ParticleStore>>,
    events: Vec<(String, Vec<(String, Value)>)>,
    fire: Fire,
}

fn rig_seeded(seed: u64) -> Rig {
    let mut world = World::new();
    argus_ecs::hierarchy::register(&mut world).unwrap();
    let mut sched = Scheduler::new(SchedulerConfig::continuous(60.0).seed(seed));
    let store = Rc::new(RefCell::new(ParticleStore::default()));
    argus_particles::install(&mut world, &mut sched, store.clone()).unwrap();
    // Gameplay that asks for bursts by event, earlier in the tick.
    let fire: Fire = Rc::new(RefCell::new(Vec::new()));
    let f = fire.clone();
    sched
        .add_system(
            SystemDesc::new("test.fire", Stage::Sim),
            Box::new(move |ctx| {
                for (e, n, pos) in f.borrow_mut().drain(..) {
                    let mut fields = vec![
                        ("emitter".to_string(), Value::Entity(e)),
                        ("count".to_string(), Value::Int(n)),
                    ];
                    if let Some(p) = pos {
                        fields.push(("pos".to_string(), Value::Vec2(p)));
                    }
                    ctx.emit("particles.burst", fields);
                }
                Ok(())
            }),
        )
        .unwrap();
    Rig {
        world,
        sched,
        store,
        events: Vec::new(),
        fire,
    }
}

fn rig() -> Rig {
    rig_seeded(1)
}

impl Rig {
    fn effect(&self, name: &str, toml: &str) {
        let (spec, warnings) = parse_emitter(&format!("assets/{name}.toml"), toml).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        self.store.borrow_mut().set_effect(name, &spec);
    }
    fn emitter(&mut self, effect: &str, x: f32, y: f32, m: Emitter) -> Entity {
        let e = self.world.spawn();
        self.world.insert(e, Transform::at(x, y)).unwrap();
        let effect = self.world.strings().intern(effect);
        self.world.insert(e, Emitter { effect, ..m }).unwrap();
        e
    }
    fn tick(&mut self, n: u64) {
        for _ in 0..n {
            self.sched.step(&mut self.world, 1).unwrap();
            for r in self.world.drain_events() {
                if let Event::Game { name, fields } = r.event {
                    self.events.push((name, fields));
                }
            }
        }
        for s in self.sched.systems(self.world.registry()).unwrap() {
            assert!(s.enabled, "{} faulted: {:?}", s.name, s.disabled_reason);
        }
    }
    fn em(&self, e: Entity) -> Emitter {
        self.world.get::<Emitter>(e).unwrap()
    }
    fn xs(&self, e: Entity) -> Vec<(f32, f32)> {
        let s = self.store.borrow();
        let p = s.pool(e).unwrap();
        p.x().iter().zip(p.y()).map(|(a, b)| (*a, *b)).collect()
    }
    fn named(&self, n: &str) -> Vec<&Vec<(String, Value)>> {
        self.events
            .iter()
            .filter(|e| e.0 == n)
            .map(|e| &e.1)
            .collect()
    }
}

/// The opening burst fires when it starts, the rate emits exactly what it
/// owes, and particles die at their lifetime, so the pool settles at rate
/// times lifetime.
#[test]
fn a_burst_then_the_rate_and_lifetimes_bound_the_pool() {
    let mut r = rig();
    r.effect(
        "fx/steady",
        "type = \"emitter\"\nburst = 10\nrate = 30\nlifetime = 0.5\nspeed = 0\n",
    );
    let e = r.emitter("fx/steady", 0.0, 0.0, Emitter::default());
    r.tick(1);
    let m = r.em(e);
    assert!(m.started && !m.done);
    assert_eq!((m.live, m.emitted), (10, 10));
    r.tick(60);
    let m = r.em(e);
    assert_eq!(m.emitted, 10 + 30, "30 a second for a second, exactly");
    assert_eq!(m.live, 15, "rate x lifetime; the burst is long gone");
    assert!((m.time - 1.0).abs() < 1e-4, "{}", m.time);
    assert_eq!(r.store.borrow().live(), 15);
    // rate_scale is a tune field: double it and the pool doubles.
    r.world.set_field(e, "Emitter.rate_scale", 2.0f32).unwrap();
    r.tick(60);
    assert_eq!(r.em(e).live, 30);
    // Stopping it lets the particles already out live out their lives.
    r.world.set_field(e, "Emitter.playing", false).unwrap();
    r.tick(10);
    let live = r.em(e).live;
    assert!(live > 0 && live < 30, "{live}");
    r.tick(30);
    assert_eq!(r.em(e).live, 0);
}

/// Particles move by their velocity with gravity and drag, and the velocity
/// curve scales their speed over life.
#[test]
fn particles_move_with_gravity_drag_and_the_velocity_curve() {
    let mut r = rig();
    r.effect(
        "fx/one",
        "burst = 1\nspeed = 2\ndirection = 0\nspread = 0\nlifetime = 10\n",
    );
    r.effect(
        "fx/fall",
        "burst = 1\nspeed = 0\nlifetime = 10\ngravity = [0, -10]\n",
    );
    r.effect(
        "fx/drag",
        "burst = 1\nspeed = 2\ndirection = 90\nspread = 0\nlifetime = 10\ndrag = 1\n",
    );
    r.effect("fx/slow", "burst = 1\nspeed = 2\ndirection = 0\nspread = 0\nlifetime = 1\nvelocity = [[0, 1], [1, 0]]\n");
    let one = r.emitter("fx/one", 1.0, 1.0, Emitter::default());
    let fall = r.emitter("fx/fall", 0.0, 0.0, Emitter::default());
    let drag = r.emitter("fx/drag", 0.0, 0.0, Emitter::default());
    let slow = r.emitter("fx/slow", 0.0, 0.0, Emitter::default());
    r.tick(61);
    let (x, y) = r.xs(one)[0];
    assert!((x - 3.0).abs() < 1e-3 && (y - 1.0).abs() < 1e-5, "{x} {y}");
    let (_, y) = r.xs(fall)[0];
    assert!((y + 5.0).abs() < 0.1, "about g t^2 / 2 after a second: {y}");
    let s = r.store.borrow();
    let vy = s.pool(drag).unwrap().vy()[0];
    assert!((vy - 2.0 * (-1.0f32).exp()).abs() < 1e-3, "{vy}");
    drop(s);
    // Slowed linearly to nothing over its life: about half the distance.
    let s = r.store.borrow();
    let p = s.pool(slow).unwrap();
    assert!(p.is_empty(), "it lived one second");
    drop(s);
    let mut r = rig();
    r.effect("fx/slow", "burst = 1\nspeed = 2\ndirection = 0\nspread = 0\nlifetime = 1\nvelocity = [[0, 1], [1, 0]]\n");
    let slow = r.emitter("fx/slow", 0.0, 0.0, Emitter::default());
    r.tick(55);
    let (x, _) = r.xs(slow)[0];
    let t = 54.0 / 60.0f32;
    let want = 2.0 * (t - t * t / 2.0);
    assert!((x - want).abs() < 0.05, "{x} vs {want}");
}

/// The effect's max is the pool: what does not fit is counted, not emitted.
#[test]
fn max_caps_the_pool_and_counts_what_was_dropped() {
    let mut r = rig();
    r.effect("fx/cap", "burst = 8\nmax = 5\nlifetime = 1\n");
    let e = r.emitter("fx/cap", 0.0, 0.0, Emitter::default());
    r.tick(1);
    let m = r.em(e);
    assert_eq!((m.live, m.emitted, m.dropped), (5, 5, 3));
}

/// Gameplay fires bursts by setting Emitter.burst or by emitting
/// particles.burst {emitter, count, pos}; neither needs the emitter to be
/// playing.
#[test]
fn bursts_come_from_the_field_and_from_events_at_a_position() {
    let mut r = rig();
    r.effect("fx/hit", "burst = 0\nrate = 0\nspeed = 0\nlifetime = 1\n");
    let e = r.emitter(
        "fx/hit",
        2.0,
        0.0,
        Emitter {
            playing: false,
            ..Default::default()
        },
    );
    r.tick(1);
    assert_eq!(r.em(e).live, 0);
    r.world.set_field(e, "Emitter.burst", 4u64).unwrap();
    r.tick(1);
    assert_eq!(r.em(e).live, 4);
    assert_eq!(r.em(e).burst, 0, "the request is consumed");
    assert!(r.xs(e).iter().all(|p| *p == (2.0, 0.0)));
    r.fire.borrow_mut().push((e, 3, Some(Vec2::new(10.0, 5.0))));
    r.tick(1);
    assert_eq!(r.em(e).live, 7);
    assert_eq!(r.xs(e).iter().filter(|p| **p == (10.0, 5.0)).count(), 3);
    assert!(
        !r.em(e).done && !r.em(e).started,
        "not playing: it never started"
    );
}

/// A finite effect finishes: particles.done once its last particle is gone,
/// and auto_despawn removes the entity and its pool. Clearing started plays
/// an effect again.
#[test]
fn a_finished_emitter_says_so_and_can_despawn_itself() {
    let mut r = rig();
    r.effect(
        "fx/short",
        "rate = 60\nduration = 0.25\nlifetime = 0.2\nspeed = 1\n",
    );
    r.effect("fx/pop", "burst = 5\nlifetime = 0.1\n");
    let a = r.emitter(
        "fx/short",
        0.0,
        0.0,
        Emitter {
            auto_despawn: true,
            ..Default::default()
        },
    );
    let b = r.emitter("fx/pop", 0.0, 0.0, Emitter::default());
    r.tick(1);
    assert!(r.em(b).done, "a burst with no rate is done at once");
    r.tick(20);
    let m = r.em(a);
    assert!(m.done && m.emitted == 15, "{m:?}");
    assert!(r.world.is_alive(a), "its particles are still out");
    r.tick(20);
    assert!(!r.world.is_alive(a), "despawned once empty");
    assert!(r.store.borrow().pool(a).is_none());
    let done: Vec<Value> = r
        .named("particles.done")
        .iter()
        .map(|f| f[0].1.clone())
        .collect();
    assert_eq!(done, [Value::Entity(b), Value::Entity(a)]);
    // b stays (no auto_despawn), empty, and plays again from the start.
    assert_eq!(r.em(b).live, 0);
    r.world.set_field(b, "Emitter.started", false).unwrap();
    r.tick(1);
    assert_eq!((r.em(b).live, r.em(b).emitted), (5, 10));
    r.tick(10);
    assert_eq!(r.named("particles.done").len(), 3);
}

/// Randomness comes from the run's seed: the same seed emits the same
/// particles, another seed different ones.
#[test]
fn the_same_seed_emits_the_same_particles() {
    let run = |seed: u64| {
        let mut r = rig_seeded(seed);
        r.effect("fx/spray", "rate = 200\nspeed = [1, 4]\nspread = 90\nlifetime = [0.3, 0.8]\nshape = \"circle\"\nradius = 0.5\n");
        let e = r.emitter("fx/spray", 0.0, 0.0, Emitter::default());
        r.tick(40);
        r.xs(e)
    };
    let a = run(7);
    assert!(a.len() > 50);
    assert_eq!(a, run(7));
    assert_ne!(a, run(8));
}

/// A moving emitter's particles are born along its path, not in clumps at
/// each tick's end; a local effect's particles move with it.
#[test]
fn a_moving_emitter_leaves_an_even_trail_and_local_particles_follow() {
    let mut r = rig();
    r.effect("fx/trail", "rate = 600\nspeed = 0\nlifetime = 5\n");
    r.effect(
        "fx/aura",
        "burst = 1\nspeed = 0\nlifetime = 5\nspace = \"local\"\n",
    );
    let e = r.emitter("fx/trail", 0.0, 0.0, Emitter::default());
    let l = r.emitter("fx/aura", 0.0, 0.0, Emitter::default());
    r.tick(1);
    for k in 1..=3 {
        for x in [e, l] {
            r.world
                .set_field(x, "Transform.pos", Vec2::new(k as f32, 0.0))
                .unwrap();
        }
        r.tick(1);
    }
    // The last tick moved it from 2 to 3: ten particles, a tenth apart.
    let xs: Vec<f32> = r.xs(e).iter().rev().take(10).map(|p| p.0).collect();
    for (i, x) in xs.iter().enumerate() {
        assert!((x - (3.0 - i as f32 * 0.1)).abs() < 1e-4, "{xs:?}");
    }
    let s = r.store.borrow();
    let p = s.pool(l).unwrap();
    assert_eq!(
        (p.x()[0], p.origin, p.origin_prev),
        (0.0, [3.0, 0.0], [2.0, 0.0])
    );
}

/// An effect that does not exist is reported once, naming the ones that do,
/// and the other emitters run.
#[test]
fn an_unknown_effect_is_reported_once() {
    let mut r = rig();
    r.effect("fx/ok", "burst = 2\nlifetime = 1\n");
    let bad = r.emitter("fx/nope", 0.0, 0.0, Emitter::default());
    let ok = r.emitter("fx/ok", 0.0, 0.0, Emitter::default());
    r.tick(5);
    let errs = r.named("particles.error");
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0][0].1, Value::Entity(bad));
    let Value::Str(msg) = &errs[0][1].1 else {
        panic!()
    };
    assert!(
        msg.contains("\"fx/nope\"") && msg.contains("fx/ok"),
        "{msg}"
    );
    assert_eq!(r.em(ok).live, 2);
    // Supplying the effect later starts it.
    r.effect("fx/nope", "burst = 3\nlifetime = 1\n");
    r.tick(1);
    assert_eq!(r.em(bad).live, 3);
}

/// Curves are baked into tables once; the renderer reads a particle's size,
/// rotation, and colour from its life fraction.
#[test]
fn curves_bake_into_tables_the_renderer_reads() {
    let (spec, _) = parse_emitter(
        "assets/fx/c.toml",
        "burst = 1\nsize = [[0, 1], [1, 3]]\nrotation = [[0, 0], [1, 180]]\ncolor = [[0, \"#FFFFFFFF\"], [1, \"#00000000\"]]\n",
    )
    .unwrap();
    let fx = argus_particles::Effect::compile("fx/c", &spec);
    assert!(
        fx.velocity.is_none(),
        "a flat velocity curve skips its table"
    );
    assert_eq!(fx.atlas, "rect");
    assert_eq!(fx.size[0], 1.0);
    assert_eq!(fx.size[LUT - 1], 3.0);
    assert!((fx.size[lut_index(0.5)] - 2.0).abs() < 0.05);
    assert!((fx.rotation[LUT - 1] - std::f32::consts::PI).abs() < 1e-5);
    assert_eq!(fx.color[0], [1.0, 1.0, 1.0, 1.0]);
    assert_eq!(fx.color[LUT - 1], [0.0, 0.0, 0.0, 0.0]);
}

/// Effects come from the importer; a file that stops importing keeps its
/// last good effect running.
#[test]
fn effects_come_from_the_importer_and_a_broken_file_keeps_the_last_good_one() {
    let dir = temp("particles");
    std::fs::create_dir_all(dir.join("assets/fx")).unwrap();
    std::fs::write(
        dir.join("assets/fx/a.toml"),
        "type = \"emitter\"\nburst = 4\n",
    )
    .unwrap();
    let mut imp = argus_assets::Importer::new(&dir);
    imp.scan();
    let mut store = ParticleStore::default();
    store.set_assets(&imp);
    let g = store.generation;
    assert_eq!(store.effects["fx/a"].spec.burst, 4);
    store.set_assets(&imp);
    assert_eq!(store.generation, g, "nothing changed");
    std::fs::write(
        dir.join("assets/fx/a.toml"),
        "type = \"emitter\"\nburst = -4\n",
    )
    .unwrap();
    imp.scan();
    assert!(imp.failures().contains_key("fx/a"));
    store.set_assets(&imp);
    assert_eq!(store.effects["fx/a"].spec.burst, 4);
    std::fs::write(
        dir.join("assets/fx/a.toml"),
        "type = \"emitter\"\nburst = 6\n",
    )
    .unwrap();
    imp.scan();
    store.set_assets(&imp);
    assert_eq!(store.effects["fx/a"].spec.burst, 6);
    assert!(store.generation > g);
}

/// §17: a quarter of a million live particles across fifty emitters,
/// simulated in under a millisecond a tick. Release only:
/// `cargo test --release -p argus-particles -- --ignored`.
#[test]
#[ignore]
fn perf_a_quarter_million_particles() {
    let mut r = rig();
    r.effect(
        "fx/storm",
        "rate = 5000\nmax = 6000\nlifetime = 1\nspeed = [1, 3]\nspread = 360\ngravity = [0, -4]\ndrag = 0.5\nshape = \"circle\"\nradius = 1\nvelocity = [[0, 1], [1, 0.3]]\n",
    );
    let es: Vec<Entity> = (0..50)
        .map(|i| r.emitter("fx/storm", i as f32 * 3.0, 0.0, Emitter::default()))
        .collect();
    r.tick(90);
    let live = r.store.borrow().live();
    assert!(live > 240_000, "{live}");
    let mut total = std::time::Duration::ZERO;
    for t in 0..300u32 {
        for (i, &e) in es.iter().enumerate() {
            let x = i as f32 * 3.0 + (t as f32 * 0.05).sin();
            r.world
                .set_field(e, "Transform.pos", Vec2::new(x, 0.0))
                .unwrap();
        }
        let s = std::time::Instant::now();
        r.sched.step(&mut r.world, 1).unwrap();
        total += s.elapsed();
        r.world.drain_events();
    }
    let rows = r.sched.prof_systems(300).unwrap();
    let p = rows
        .iter()
        .find(|row| row.name == "particles.update")
        .unwrap();
    println!(
        "{} live particles, 50 emitters: particles.update mean {:.3} ms, p95 {:.3} ms; tick mean {:?}",
        r.store.borrow().live(),
        p.mean_ms,
        p.p95_ms,
        total / 300
    );
    assert!(p.mean_ms < 1.0, "{} ms", p.mean_ms);
}

/// With lifetimes drawn from a range the dead leave holes all through a
/// pool; the survivors close them in the order they were born.
#[test]
fn random_lifetimes_leave_holes_that_close_in_birth_order() {
    let mut r = rig();
    r.effect("fx/mixed", "rate = 120\nspeed = 0\nlifetime = [0.05, 1]\n");
    let e = r.emitter("fx/mixed", 0.0, 0.0, Emitter::default());
    for k in 1..=300 {
        r.world
            .set_field(e, "Transform.pos", Vec2::new(k as f32 * 0.1, 0.0))
            .unwrap();
        r.tick(1);
        let s = r.store.borrow();
        let p = s.pool(e).unwrap();
        assert!(
            p.x().windows(2).all(|w| w[0] < w[1]),
            "born later, further along, after tick {k}"
        );
        assert!(p.life().iter().all(|t| *t < 1.0));
        assert_eq!(p.len(), r.em(e).live as usize);
    }
    let live = r.em(e).live;
    assert!((40..=90).contains(&live), "about 120 x 0.525: {live}");
}
