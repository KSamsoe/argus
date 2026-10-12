//! §13 Sprite animation: an Animator runs a TOML state machine whose
//! transitions are field comparisons (with windows and crossfades), whose
//! states carry events by frame, and whose rates, anticipation, and holds
//! can be tune fields; §17 2,000 animators under 0.3 ms a tick.

use argus_anim::{AnimStore, Animator, SpriteClips};
use argus_assets::animator::parse_animator;
use argus_assets::sidecar::Direction;
use argus_assets::Clip;
use argus_ecs::{Entity, Event, FieldType, SchemaBuilder, Value, Vec2, World};
use argus_sim::{Scheduler, SchedulerConfig};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

/// A game event as the rig keeps it: frame, name, fields.
type Ev = (u64, String, Vec<(String, Value)>);

struct Rig {
    world: World,
    sched: Scheduler,
    store: Rc<RefCell<AnimStore>>,
    events: Vec<Ev>,
}

fn clip(frames: &[u32], fps: f32, looping: bool) -> Clip {
    Clip {
        frames: frames.to_vec(),
        fps: Some(fps),
        looping,
        direction: Direction::Forward,
    }
}

fn rig(machine: &str, clips: &[(&str, Clip)]) -> Rig {
    let mut world = World::new();
    argus_ecs::hierarchy::register(&mut world).unwrap();
    // The renderer's Sprite, by name: the animator writes it by name.
    let sprite = SchemaBuilder::new("Sprite")
        .field("atlas", FieldType::Str)
        .field("frame", FieldType::U32)
        .field("blend_frame", FieldType::U32)
        .field("blend", FieldType::F32)
        .field("flip_x", FieldType::Bool)
        .build()
        .unwrap();
    world.register_schema(sprite).unwrap();
    let body = SchemaBuilder::new("Body")
        .field("vx", FieldType::F32)
        .field("grounded", FieldType::Bool)
        .field("vel", FieldType::Vec2)
        .field("dash", FieldType::I32)
        .build()
        .unwrap();
    world.register_schema(body).unwrap();
    let feel = SchemaBuilder::new("Feel")
        .field("windup", FieldType::F32)
        .field("hold", FieldType::F32)
        .field("run_fps", FieldType::F32)
        .build()
        .unwrap();
    world.register_schema(feel).unwrap();
    let mut sched = Scheduler::new(SchedulerConfig::continuous(60.0).seed(1));
    let store = Rc::new(RefCell::new(AnimStore::default()));
    {
        let mut s = store.borrow_mut();
        let (spec, _) = parse_animator("assets/anim/hero.toml", machine).unwrap();
        s.machines.insert("anim/hero".into(), spec);
        s.sprites.insert(
            "hero".into(),
            SpriteClips {
                clips: clips
                    .iter()
                    .map(|(n, c)| (n.to_string(), c.clone()))
                    .collect::<BTreeMap<_, _>>(),
                frame_ms: vec![100; 32],
            },
        );
        s.generation += 1;
    }
    argus_anim::install(&mut world, &mut sched, store.clone()).unwrap();
    Rig {
        world,
        sched,
        store,
        events: Vec::new(),
    }
}

impl Rig {
    fn actor(&mut self) -> Entity {
        let e = self.world.spawn();
        let sprite = self.world.component_id("Sprite").unwrap();
        let body = self.world.component_id("Body").unwrap();
        let feel = self.world.component_id("Feel").unwrap();
        for c in [sprite, body, feel] {
            self.world.insert_default(e, c).unwrap();
        }
        self.world.set_field(e, "Sprite.atlas", "hero").unwrap();
        let machine = self.world.strings().intern("anim/hero");
        self.world
            .insert(
                e,
                Animator {
                    machine,
                    ..Default::default()
                },
            )
            .unwrap();
        e
    }
    fn tick(&mut self, n: u64) {
        for _ in 0..n {
            let frame = self.world.frame();
            self.sched.step(&mut self.world, 1).unwrap();
            for r in self.world.drain_events() {
                if let Event::Game { name, fields } = r.event {
                    self.events.push((frame, name, fields));
                }
            }
        }
        for s in self.sched.systems(self.world.registry()).unwrap() {
            assert!(s.enabled, "{} faulted: {:?}", s.name, s.disabled_reason);
        }
    }
    fn state(&self, e: Entity) -> String {
        let a = self.world.get::<Animator>(e).unwrap();
        self.world.strings().resolve(a.state)
    }
    fn anim(&self, e: Entity) -> Animator {
        self.world.get::<Animator>(e).unwrap()
    }
    fn frame(&self, e: Entity) -> u64 {
        match self.world.get_field(e, "Sprite.frame").unwrap() {
            Value::UInt(u) => u,
            v => panic!("{v:?}"),
        }
    }
    fn set(&mut self, e: Entity, path: &str, v: impl Into<Value>) {
        self.world.set_field(e, path, v).unwrap();
    }
    fn named(&self, n: &str) -> Vec<&Ev> {
        self.events.iter().filter(|e| e.1 == n).collect()
    }
}

