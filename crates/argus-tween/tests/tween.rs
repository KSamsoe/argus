//! §13 Tweens: a tween is a component (target path, from, to, duration,
//! easing curve, on-complete event), queryable, and reproduced exactly by a
//! replay.

use argus_ecs::{Entity, Event, FieldType, SchemaBuilder, Transform, Value, Vec2, World};
use argus_sim::{Scheduler, SchedulerConfig};
use argus_tween::ease::{ease, NAMES};
use argus_tween::Tween;

struct Rig {
    world: World,
    sched: Scheduler,
    events: Vec<(String, Vec<(String, Value)>)>,
}

fn rig() -> Rig {
    let mut world = World::new();
    argus_ecs::hierarchy::register(&mut world).unwrap();
    let score = SchemaBuilder::new("Score")
        .field("points", FieldType::I32)
        .field_full(
            "glow",
            FieldType::F32,
            Value::Float(0.0),
            Some((0.0, 1.0)),
            "0 to 1",
            true,
        )
        .field("name", FieldType::Str)
        .build()
        .unwrap();
    world.register_schema(score).unwrap();
    let mut sched = Scheduler::new(SchedulerConfig::continuous(60.0).seed(1));
    argus_tween::install(&mut world, &mut sched).unwrap();
    Rig {
        world,
        sched,
        events: Vec::new(),
    }
}

impl Rig {
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
    fn thing(&mut self) -> Entity {
        let e = self.world.spawn();
        self.world.insert(e, Transform::at(0.0, 0.0)).unwrap();
        let s = self.world.component_id("Score").unwrap();
        self.world.insert_default(e, s).unwrap();
        e
    }
    fn tween(&mut self, target: Entity, path: &str, t: Tween) -> Entity {
        let e = self.world.spawn();
        let path = self.world.strings().intern(path);
        self.world.insert(e, Tween { target, path, ..t }).unwrap();
        e
    }
    fn s(&self, x: &str) -> argus_ecs::StrId {
        self.world.strings().intern(x)
    }
    fn scale(&self, e: Entity) -> Vec2 {
        self.world.get::<Transform>(e).unwrap().scale
    }
    fn named(&self, n: &str) -> Vec<&Vec<(String, Value)>> {
        self.events
            .iter()
            .filter(|e| e.0 == n)
            .map(|e| &e.1)
            .collect()
    }
}

fn close(a: Vec2, b: Vec2) -> bool {
    (a.x - b.x).abs() < 1e-4 && (a.y - b.y).abs() < 1e-4
}

#[test]
fn every_easing_starts_at_0_ends_at_1_and_has_its_shape() {
    for n in NAMES {
        assert!(ease(n, 0.0).unwrap().abs() < 1e-5, "{n} at 0");
        assert!((ease(n, 1.0).unwrap() - 1.0).abs() < 1e-5, "{n} at 1");
    }
    assert_eq!(ease("in_quad", 0.5), Some(0.25));
    assert_eq!(ease("out_quad", 0.5), Some(0.75));
    assert!((ease("in_out_cubic", 0.5).unwrap() - 0.5).abs() < 1e-6);
    assert!(
        ease("out_back", 0.7).unwrap() > 1.0,
        "back overshoots: the squash reads"
    );
    assert!(ease("in_back", 0.2).unwrap() < 0.0, "and winds up first");
    assert_eq!(ease("step", 0.99), Some(0.0));
    assert_eq!(ease("wobble", 0.5), None);
}

#[test]
fn a_tween_drives_its_field_to_the_end_emits_its_event_and_goes() {
    let mut r = rig();
    let e = r.thing();
    let done = r.s("grow.done");
    let t = r.tween(
        e,
        "Transform.scale",
        Tween {
            from: Vec2::new(1.0, 1.0),
            to: Vec2::new(2.0, 0.5),
            from_current: false,
            duration: 0.5,
            on_complete: done,
            ..Default::default()
        },
    );
    // What is animating is a query.
    let tw = r.world.component_id_of::<Tween>().unwrap();
    assert_eq!(r.world.query().has(tw).count(), 1);
    r.tick(15);
    assert!(
        close(r.scale(e), Vec2::new(1.5, 0.75)),
        "half way, linear: {:?}",
        r.scale(e)
    );
    assert!((r.world.get::<Tween>(t).unwrap().time - 0.25).abs() < 1e-4);
    r.tick(15);
    assert!(close(r.scale(e), Vec2::new(2.0, 0.5)), "{:?}", r.scale(e));
    let ev = r.named("grow.done");
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0][0], ("entity".to_string(), Value::Entity(e)));
    assert_eq!(ev[0][1], ("tween".to_string(), Value::Entity(t)));
    assert_eq!(
        ev[0][2],
        ("path".to_string(), Value::Str("Transform.scale".into()))
    );
    assert!(!r.world.is_alive(t), "done tweens go");
    assert_eq!(r.world.query().has(tw).count(), 0, "nothing animating");
    r.tick(5);
    assert!(
        close(r.scale(e), Vec2::new(2.0, 0.5)),
        "and the value stays"
    );
    // The system that wrote Transform says so.
    let who = r
        .sched
        .who_writes(r.world.component_id_of::<Transform>().unwrap())
        .unwrap();
    assert_eq!(who, ["tween.update"]);
}

