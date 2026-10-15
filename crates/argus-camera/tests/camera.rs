//! §12 Camera: follow with a dead zone, lookahead, smoothing, and room
//! bounds; free pan and zoom; trauma shake with a curve and a decay; cameras
//! are entities and a cut between two is an event.

use argus_camera::{current, Camera, CameraShake, Zones};
use argus_ecs::{Entity, Event, FieldType, SchemaBuilder, Transform, Value, Vec2, World};
use argus_sim::{Scheduler, SchedulerConfig, Stage, SystemDesc};
use std::cell::RefCell;
use std::rc::Rc;

struct Rig {
    world: World,
    sched: Scheduler,
    zones: Rc<RefCell<Zones>>,
    events: Vec<(String, Vec<(String, Value)>)>,
}

fn rig() -> Rig {
    let mut world = World::new();
    argus_ecs::hierarchy::register(&mut world).unwrap();
    // A stand-in for the action map's generated component.
    let actions = SchemaBuilder::new("Actions")
        .field("pan", FieldType::Vec2)
        .field("zoom", FieldType::F32)
        .field("device", FieldType::F32)
        .build()
        .unwrap();
    world.register_schema(actions).unwrap();
    let mut sched = Scheduler::new(SchedulerConfig::continuous(60.0).seed(1));
    let zones = Rc::new(RefCell::new(Zones::default()));
    argus_camera::install(&mut world, &mut sched, zones.clone()).unwrap();
    Rig {
        world,
        sched,
        zones,
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
    fn body(&mut self, x: f32, y: f32) -> Entity {
        let e = self.world.spawn();
        self.world.insert(e, Transform::at(x, y)).unwrap();
        e
    }
    fn camera(&mut self, x: f32, y: f32, c: Camera) -> Entity {
        let e = self.body(x, y);
        self.world.insert(e, c).unwrap();
        e
    }
    fn at(&self, e: Entity) -> Vec2 {
        self.world.get::<Transform>(e).unwrap().pos
    }
    fn go(&mut self, e: Entity, x: f32, y: f32) {
        self.world
            .update::<Transform, _>(e, |t| t.pos = Vec2::new(x, y))
            .unwrap();
    }
    fn cam(&self, e: Entity) -> Camera {
        self.world.get::<Camera>(e).unwrap()
    }
    fn named(&self, n: &str) -> Vec<&Vec<(String, Value)>> {
        self.events
            .iter()
            .filter(|e| e.0 == n)
            .map(|e| &e.1)
            .collect()
    }
}

/// A camera that follows exactly: no dead zone, lookahead, or smoothing.
fn exact(target: Entity) -> Camera {
    Camera {
        height: 10.0,
        aspect: 2.0,
        target,
        dead_zone: Vec2::ZERO,
        lookahead: Vec2::ZERO,
        smoothing: Vec2::ZERO,
        ..Default::default()
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn a_following_camera_snaps_on_then_holds_while_the_target_is_in_its_dead_zone() {
    let mut r = rig();
    let t = r.body(5.0, 2.0);
    let cam = r.camera(
        0.0,
        0.0,
        Camera {
            dead_zone: Vec2::new(1.0, 0.5),
            offset: Vec2::new(0.0, 1.0),
            ..exact(t)
        },
    );
    r.tick(1);
    let p = r.at(cam);
    assert!(
        close(p.x, 5.0) && close(p.y, 3.0),
        "on at the aim at once: {p:?}"
    );
    assert!(r.cam(cam).tracking);
    // Inside the dead zone: the camera holds.
    r.go(t, 5.8, 2.3);
    r.tick(1);
    let p = r.at(cam);
    assert!(close(p.x, 5.0) && close(p.y, 3.0), "{p:?}");
    // Past its edge: dragged along so the aim sits on the edge.
    r.go(t, 8.0, 2.0);
    r.tick(1);
    let p = r.at(cam);
    assert!(close(p.x, 7.0), "aim 8 minus the dead zone's 1: {p:?}");
    // The view rect is the camera's, height 10 at aspect 2.
    let c = r.cam(cam);
    assert!(
        close(c.view_min.x, 7.0 - 10.0) && close(c.view_max.y, 3.0 + 5.0),
        "{c:?}"
    );
}

#[test]
fn lookahead_leads_a_moving_target_and_is_capped() {
    let mut r = rig();
    let t = r.body(0.0, 0.0);
    let cam = r.camera(
        0.0,
        0.0,
        Camera {
            lookahead: Vec2::new(0.25, 0.0),
            lookahead_max: Vec2::new(3.0, 0.0),
            lookahead_smoothing: 0.0,
            ..exact(t)
        },
    );
    r.tick(1);
    // 6 units a second: a quarter second ahead is 1.5.
    for i in 1..=30 {
        r.go(t, i as f32 * 0.1, 0.0);
        r.tick(1);
    }
    let (x, tx) = (r.at(cam).x, r.at(t).x);
    assert!(close(x - tx, 1.5), "ahead by {}", x - tx);
    // 30 units a second would be 7.5 ahead: capped at 3.
    for i in 1..=30 {
        r.go(t, 3.0 + i as f32 * 0.5, 0.0);
        r.tick(1);
    }
    let (x, tx) = (r.at(cam).x, r.at(t).x);
    assert!(close(x - tx, 3.0), "capped: {}", x - tx);
    // Standing still, the lookahead goes.
    r.tick(2);
    assert!(close(r.at(cam).x, r.at(t).x));
}

#[test]
fn smoothing_is_a_time_constant() {
    let mut r = rig();
    let t = r.body(0.0, 0.0);
    let cam = r.camera(
        0.0,
        0.0,
        Camera {
            smoothing: Vec2::new(0.2, 0.0),
            ..exact(t)
        },
    );
    r.tick(1);
    r.go(t, 10.0, 0.0);
    // 0.2 s later the camera has covered 1 - 1/e of the way, whatever the tick rate.
    r.tick(12);
    let x = r.at(cam).x;
    assert!((x - 10.0 * (1.0 - (-1.0f32).exp())).abs() < 0.01, "{x}");
    r.tick(120);
    assert!((r.at(cam).x - 10.0).abs() < 0.01);
}

#[test]
fn room_bounds_keep_the_view_inside_and_a_new_room_slides_in() {
    let mut r = rig();
    r.zones.borrow_mut().rects = vec![
        ("a".into(), [0.0, 0.0, 20.0, 12.0]),
        ("b".into(), [20.0, 0.0, 60.0, 12.0]),
        ("narrow".into(), [60.0, 0.0, 64.0, 12.0]),
    ];
    let t = r.body(2.0, 2.0);
    let cam = r.camera(
        0.0,
        0.0,
        Camera {
            smoothing: Vec2::new(0.1, 0.1),
            ..exact(t)
        },
    );
    r.tick(1);
    // Room a is exactly as wide as the view (20): centred. Low down, the view's
    // bottom sits on the room's floor.
    let p = r.at(cam);
    assert!(close(p.x, 10.0) && close(p.y, 5.0), "{p:?}");
    let z = r.named("camera.zone");
    assert_eq!(z.len(), 1);
    assert_eq!(z[0][1], ("zone".to_string(), Value::Str("a".into())));
    // Into room b: the camera slides over, never jumping.
    r.go(t, 35.0, 6.0);
    let mut last = r.at(cam).x;
    let mut outside = false;
    for _ in 0..90 {
        r.tick(1);
        let x = r.at(cam).x;
        assert!(x >= last - 1e-4 && x - last < 5.0, "slides: {last} to {x}");
        outside |= r.cam(cam).view_min.x < 20.0;
        last = x;
    }
    assert!(outside, "the slide shows the way between the rooms");
    let p = r.at(cam);
    assert!(
        close(p.x, 35.0) && close(p.y, 6.0),
        "settled on the target inside b: {p:?}"
    );
    assert_eq!(r.named("camera.zone").len(), 2);
    // Near b's far wall the view stops at it.
    r.go(t, 58.0, 6.0);
    r.tick(120);
    assert!(close(r.cam(cam).view_max.x, 60.0), "{:?}", r.cam(cam));
    // A room narrower than the view: centred on it.
    r.go(t, 62.0, 6.0);
    r.tick(120);
    assert!(close(r.at(cam).x, 62.0));
    // Unbounded, the camera goes where the target goes.
    r.world
        .update::<Camera, _>(cam, |c| c.bounded = false)
        .unwrap();
    r.go(t, 100.0, 50.0);
    r.tick(120);
    assert!(close(r.at(cam).x, 100.0) && close(r.at(cam).y, 50.0));
}

#[test]
fn played_by_touch_the_target_is_framed_clear_of_the_thumbs() {
    let mut r = rig();
    r.zones.borrow_mut().rects = vec![("a".into(), [0.0, 0.0, 20.0, 14.0])];
    // A player standing on the room's floor, its Actions saying which kind
    // of device it plays with (the input stage writes it; 0 the keyboard).
    let t = r.body(10.0, 1.0);
    let actions = r.world.component_id("Actions").unwrap();
    r.world.insert_default(t, actions).unwrap();
    let cam = r.camera(
        0.0,
        0.0,
        Camera {
            touch_offset: Vec2::new(0.0, -4.0),
            smoothing: Vec2::new(0.1, 0.1),
            ..exact(t)
        },
    );
    r.tick(1);
    // On the keyboard the view stops at the floor (height 10: centre 5), and
    // the player is drawn near the bottom of the screen.
    assert!(close(r.at(cam).y, 5.0), "{:?}", r.at(cam));
    // By touch the aim is 4 lower and the view may pass the floor by 4: the
    // player is in the middle, over a strip below the room. It glides there.
    r.world
        .set_field(t, "Actions.device", argus_camera::TOUCH_DEVICE)
        .unwrap();
    let mut last = r.at(cam).y;
    for _ in 0..120 {
        r.tick(1);
        let y = r.at(cam).y;
        assert!(y <= last + 1e-4 && last - y < 1.0, "glides: {last} to {y}");
        last = y;
    }
    assert!(close(r.at(cam).y, 1.0), "{:?}", r.at(cam));
    assert!(close(r.cam(cam).view_min.y, -4.0), "{:?}", r.cam(cam));
    // Never further than the touch offset: a player in a pit below the floor
    // still sees the view stop 4 under it.
    r.go(t, 10.0, -6.0);
    r.tick(120);
    assert!(close(r.cam(cam).view_min.y, -4.0), "{:?}", r.cam(cam));
    // Back on the keyboard, the framing and bounds are as before.
    r.go(t, 10.0, 1.0);
    r.world.set_field(t, "Actions.device", 0.0f32).unwrap();
    r.tick(120);
    assert!(close(r.at(cam).y, 5.0), "{:?}", r.at(cam));
    // A camera with no touch offset frames the same on any device.
    let plain = r.camera(0.0, 0.0, exact(t));
    r.world
        .set_field(t, "Actions.device", argus_camera::TOUCH_DEVICE)
        .unwrap();
    r.tick(2);
    assert!(close(r.at(plain).y, 5.0), "{:?}", r.at(plain));
}

#[test]
fn a_free_camera_pans_and_zooms_with_actions() {
    let mut r = rig();
    let pan = r.world.strings().intern("pan");
    let zoom = r.world.strings().intern("zoom");
    let cam = r.camera(
        0.0,
        0.0,
        Camera {
            height: 10.0,
            pan_action: pan,
            zoom_action: zoom,
            pan_speed: 1.0,
            zoom_speed: 1.5,
            zoom_min: 4.0,
            ..Default::default()
        },
    );
    let actions = r.world.component_id("Actions").unwrap();
    r.world.insert_default(cam, actions).unwrap();
    r.world
        .set_field(cam, "Actions.pan", Vec2::new(1.0, 0.0))
        .unwrap();
    r.tick(60);
    // One view height a second at height 10.
    assert!((r.at(cam).x - 10.0).abs() < 0.01, "{}", r.at(cam).x);
    r.world.set_field(cam, "Actions.pan", Vec2::ZERO).unwrap();
    r.world.set_field(cam, "Actions.zoom", 1.0f32).unwrap();
    r.tick(30);
    // Half a second of full zoom-in: height times e^-0.75.
    assert!(
        (r.cam(cam).height - 10.0 * (-0.75f32).exp()).abs() < 0.01,
        "{}",
        r.cam(cam).height
    );
    r.tick(60);
    assert!(close(r.cam(cam).height, 4.0), "held at zoom_min");
    // Bounds hold a free camera too.
    r.zones.borrow_mut().rects = vec![("map".into(), [0.0, 0.0, 30.0, 30.0])];
    r.world.set_field(cam, "Actions.zoom", 0.0f32).unwrap();
    r.world
        .set_field(cam, "Actions.pan", Vec2::new(-1.0, 0.0))
        .unwrap();
    r.tick(240);
    let c = r.cam(cam);
    assert!(
        close(c.view_min.x, 0.0),
        "panned to the map's edge and stopped: {c:?}"
    );
}

#[test]
fn a_camera_on_the_player_itself_is_left_where_its_entity_is() {
    let mut r = rig();
    r.zones.borrow_mut().rects = vec![("a".into(), [0.0, 0.0, 20.0, 12.0])];
    let player = r.camera(
        1.0,
        1.0,
        Camera {
            height: 10.0,
            aspect: 2.0,
            ..Default::default()
        },
    );
    r.tick(3);
    let p = r.at(player);
    assert!(
        close(p.x, 1.0) && close(p.y, 1.0),
        "the system never moves an entity it does not steer: {p:?}"
    );
    let c = r.cam(player);
    assert!(close(c.view_min.x, -9.0) && close(c.view_max.x, 11.0));
}

/// A system that raises trauma through the event at tick 2.
fn shaker(r: &mut Rig) {
    r.sched
        .add_system(
            SystemDesc::new("sim.hit", Stage::Sim),
            Box::new(|ctx| {
                if ctx.tick == 2 {
                    ctx.emit("camera.shake", vec![("amount".into(), Value::Float(0.8))]);
                }
                Ok(())
            }),
        )
        .unwrap();
}

#[test]
fn shake_follows_trauma_through_its_curve_decays_and_replays_the_same() {
    let run = || {
        let mut r = rig();
        shaker(&mut r);
        let cam = r.camera(
            3.0,
            4.0,
            Camera {
                height: 10.0,
                aspect: 2.0,
                ..Default::default()
            },
        );
        r.world.insert(cam, CameraShake::default()).unwrap();
        let mut offsets = Vec::new();
        let mut traumas = Vec::new();
        for _ in 0..70 {
            r.tick(1);
            let s = r.world.get::<CameraShake>(cam).unwrap();
            offsets.push((
                s.offset.x.to_bits(),
                s.offset.y.to_bits(),
                s.angle.to_bits(),
            ));
            traumas.push(s.trauma);
            let k = (s.trauma + s.decay / 60.0).min(1.0).powf(s.curve);
            assert!(
                s.offset.x.abs() <= s.max_offset.x * k + 1e-5
                    && s.angle.abs() <= s.max_angle * k + 1e-5
            );
            // The entity stays put; the view moves.
            let p = r.at(cam);
            assert!(close(p.x, 3.0) && close(p.y, 4.0));
            let c = r.cam(cam);
            assert!(close(c.view_min.x, 3.0 + s.offset.x - 10.0), "{c:?} {s:?}");
        }
        (offsets, traumas)
    };
    let (offsets, traumas) = run();
    assert_eq!(traumas[1], 0.0, "no trauma before the hit");
    assert!(
        (traumas[2] - (0.8 - 1.5 / 60.0)).abs() < 1e-5,
        "raised to 0.8, one tick of decay: {}",
        traumas[2]
    );
    assert!(traumas[2..].windows(2).all(|w| w[1] <= w[0]), "only decays");
    assert_eq!(*traumas.last().unwrap(), 0.0, "gone after 0.8 / 1.5 s");
    assert!(offsets[5..20].iter().any(|o| o.0 != 0), "it shook");
    assert_eq!(offsets.last().unwrap().0, 0f32.to_bits(), "and stopped");
    assert_eq!(run().0, offsets, "the same shake on a replay");
}

#[test]
fn a_hit_stop_holds_the_view_and_the_shake_plays_on() {
    let mut r = rig();
    // Stops ticks 10 to 14 (phase 2 item 6) and shakes on the first of them.
    r.sched
        .add_system(
            SystemDesc::new("sim.stop", Stage::Input).first(),
            Box::new(|ctx| {
                if (10..15).contains(&ctx.tick) {
                    ctx.freeze();
                }
                if ctx.tick == 10 {
                    ctx.emit("camera.shake", vec![("amount".into(), Value::Float(1.0))]);
                }
                Ok(())
            }),
        )
        .unwrap();
    let t = r.body(0.0, 0.0);
    let cam = r.camera(
        0.0,
        0.0,
        Camera {
            smoothing: Vec2::new(0.2, 0.0),
            ..exact(t)
        },
    );
    r.world.insert(cam, CameraShake::default()).unwrap();
    r.tick(5);
    r.go(t, 10.0, 0.0);
    r.tick(5);
    let held = r.at(cam).x;
    assert!(held > 0.5 && held < 9.0, "on its way: {held}");
    let mut offsets = Vec::new();
    for _ in 0..5 {
        r.tick(1);
        assert_eq!(r.at(cam).x, held, "the view holds through the stop");
        let s = r.world.get::<CameraShake>(cam).unwrap();
        assert!(close(r.cam(cam).view_min.x, held + s.offset.x - 10.0));
        offsets.push(s.offset);
    }
    assert!(
        offsets.windows(2).all(|w| w[0] != w[1]),
        "and shakes on: {offsets:?}"
    );
    r.tick(1);
    assert!(r.at(cam).x > held, "then moves on");
}

#[test]
fn the_highest_priority_active_camera_draws_and_a_change_is_a_cut() {
    let mut r = rig();
    let a = r.camera(0.0, 0.0, Camera::default());
    let b = r.camera(
        9.0,
        0.0,
        Camera {
            priority: -1,
            ..Default::default()
        },
    );
    r.tick(1);
    assert_eq!(current(&r.world), Some(a));
    assert!(
        r.named("camera.cut").is_empty(),
        "the first choice is not a cut"
    );
    r.world.update::<Camera, _>(b, |c| c.priority = 3).unwrap();
    r.tick(1);
    assert_eq!(current(&r.world), Some(b));
    let cuts = r.named("camera.cut");
    assert_eq!(cuts.len(), 1);
    assert_eq!(
        *cuts[0],
        vec![
            ("from".into(), Value::Entity(a)),
            ("to".into(), Value::Entity(b))
        ]
    );
    r.world
        .update::<Camera, _>(b, |c| c.active = false)
        .unwrap();
    r.tick(1);
    assert_eq!(current(&r.world), Some(a));
    assert_eq!(r.named("camera.cut").len(), 2);
    // Equal priority: the older entity.
    r.world
        .update::<Camera, _>(b, |c| {
            c.active = true;
            c.priority = 0;
        })
        .unwrap();
    assert_eq!(current(&r.world), Some(a));
}

#[test]
fn a_camera_with_a_blend_glides_from_the_view_before() {
    let mut r = rig();
    let a = r.camera(0.0, 0.0, Camera::default());
    let b = r.camera(
        30.0,
        0.0,
        Camera {
            active: false,
            priority: 2,
            height: 20.0,
            blend: 0.5,
            ..Default::default()
        },
    );
    r.tick(1);
    assert!(r.cam(a).drew && !r.cam(b).drew);
    r.world.update::<Camera, _>(b, |c| c.active = true).unwrap();
    r.tick(1);
    let cuts = r.named("camera.cut");
    assert_eq!(cuts.len(), 1);
    assert!(cuts[0].contains(&("blend".to_string(), Value::Float(0.5))));
    // It starts from where a was looking and gets there in half a second,
    // easing in and out: slow at first, then all the way.
    let mid = |r: &Rig| {
        let c = r.cam(b);
        (
            (c.view_min.x + c.view_max.x) * 0.5,
            c.view_max.y - c.view_min.y,
        )
    };
    let (x0, h0) = mid(&r);
    assert!(x0 < 1.0 && h0 > 39.0, "from a's view: {x0}, {h0}");
    let v = argus_camera::view(&r.world, 16.0 / 9.0).unwrap();
    assert!((v.center[0] - x0).abs() < 1e-3, "what draws is the blend");
    r.tick(14);
    let (x1, h1) = mid(&r);
    assert!(
        x1 > 10.0 && x1 < 20.0 && h1 < 40.0 && h1 > 20.0,
        "half way: {x1}, {h1}"
    );
    r.tick(16);
    let (x2, h2) = mid(&r);
    assert!(close(x2, 30.0) && close(h2, 20.0), "there: {x2}, {h2}");
    assert_eq!(r.cam(b).blend_left, 0.0);
    // Back to a: its blend is 0, so that is a cut.
    r.world
        .update::<Camera, _>(b, |c| c.active = false)
        .unwrap();
    r.tick(1);
    let (xa, _) = {
        let c = r.cam(a);
        ((c.view_min.x + c.view_max.x) * 0.5, 0.0)
    };
    assert!(close(xa, 0.0));
    assert_eq!(r.named("camera.cut").len(), 2);
    assert!(!r.named("camera.cut")[1].iter().any(|f| f.0 == "blend"));
}