fn field<'a>(ev: &'a Ev, k: &str) -> Option<&'a Value> {
    ev.2.iter().find(|(n, _)| n == k).map(|(_, v)| v)
}

const LOCOMOTION: &str = r#"type = "animator"
initial = "idle"

[states.idle]
clip = "idle"

[[states.idle.transitions]]
to = "run"
when = "Body.grounded and abs(Body.vx) > 0.5"

[states.run]
clip = "run"
events = [{ frame = 1, name = "footstep", sound = "sfx/step" }, { frame = 3, name = "footstep", sound = "sfx/step" }]

[[states.run.transitions]]
to = "idle"
when = "abs(Body.vx) <= 0.5"
blend = 0.1

[states.fall]
clip = "fall"

[[states.fall.transitions]]
to = "idle"
when = "Body.grounded"

[[any]]
to = "fall"
when = "not Body.grounded and Body.vel.y < 0"
"#;

fn locomotion() -> Rig {
    rig(
        LOCOMOTION,
        &[
            ("idle", clip(&[0, 1], 4.0, true)),
            ("run", clip(&[2, 3, 4, 5], 10.0, true)),
            ("fall", clip(&[6], 1.0, true)),
        ],
    )
}

#[test]
fn states_follow_field_conditions_and_clips_play_at_their_rates() {
    let mut r = locomotion();
    let e = r.actor();
    r.set(e, "Body.grounded", true);
    r.tick(1);
    assert_eq!(r.state(e), "idle");
    let entered = r.named("anim.entered");
    assert_eq!(entered.len(), 1);
    assert_eq!(
        field(entered[0], "from"),
        None,
        "the first state has no from"
    );
    assert_eq!(r.frame(e), 0);
    // Four frames a second: frame 1 a quarter second in, not before.
    r.tick(13);
    assert_eq!(r.frame(e), 0);
    r.tick(3);
    assert_eq!(r.frame(e), 1);
    // Running: the next tick is run.
    r.set(e, "Body.vx", 3.0f32);
    r.tick(1);
    assert_eq!(r.state(e), "run");
    assert_eq!(r.frame(e), 2, "run's first frame");
    let entered = r.named("anim.entered");
    assert_eq!(field(entered[1], "from"), Some(&Value::Str("idle".into())));
    // Ten frames a second: after 0.35 s it is on the fourth frame.
    r.tick(21);
    assert_eq!(r.frame(e), 5);
    assert!(r.anim(e).time > 0.34 && r.anim(e).time < 0.36);
    // Off a ledge: the any-state transition wins.
    r.set(e, "Body.grounded", false);
    r.set(e, "Body.vel", Vec2::new(3.0, -2.0));
    r.tick(1);
    assert_eq!(r.state(e), "fall");
    assert_eq!(r.frame(e), 6);
    r.set(e, "Body.grounded", true);
    r.set(e, "Body.vx", 0.0f32);
    r.tick(1);
    assert_eq!(r.state(e), "idle");
}