#[test]
fn delay_repeat_and_yoyo() {
    let mut r = rig();
    let e = r.thing();
    let keep = r.tween(
        e,
        "Transform.pos.x",
        Tween {
            from: Vec2::new(0.0, 0.0),
            to: Vec2::new(10.0, 0.0),
            from_current: false,
            duration: 0.5,
            delay: 0.25,
            repeats: 1,
            yoyo: true,
            keep: true,
            ..Default::default()
        },
    );
    r.tick(15);
    assert_eq!(
        r.world.get::<Transform>(e).unwrap().pos.x,
        0.0,
        "still in its delay"
    );
    assert!(!r.world.get::<Tween>(keep).unwrap().started);
    r.tick(30);
    let x = r.world.get::<Transform>(e).unwrap().pos.x;
    assert!((x - 10.0).abs() < 0.4, "one play: at the end, turning: {x}");
    r.tick(30);
    let t = r.world.get::<Tween>(keep).unwrap();
    assert!(t.done && t.plays == 1);
    assert!(
        r.world.get::<Transform>(e).unwrap().pos.x.abs() < 1e-4,
        "there and back"
    );
    assert!(r.world.is_alive(keep), "keep keeps it");
    // Forever never finishes.
    let spin = r.tween(
        e,
        "Transform.rot",
        Tween {
            to: Vec2::new(std::f32::consts::TAU, 0.0),
            duration: 0.1,
            repeats: -1,
            ..Default::default()
        },
    );
    r.tick(120);
    let t = r.world.get::<Tween>(spin).unwrap();
    assert!(!t.done && t.plays >= 19, "{t:?}");
}

#[test]
fn from_current_one_axis_integers_and_ranges() {
    let mut r = rig();
    let e = r.thing();
    r.world
        .update::<Transform, _>(e, |t| t.pos = Vec2::new(3.0, 4.0))
        .unwrap();
    // From where it is, one axis only: x is untouched.
    r.tween(
        e,
        "Transform.pos.y",
        Tween {
            to: Vec2::new(8.0, 0.0),
            duration: 0.2,
            ..Default::default()
        },
    );
    // An integer field rounds; a ranged field is held inside its range
    // however far the curve overshoots.
    let back = r.s("out_back");
    r.tween(
        e,
        "Score.points",
        Tween {
            to: Vec2::new(10.0, 0.0),
            duration: 0.2,
            ..Default::default()
        },
    );
    r.tween(
        e,
        "Score.glow",
        Tween {
            to: Vec2::new(1.0, 0.0),
            duration: 0.2,
            easing: back,
            ..Default::default()
        },
    );
    let mut over = false;
    for _ in 0..12 {
        r.tick(1);
        let g = match r.world.get_field(e, "Score.glow").unwrap() {
            Value::Float(f) => f,
            v => panic!("{v:?}"),
        };
        assert!(g <= 1.0, "clamped to its range: {g}");
        over |= g >= 1.0 - 1e-6;
    }
    assert!(over, "out_back reached the top of the range early");
    let p = r.world.get::<Transform>(e).unwrap().pos;
    assert!(close(p, Vec2::new(3.0, 8.0)), "{p:?}");
    assert_eq!(
        r.world.get_field(e, "Score.points").unwrap(),
        Value::Int(10)
    );
    assert!(r.named("tween.error").is_empty(), "{:?}", r.events);
}

#[test]
fn bad_tweens_fail_once_and_go() {
    let mut r = rig();
    let e = r.thing();
    let bare = r.world.spawn();
    let wobble = r.s("wobble");
    let a = r.tween(e, "Nothing.here", Tween::default());
    let b = r.tween(e, "Score.name", Tween::default());
    let c = r.tween(bare, "Transform.scale", Tween::default());
    let d = r.tween(
        e,
        "Transform.scale",
        Tween {
            easing: wobble,
            ..Default::default()
        },
    );
    let gone_target = r.thing();
    let f = r.tween(gone_target, "Transform.scale", Tween::default());
    r.world.despawn(gone_target).unwrap();
    r.tick(3);
    let errs = r.named("tween.error");
    assert_eq!(errs.len(), 4, "{errs:?}");
    let msg = |i: usize| match &errs[i][1].1 {
        Value::Str(s) => s.clone(),
        v => panic!("{v:?}"),
    };
    assert!(msg(0).contains("Nothing"), "{}", msg(0));
    assert!(msg(1).contains("drives numbers and vec2s"), "{}", msg(1));
    assert!(msg(2).contains("has no Transform"), "{}", msg(2));
    assert!(msg(3).contains("easing \"wobble\""), "{}", msg(3));
    for t in [a, b, c, d, f] {
        assert!(!r.world.is_alive(t));
    }
}

#[test]
fn the_same_run_twice_is_the_same() {
    let run = || {
        let mut r = rig();
        let e = r.thing();
        let el = r.s("out_elastic");
        r.tween(
            e,
            "Transform.scale",
            Tween {
                to: Vec2::new(1.4, 0.6),
                duration: 0.37,
                easing: el,
                repeats: 3,
                yoyo: true,
                ..Default::default()
            },
        );
        let mut out = Vec::new();
        for _ in 0..100 {
            r.tick(1);
            let s = r.scale(e);
            out.push((s.x.to_bits(), s.y.to_bits()));
        }
        out
    };
    assert_eq!(run(), run());
}