#[test]
fn frame_events_fire_on_every_frame_passed_and_ask_for_sounds() {
    // Run at 120 frames a second on a 60 Hz sim: two frames a tick, and
    // every footstep still fires.
    let mut r = rig(
        LOCOMOTION,
        &[
            ("idle", clip(&[0], 4.0, true)),
            ("run", clip(&[2, 3, 4, 5], 120.0, true)),
            ("fall", clip(&[6], 1.0, true)),
        ],
    );
    let e = r.actor();
    r.set(e, "Body.grounded", true);
    r.set(e, "Body.vx", 3.0f32);
    r.tick(2);
    assert_eq!(r.state(e), "run");
    r.events.clear();
    r.tick(60);
    // 120 frames in a second is 30 loops of 4 with 2 footsteps each; the
    // count can be off by one at the second's boundary.
    let steps = r.named("footstep").len();
    assert!((59..=61).contains(&steps), "{steps}");
    let sounds = r.named("sound.play");
    assert_eq!(sounds.len(), steps, "each footstep asks for its sound");
    assert_eq!(
        field(sounds[0], "sound"),
        Some(&Value::Str("sfx/step".into()))
    );
    let s = r.named("footstep")[0];
    assert_eq!(field(s, "state"), Some(&Value::Str("run".into())));
    assert!(matches!(
        field(s, "frame"),
        Some(Value::Int(1)) | Some(Value::Int(3))
    ));
    assert_eq!(field(s, "entity"), Some(&Value::Entity(e)));
}

const SLASH: &str = r#"type = "animator"
initial = "idle"

[states.idle]
clip = "idle"

[states.slash]
clip = "slash"
loop = false
windup = 2
anticipation = "Feel.windup"
hold = "Feel.hold"
events = [{ frame = 3, name = "hitbox.on" }]

[[states.slash.transitions]]
to = "dash"
when = "Body.dash > 0"
window = [3, 4]

[[states.slash.transitions]]
to = "idle"
when = "end"

[states.dash]
clip = "dash"
"#;

fn slash() -> Rig {
    rig(
        SLASH,
        &[
            ("idle", clip(&[0], 1.0, true)),
            ("slash", clip(&[10, 11, 12, 13, 14, 15], 10.0, false)),
            ("dash", clip(&[20], 1.0, true)),
        ],
    )
}

#[test]
fn a_one_shot_ends_a_window_gates_a_cancel_and_play_enters_a_state() {
    let mut r = slash();
    let e = r.actor();
    // Wind-up and hold as authored: two frames and none.
    r.set(e, "Feel.windup", 2.0f32);
    r.set(e, "Feel.hold", 0.0f32);
    r.tick(2);
    let p = r.world.strings().intern("slash");
    r.world.update::<Animator, _>(e, |a| a.play = p).unwrap();
    r.tick(1);
    assert_eq!(r.state(e), "slash");
    assert_eq!(
        r.anim(e).play,
        argus_ecs::StrId::EMPTY,
        "the request is cleared"
    );
    // Asking to dash before the window does nothing.
    r.set(e, "Body.dash", 1i32);
    r.tick(10);
    assert_eq!(r.state(e), "slash", "frames 0..2 are outside the window");
    // At frame 3 the window opens and the cancel fires.
    r.tick(12);
    assert_eq!(
        r.state(e),
        "dash",
        "cancelled inside the window: {:?}",
        r.anim(e)
    );
    let hit = r.named("hitbox.on");
    assert_eq!(hit.len(), 1, "the hitbox came out on frame 3");
    assert_eq!(field(hit[0], "frame"), Some(&Value::Int(3)));

    // Without the dash, the one-shot plays out and ends.
    let mut r = slash();
    let e = r.actor();
    r.set(e, "Feel.windup", 2.0f32);
    r.tick(1);
    r.world.update::<Animator, _>(e, |a| a.play = p).unwrap();
    r.tick(1);
    r.tick(36);
    assert_eq!(r.state(e), "slash", "0.6 s of clip");
    r.tick(3);
    assert_eq!(r.state(e), "idle", "ended, then back");
}

/// The tick (after entering) on which a slash reaches `frame`.
fn ticks_to(r: &mut Rig, e: Entity, frame: u32) -> u32 {
    let p = r.world.strings().intern("slash");
    r.world.update::<Animator, _>(e, |a| a.play = p).unwrap();
    r.tick(1);
    for n in 1..400 {
        r.tick(1);
        if r.anim(e).frame >= frame || r.state(e) != "slash" {
            return n;
        }
    }
    panic!("never reached frame {frame}");
}

#[test]
fn anticipation_and_hold_are_tunable_from_fields() {
    // Authored: the two wind-up frames last two frames (0.2 s).
    let mut r = slash();
    let e = r.actor();
    r.set(e, "Feel.windup", 2.0f32);
    r.tick(1);
    let n = ticks_to(&mut r, e, 2);
    assert!((11..=13).contains(&n), "wind-up of 0.2 s: {n} ticks");
    // Tuned to four frames: the wind-up takes twice as long.
    let mut r = slash();
    let e = r.actor();
    r.set(e, "Feel.windup", 4.0f32);
    r.tick(1);
    let n = ticks_to(&mut r, e, 2);
    assert!((23..=25).contains(&n), "wind-up of 0.4 s: {n} ticks");
    // A hold of three frames on the last frame: the clip ends 0.3 s later.
    let mut r = slash();
    let e = r.actor();
    r.set(e, "Feel.windup", 2.0f32);
    r.set(e, "Feel.hold", 3.0f32);
    r.tick(1);
    let p = r.world.strings().intern("slash");
    r.world.update::<Animator, _>(e, |a| a.play = p).unwrap();
    r.tick(1);
    let mut n = 0;
    while r.state(e) == "slash" {
        r.tick(1);
        n += 1;
    }
    assert!(
        (54..=56).contains(&n),
        "0.6 s of clip plus a 0.3 s hold: {n} ticks"
    );
    // Speed is a tune field too: double speed, half the time.
    let mut r = slash();
    let e = r.actor();
    r.set(e, "Feel.windup", 2.0f32);
    r.world.update::<Animator, _>(e, |a| a.speed = 2.0).unwrap();
    r.tick(1);
    let n = ticks_to(&mut r, e, 2);
    assert!((5..=7).contains(&n), "half of 0.2 s: {n} ticks");
}

#[test]
fn a_blend_crossfades_the_outgoing_frame_out() {
    let mut r = locomotion();
    let e = r.actor();
    r.set(e, "Body.grounded", true);
    r.set(e, "Body.vx", 3.0f32);
    r.tick(3);
    assert_eq!(r.state(e), "run");
    let out = r.frame(e);
    r.set(e, "Body.vx", 0.0f32);
    r.tick(1);
    assert_eq!(r.state(e), "idle");
    assert_eq!(
        r.world.get_field(e, "Sprite.blend_frame").unwrap(),
        Value::UInt(out)
    );
    let w = |r: &Rig| match r.world.get_field(e, "Sprite.blend").unwrap() {
        Value::Float(f) => f,
        v => panic!("{v:?}"),
    };
    assert!(
        (w(&r) - 1.0).abs() < 1e-6,
        "the outgoing frame starts fully over"
    );
    r.tick(3);
    assert!(
        (w(&r) - 0.5).abs() < 0.01,
        "half way through 0.1 s: {}",
        w(&r)
    );
    r.tick(4);
    assert_eq!(w(&r), 0.0, "gone");
}

#[test]
fn problems_are_reported_once_and_other_animators_run() {
    let bad = LOCOMOTION.replace("Body.grounded and abs(Body.vx) > 0.5", "Hero.speed > 1");
    let mut r = rig(&bad, &[("idle", clip(&[0], 1.0, true))]);
    let e = r.actor();
    r.tick(5);
    let errs = r.named("anim.error");
    assert_eq!(errs.len(), 1, "reported once, not every tick");
    let msg = match field(errs[0], "message") {
        Some(Value::Str(s)) => s.clone(),
        v => panic!("{v:?}"),
    };
    assert!(msg.contains("no component \"Hero\""), "{msg}");
    assert!(
        matches!(field(errs[0], "at"), Some(Value::Str(s)) if s.starts_with("assets/anim/hero.toml:"))
    );
    assert_eq!(
        r.state(e),
        "",
        "the machine did not compile, so nothing ran"
    );

    // A clip the sprite lacks: reported, and a second machine still runs.
    let mut r = rig(LOCOMOTION, &[("idle", clip(&[0], 1.0, true))]);
    {
        let mut s = r.store.borrow_mut();
        let (spec, _) = parse_animator(
            "assets/anim/blink.toml",
            "type = \"animator\"\n[states.on]\nclip = \"idle\"\n",
        )
        .unwrap();
        s.machines.insert("anim/blink".into(), spec);
        s.generation += 1;
    }
    let broken = r.actor();
    let ok = r.actor();
    let blink = r.world.strings().intern("anim/blink");
    r.world
        .update::<Animator, _>(ok, |a| a.machine = blink)
        .unwrap();
    r.tick(3);
    let errs = r.named("anim.error");
    assert_eq!(errs.len(), 1);
    assert!(
        matches!(field(errs[0], "message"), Some(Value::Str(s)) if s.contains("clip \"run\"")),
        "{errs:?}"
    );
    assert_eq!(r.state(broken), "");
    assert_eq!(r.state(ok), "on");
}

/// 2,000 animators, each three states with two-term transitions, running
/// clips at 12 fps. Release only: `cargo test --release -p argus-anim -- --ignored`.
#[test]
#[ignore]
fn perf_two_thousand_animators() {
    let mut r = rig(
        LOCOMOTION,
        &[
            ("idle", clip(&[0, 1, 2, 3], 12.0, true)),
            ("run", clip(&[4, 5, 6, 7, 8, 9], 12.0, true)),
            ("fall", clip(&[10, 11], 12.0, true)),
        ],
    );
    let es: Vec<Entity> = (0..2000).map(|_| r.actor()).collect();
    for (i, &e) in es.iter().enumerate() {
        r.set(e, "Body.grounded", i % 7 != 0);
        r.set(e, "Body.vel", Vec2::new(0.0, -1.0));
    }
    r.tick(2);
    let mut total = std::time::Duration::ZERO;
    let mut worst = std::time::Duration::ZERO;
    for t in 0..300u32 {
        for (i, &e) in es.iter().enumerate() {
            let vx = if (t / 20 + i as u32).is_multiple_of(3) {
                3.0f32
            } else {
                0.0
            };
            r.world.set_field(e, "Body.vx", vx).unwrap();
        }
        let s = std::time::Instant::now();
        r.sched.step(&mut r.world, 1).unwrap();
        let d = s.elapsed();
        r.world.drain_events();
        total += d;
        worst = worst.max(d);
    }
    let rows = r.sched.prof_systems(300).unwrap();
    let anim = rows.iter().find(|row| row.name == "anim.update").unwrap();
    println!(
        "2,000 animators: anim.update mean {:.3} ms, p95 {:.3} ms; tick mean {:?}, worst {worst:?}",
        anim.mean_ms,
        anim.p95_ms,
        total / 300
    );
    assert!(anim.mean_ms < 0.3, "{} ms", anim.mean_ms);
}

/// `or` joins groups of terms (`and` binds tighter), so idle to walk is one
/// transition, not two; and `face` mirrors the sprite while its field is
/// below zero, keeping the way it faced at zero (the art passes wrote a
/// script for both).
#[test]
fn or_joins_terms_and_face_mirrors_the_sprite_by_a_field() {
    let mut r = rig(
        r#"type = "animator"
initial = "idle"
face = "Body.vx"

[states.idle]
clip = "idle"

[[states.idle.transitions]]
to = "walk"
when = "Body.grounded and Body.vx > 0.5 or Body.grounded and Body.vx < -0.5 or Body.dash > 0"

[states.walk]
clip = "walk"

[[states.walk.transitions]]
to = "idle"
when = "abs(Body.vx) <= 0.5 and Body.dash == 0"
"#,
        &[
            ("idle", clip(&[0], 1.0, true)),
            ("walk", clip(&[1, 2], 8.0, true)),
        ],
    );
    let e = r.actor();
    let flip = |r: &Rig| r.world.get_field(e, "Sprite.flip_x").unwrap() == Value::Bool(true);
    r.set(e, "Body.grounded", true);
    r.tick(1);
    assert_eq!(r.state(e), "idle");
    // Walking left: the second group holds, and the sprite is mirrored.
    r.set(e, "Body.vx", -2.0f32);
    r.tick(1);
    assert_eq!(r.state(e), "walk");
    assert!(flip(&r), "facing left");
    // Stopped: idle again, still facing left.
    r.set(e, "Body.vx", 0.0f32);
    r.tick(1);
    assert_eq!(r.state(e), "idle");
    assert!(flip(&r), "a stop keeps the facing");
    // Right: the first group; facing right.
    r.set(e, "Body.vx", 2.0f32);
    r.tick(1);
    assert_eq!(r.state(e), "walk");
    assert!(!flip(&r), "facing right");
    // In the air, neither and-group holds; the third (a dash) alone does.
    r.set(e, "Body.vx", 0.0f32);
    r.tick(1);
    assert_eq!(r.state(e), "idle");
    r.set(e, "Body.grounded", false);
    r.set(e, "Body.vx", 2.0f32);
    r.tick(1);
    assert_eq!(r.state(e), "idle", "airborne: no group holds");
    r.set(e, "Body.dash", 1i32);
    r.tick(1);
    assert_eq!(r.state(e), "walk", "the dash group");
}
