//! Skeletal 2D (phase 5 item 8): §13 "Skeletal 2D via a Spine-compatible
//! runtime" read as the engine's own runtime posing Spine 4 JSON; §8 what
//! happens is an event; §6 the same run reaches the same world and a seek
//! replays it; "costs nothing" without the components.

use argus_anim::{AnimStore, Animator};
use argus_assets::{encode_png, Asset, Importer};
use argus_ecs::{
    Entity, Event, FieldType, GlobalTransform, Parent, SchemaBuilder, StrId, Transform, Value,
    Vec2, World,
};
use argus_sim::{Scheduler, SchedulerConfig};
use argus_skeletal::pose;
use argus_skeletal::{Skeleton, SkeletonPart};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// A puppet: a hip, a torso with a head and an arm, a badge that keeps
/// upright (onlyTranslation), and a two-bone leg bent to a target by IK.
pub const PUPPET: &str = r#"{
"skeleton": { "hash": "test", "spine": "4.1.24", "fps": 30, "images": "./images/", "audio": "" },
"bones": [
  { "name": "root" },
  { "name": "hip", "parent": "root", "y": 100 },
  { "name": "torso", "parent": "hip", "length": 60, "rotation": 90 },
  { "name": "head", "parent": "torso", "x": 60 },
  { "name": "arm", "parent": "torso", "x": 50, "rotation": 90, "length": 40 },
  { "name": "badge", "parent": "torso", "x": 30, "inherit": "onlyTranslation" },
  { "name": "thigh", "parent": "hip", "rotation": -90, "length": 50 },
  { "name": "shin", "parent": "thigh", "x": 50, "length": 50 },
  { "name": "target", "parent": "root", "x": 30, "y": 20 }
],
"slots": [
  { "name": "leg", "bone": "thigh", "attachment": "thigh" },
  { "name": "shin", "bone": "shin", "attachment": "shin" },
  { "name": "torso", "bone": "torso", "attachment": "torso" },
  { "name": "arm", "bone": "arm", "attachment": "arm" },
  { "name": "head", "bone": "head", "attachment": "head" },
  { "name": "eyes", "bone": "head", "attachment": "eyes-open" },
  { "name": "badge", "bone": "badge", "attachment": "badge", "color": "00ff00ff" }
],
"ik": [
  { "name": "leg", "order": 0, "bones": [ "thigh", "shin" ], "target": "target", "bendPositive": false }
],
"skins": [
  {
    "name": "default",
    "attachments": {
      "leg": { "thigh": { "x": 25, "rotation": 90, "width": 10, "height": 50 } },
      "shin": { "shin": { "x": 25, "rotation": 90, "width": 10, "height": 50 } },
      "torso": { "torso": { "x": 30, "rotation": -90, "width": 20, "height": 60 } },
      "arm": {
        "arm": { "x": 20, "width": 40, "height": 8 },
        "arm-mesh": { "type": "mesh", "uvs": [], "triangles": [], "vertices": [], "hull": 0 }
      },
      "head": { "head": { "x": 10, "width": 30, "height": 30 } },
      "eyes": {
        "eyes-open": { "x": 12, "width": 20, "height": 6 },
        "eyes-closed": { "x": 12, "width": 20, "height": 2 }
      },
      "badge": { "badge": { "width": 8, "height": 8 } }
    }
  },
  {
    "name": "armored",
    "attachments": {
      "head": { "head": { "path": "helmet", "x": 10, "width": 34, "height": 34 } }
    }
  }
],
"events": {
  "step": { "int": 1 },
  "waved": { "string": "hello" }
},
"animations": {
  "walk": {
    "bones": {
      "hip": { "translate": [ { "y": 0 }, { "time": 0.5, "y": 10 }, { "time": 1, "y": 0 } ] },
      "torso": { "rotate": [ { "value": 0, "curve": [ 0.1, 0, 0.2, 10 ] }, { "time": 0.5, "value": 10 }, { "time": 1, "value": 0 } ] },
      "target": { "translate": [ { "x": -20 }, { "time": 0.5, "x": 10 }, { "time": 1, "x": -20 } ] }
    },
    "events": [ { "time": 0.25, "name": "step" }, { "time": 0.75, "name": "step", "int": 2 } ]
  },
  "blink": {
    "slots": {
      "eyes": { "attachment": [ { "name": "eyes-closed" }, { "time": 0.2, "name": "eyes-open" } ] },
      "head": { "rgba": [ { "color": "ffffffff" }, { "time": 0.4, "color": "ff000080" } ] }
    },
    "drawOrder": [ { "time": 0.1, "offsets": [ { "slot": "arm", "offset": -3 } ] } ]
  },
  "wave": {
    "bones": {
      "arm": { "rotate": [ { "value": 0 }, { "time": 0.25, "value": 60 }, { "time": 0.5, "value": 0 } ] }
    },
    "events": [ { "time": 0.5, "name": "waved" } ]
  }
}
}"#;

/// Walks; a cue waves (crossfading in), and it walks again when done.
pub const MOVES: &str = r#"type = "animator"
initial = "walk"

[states.walk]
clip = "walk"

[[states.walk.transitions]]
to = "wave"
when = "Cue.wave"
blend = 0.25

[states.wave]
clip = "wave"
loop = false

[[states.wave.transitions]]
to = "walk"
when = "end"
blend = 0.1
"#;

const IMAGES: &[&str] = &[
    "thigh",
    "shin",
    "torso",
    "arm",
    "head",
    "eyes-open",
    "eyes-closed",
    "badge",
    "helmet",
];

/// A project with the puppet in `assets/puppet/`.
pub fn project(tag: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("skeletal-{tag}"));
    let _ = std::fs::remove_dir_all(&d);
    write_puppet(&d);
    d
}

fn write_puppet(d: &Path) {
    std::fs::create_dir_all(d.join("assets/puppet/images")).unwrap();
    std::fs::write(d.join("assets/puppet/puppet.json"), PUPPET).unwrap();
    std::fs::write(d.join("assets/puppet/moves.toml"), MOVES).unwrap();
    for (i, name) in IMAGES.iter().enumerate() {
        let px = [(40 * i) as u8, 200, 255 - (20 * i) as u8, 255];
        let rgba: Vec<u8> = px.iter().copied().cycle().take(4 * 4 * 4).collect();
        encode_png(
            &d.join(format!("assets/puppet/images/{name}.png")),
            4,
            4,
            &rgba,
        )
        .unwrap();
    }
}

struct Game {
    w: World,
    s: Scheduler,
}

fn game(tag: &str) -> Game {
    let root = project(tag);
    let mut imp = Importer::new(&root);
    imp.scan();
    assert!(imp.failures().is_empty(), "{:?}", imp.failures());
    let mut w = World::new();
    argus_ecs::hierarchy::register(&mut w).unwrap();
    argus_render::register(&mut w).unwrap();
    let cue = SchemaBuilder::new("Cue")
        .field("wave", FieldType::Bool)
        .build()
        .unwrap();
    w.register_schema(cue).unwrap();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0).seed(5));
    let store = Rc::new(RefCell::new(AnimStore::default()));
    store.borrow_mut().set_assets(&imp);
    argus_anim::install(&mut w, &mut s, store.clone()).unwrap();
    argus_skeletal::install(&mut w, &mut s, store).unwrap();
    s.play(&w);
    Game { w, s }
}

fn puppet(g: &mut Game, sk: impl FnOnce(&World) -> Skeleton) -> Entity {
    let sk = sk(&g.w);
    let e = g.w.spawn();
    g.w.insert(e, Transform::default()).unwrap();
    g.w.insert(e, GlobalTransform::default()).unwrap();
    g.w.insert(
        e,
        Skeleton {
            skeleton: g.w.strings().intern("puppet/puppet"),
            ..sk
        },
    )
    .unwrap();
    e
}

fn id(g: &Game, s: &str) -> StrId {
    g.w.strings().intern(s)
}

/// The skeleton's parts by slot.
fn parts(g: &Game, of: Entity) -> Vec<(u32, Entity)> {
    let mut v = Vec::new();
    g.w.query()
        .each::<&SkeletonPart>(|e, p| {
            if p.skeleton == of {
                v.push((p.slot, e));
            }
        })
        .unwrap();
    v.sort();
    v
}

fn part(g: &Game, of: Entity, slot: u32) -> Entity {
    parts(g, of)
        .into_iter()
        .find(|p| p.0 == slot)
        .map(|p| p.1)
        .expect("a part for the slot")
}

fn local(g: &Game, e: Entity) -> Transform {
    g.w.get::<Transform>(e).unwrap()
}

fn sprite(g: &Game, e: Entity, field: &str) -> Value {
    g.w.get_field(e, &format!("Sprite.{field}")).unwrap()
}

fn near(a: Vec2, b: Vec2, tol: f32) -> bool {
    (a.x - b.x).abs() <= tol && (a.y - b.y).abs() <= tol
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

fn field<'a>(f: &'a [(String, Value)], k: &str) -> &'a Value {
    &f.iter().find(|(n, _)| n == k).expect(k).1
}

const LEG: u32 = 0;
const SHIN: u32 = 1;
const TORSO: u32 = 2;
const ARM: u32 = 3;
const HEAD: u32 = 4;
const EYES: u32 = 5;
const BADGE: u32 = 6;

#[test]
fn a_spine_export_imports_as_a_skeleton_and_its_images_as_sprites() {
    let root = project("import");
    let mut imp = Importer::new(&root);
    imp.scan();
    let e = imp.get("puppet/puppet").expect("the skeleton imported");
    let Asset::Skeleton(s) = &e.asset else {
        panic!("{:?}", e.asset)
    };
    assert_eq!(s.spine, "4.1.24");
    assert_eq!(s.bones.len(), 9);
    assert_eq!(s.slots.len(), 7);
    assert_eq!(s.ik.len(), 1);
    assert!(!s.ik[0].bend_positive);
    assert_eq!(
        s.skins.keys().map(String::as_str).collect::<Vec<_>>(),
        ["armored", "default"]
    );
    assert_eq!(
        s.animations.keys().map(String::as_str).collect::<Vec<_>>(),
        ["blink", "walk", "wave"]
    );
    assert_eq!(s.animations["walk"].duration, 1.0);
    assert_eq!(s.animations["walk"].events.len(), 2);
    assert!(s.images().contains(&"puppet/images/helmet"));
    // What is not drawn is said, with its line.
    assert!(
        e.warnings
            .iter()
            .any(|w| w.message.contains("arm-mesh") && w.message.contains("mesh")),
        "{:?}",
        e.warnings
    );
    assert!(e.warnings.iter().all(|w| w.at.contains("puppet.json:")));
    // Every attachment's image is an ordinary sprite.
    for name in s.images() {
        assert!(
            matches!(imp.get(name).ok().map(|e| &e.asset), Some(Asset::Sprite(_))),
            "{name}"
        );
    }
    let j = e.to_json();
    assert_eq!(j["kind"], "skeleton");
    assert_eq!(j["animations"]["walk"]["events"], 2);
}

/// Read a skeleton, and pose whatever reads: every animation at a few
/// times, mixed, placed, mirrored.
fn read_and_pose(text: &str) {
    let Ok((sk, _)) = argus_assets::spine::parse("p.json", "assets/p.json", text) else {
        return;
    };
    for an in sk.animations.values() {
        for t in [-1.0, 0.0, 0.3, an.duration, an.duration + 5.0, f32::NAN] {
            let mut p = pose::setup(&sk);
            pose::apply(&sk, an, t, &mut p);
            let q = pose::setup(&sk);
            pose::mix(&mut p, &q, 0.5);
            for flip in [false, true] {
                let m = pose::world(&sk, &p, flip);
                for skin in ["default", "armored", "nothing"] {
                    let _ = pose::place_slots(&sk, &p, &m, skin);
                }
            }
            let _ = pose::events_in(an, t - 0.5, t).count();
        }
        let n = argus_anim::skeleton_frames(an.duration, sk.fps);
        assert!((1..=argus_anim::MAX_SKELETON_FRAMES).contains(&n));
    }
}

#[test]
fn a_damaged_skeleton_is_an_error_or_a_pose_never_a_panic() {
    let bytes = PUPPET.as_bytes();
    for cut in 0..bytes.len() {
        read_and_pose(std::str::from_utf8(&bytes[..cut]).unwrap());
    }
    for i in 0..bytes.len() {
        for c in *b"9-\"}" {
            let mut b = bytes.to_vec();
            b[i] = c;
            read_and_pose(std::str::from_utf8(&b).unwrap());
        }
    }
    // Numbers a hand edit makes absurd.
    for (from, to) in [
        ("\"time\": 0.5", "\"time\": 1e30"),
        ("\"fps\": 30", "\"fps\": 1e30"),
        ("\"length\": 50", "\"length\": 0"),
        ("\"y\": 100", "\"y\": 0"),
        ("\"offset\": -3", "\"offset\": -300"),
    ] {
        read_and_pose(&PUPPET.replace(from, to));
    }
}

#[test]
fn a_skeleton_from_another_spine_or_not_spine_at_all_is_refused_with_its_place() {
    let root = project("refused");
    std::fs::write(
        root.join("assets/old.json"),
        r#"{ "skeleton": { "spine": "3.8.99" }, "bones": [ { "name": "root" } ] }"#,
    )
    .unwrap();
    std::fs::write(
        root.join("assets/broken.json"),
        "{ \"skeleton\": { \"spine\": \"4.1\" },\n  \"bones\": [ { \"name\": \"a\", \"parent\": \"nobody\" } ] }",
    )
    .unwrap();
    let mut imp = Importer::new(&root);
    imp.scan();
    let old = &imp.failures()["old"].error;
    assert!(old.message.contains("Spine 3.8.99"), "{}", old.message);
    let broken = &imp.failures()["broken"].error;
    assert!(broken.message.contains("nobody"), "{}", broken.message);
    assert_eq!(broken.r#ref.as_deref(), Some("assets/broken.json:2"));
}

#[test]
fn the_setup_pose_draws_each_slot_as_a_sprite_under_the_skeleton() {
    let mut g = game("setup");
    let p = puppet(&mut g, |_| Skeleton::default());
    g.s.step(&mut g.w, 1).unwrap();
    let sk: Skeleton = g.w.get(p).unwrap();
    assert_eq!(sk.parts, 7);
    assert_eq!(parts(&g, p).len(), 7);
    for (slot, e) in parts(&g, p) {
        assert_eq!(g.w.get::<Parent>(e).unwrap().entity, p, "slot {slot}");
    }
    // Hip at 100, the torso turned up: the head bone 60 above it, its
    // image 10 further along.
    let head = part(&g, p, HEAD);
    assert!(near(local(&g, head).pos, Vec2::new(0.0, 170.0), 1e-3));
    assert!((local(&g, head).rot - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    assert_eq!(
        sprite(&g, head, "atlas"),
        Value::Str("puppet/images/head".into())
    );
    assert_eq!(sprite(&g, head, "size"), Value::Vec2(Vec2::new(30.0, 30.0)));
    // The torso's image is turned back upright on its bone.
    let torso = part(&g, p, TORSO);
    assert!(near(local(&g, torso).pos, Vec2::new(0.0, 130.0), 1e-3));
    assert!(local(&g, torso).rot.abs() < 1e-5);
    // The arm hangs off the torso pointing left.
    let arm = part(&g, p, ARM);
    assert!(near(local(&g, arm).pos, Vec2::new(-20.0, 150.0), 1e-3));
    // onlyTranslation: the badge sits on the torso but does not turn with it.
    let badge = part(&g, p, BADGE);
    assert!(near(local(&g, badge).pos, Vec2::new(0.0, 130.0), 1e-3));
    assert!(local(&g, badge).rot.abs() < 1e-5);
    assert_eq!(sprite(&g, badge, "color"), Value::Str("#00ff00ff".into()));
    // Draw order: back slot at z, each in front 0.001 higher.
    let z = |e| match sprite(&g, e, "z") {
        Value::Float(f) => f as f32,
        v => panic!("{v:?}"),
    };
    assert!(z(part(&g, p, LEG)) < z(torso) && z(torso) < z(arm) && z(arm) < z(head));
    assert!((z(head) - 0.004).abs() < 1e-6);
    // The world places them under the skeleton's entity.
    g.w.update::<Transform, _>(p, |t| t.pos = Vec2::new(5.0, 0.0))
        .unwrap();
    g.s.step(&mut g.w, 1).unwrap();
    let gt: GlobalTransform = g.w.get(head).unwrap();
    assert!(near(gt.pos, Vec2::new(5.0, 170.0), 1e-3), "{gt:?}");
}

#[test]
fn scale_and_a_mirror_place_the_parts_in_world_units() {
    let mut g = game("scale");
    let p = puppet(&mut g, |_| Skeleton {
        scale: 0.01,
        flip_x: true,
        ..Default::default()
    });
    g.s.step(&mut g.w, 1).unwrap();
    let arm = part(&g, p, ARM);
    // Mirrored, the arm points right.
    assert!(near(local(&g, arm).pos, Vec2::new(0.2, 1.5), 1e-5));
    let t = local(&g, part(&g, p, HEAD));
    assert!(near(t.pos, Vec2::new(0.0, 1.7), 1e-5));
    // The image keeps its authored size; the transform scales it, and a
    // mirror is a reflection.
    assert!((t.scale.x.abs() - 0.01).abs() < 1e-6 && (t.scale.y.abs() - 0.01).abs() < 1e-6);
    assert!(t.scale.x * t.scale.y < 0.0, "mirrored: {t:?}");
    // The leg reaches its target at any scale (Spine's thresholds are in
    // its own units): the shin's middle is between the knee and (30, 20),
    // mirrored, so left of the hip and below the knee.
    let shin = local(&g, part(&g, p, SHIN));
    assert!(shin.pos.x < -0.05 && shin.pos.y < 0.5, "{shin:?}");
}

#[test]
fn two_bone_ik_reaches_the_target_bending_the_way_it_is_told() {
    let root = project("ik");
    let text = std::fs::read_to_string(root.join("assets/puppet/puppet.json")).unwrap();
    let (sk, _) =
        argus_assets::spine::parse("puppet/puppet.json", "assets/puppet/puppet.json", &text)
            .unwrap();
    let ends = |bend: bool| {
        let mut p = pose::setup(&sk);
        p.ik[0].1 = bend;
        let m = pose::world(&sk, &p, false);
        let shin = &m[sk.bone("shin").unwrap()];
        let foot = Vec2::new(shin.tx + shin.a * 50.0, shin.ty + shin.c * 50.0);
        (Vec2::new(shin.tx, shin.ty), foot)
    };
    let (knee_neg, foot_neg) = ends(false);
    let (knee_pos, foot_pos) = ends(true);
    // The foot is on the target, (30, 20), either way.
    assert!(near(foot_neg, Vec2::new(30.0, 20.0), 1e-2), "{foot_neg:?}");
    assert!(near(foot_pos, Vec2::new(30.0, 20.0), 1e-2), "{foot_pos:?}");
    // The knee is 50 from the hip and the foot, on opposite sides.
    for k in [knee_neg, knee_pos] {
        assert!(((k.x).hypot(k.y - 100.0) - 50.0).abs() < 1e-2);
        assert!(((k.x - 30.0).hypot(k.y - 20.0) - 50.0).abs() < 1e-2);
    }
    let side = |k: Vec2| (30.0 - 0.0) * (k.y - 100.0) - (20.0 - 100.0) * (k.x - 0.0);
    assert!(side(knee_neg) * side(knee_pos) < 0.0);
    // Mix 0 leaves the leg straight down.
    let mut p = pose::setup(&sk);
    p.ik[0].0 = 0.0;
    let m = pose::world(&sk, &p, false);
    let shin = &m[sk.bone("shin").unwrap()];
    assert!(near(
        Vec2::new(shin.tx, shin.ty),
        Vec2::new(0.0, 50.0),
        1e-3
    ));
    // A target out of reach: the leg points at it, straight.
    let mut p = pose::setup(&sk);
    p.bones[sk.bone("target").unwrap()].x = 300.0;
    let m = pose::world(&sk, &p, false);
    let (thigh, shin) = (&m[sk.bone("thigh").unwrap()], &m[sk.bone("shin").unwrap()]);
    let dir = Vec2::new(300.0, 20.0 - 100.0);
    let len = dir.x.hypot(dir.y);
    assert!((thigh.a - dir.x / len).abs() < 1e-3 && (shin.a - dir.x / len).abs() < 1e-3);
}

#[test]
fn an_animation_plays_its_keys_over_the_setup_pose() {
    let mut g = game("play");
    let p = puppet(&mut g, |w| Skeleton {
        animation: w.strings().intern("walk"),
        ..Default::default()
    });
    g.s.step(&mut g.w, 1).unwrap();
    // Half a second in: the hip is up 10, the torso turned 10 more.
    g.s.step(&mut g.w, 30).unwrap();
    let sk: Skeleton = g.w.get(p).unwrap();
    assert!((sk.time - 0.5).abs() < 1e-4, "{}", sk.time);
    assert_eq!(sk.shown, id(&g, "walk"));
    let r = 100f32.to_radians();
    let want = Vec2::new(70.0 * r.cos(), 110.0 + 70.0 * r.sin());
    let head = part(&g, p, HEAD);
    assert!(
        near(local(&g, head).pos, want, 1e-2),
        "{:?}",
        local(&g, head)
    );
    // The foot follows its target (moved 10 right): IK on the animated
    // pose.
    let (sk_asset, _) = parse_puppet();
    let mut pz = pose::setup(&sk_asset);
    pose::apply(&sk_asset, &sk_asset.animations["walk"], 0.5, &mut pz);
    let m = pose::world(&sk_asset, &pz, false);
    let shin = &m[sk_asset.bone("shin").unwrap()];
    assert!(near(
        Vec2::new(shin.tx + shin.a * 50.0, shin.ty + shin.c * 50.0),
        Vec2::new(40.0, 20.0),
        1e-2
    ));
    // It loops, and a one-shot holds its end.
    g.s.step(&mut g.w, 45).unwrap();
    let sk: Skeleton = g.w.get(p).unwrap();
    assert!(
        (sk.time - 0.25).abs() < 1e-3 && sk.loops == 1 && sk.ended,
        "{sk:?}"
    );
    g.w.update::<Skeleton, _>(p, |s| {
        s.animation = id(&g, "wave");
        s.looping = false;
    })
    .unwrap();
    g.s.step(&mut g.w, 60).unwrap();
    let sk: Skeleton = g.w.get(p).unwrap();
    assert!(sk.ended && sk.time == 0.5, "{sk:?}");
}

fn parse_puppet() -> (argus_assets::SkeletonAsset, Vec<argus_assets::Warning>) {
    argus_assets::spine::parse("puppet/puppet.json", "assets/puppet/puppet.json", PUPPET).unwrap()
}

#[test]
fn spine_events_fire_as_the_animation_passes_them() {
    let mut g = game("events");
    let p = puppet(&mut g, |w| Skeleton {
        animation: w.strings().intern("walk"),
        ..Default::default()
    });
    let mut steps = Vec::new();
    for _ in 0..121 {
        g.s.step(&mut g.w, 1).unwrap();
        steps.extend(game_events(&g, "step"));
        g.w.drain_events();
    }
    // Two a loop, two loops; each with its own values and the event's
    // defaults where the key has none.
    assert_eq!(steps.len(), 4, "{steps:?}");
    let ints: Vec<&Value> = steps.iter().map(|f| field(f, "int")).collect();
    assert_eq!(
        ints,
        [
            &Value::Int(1),
            &Value::Int(2),
            &Value::Int(1),
            &Value::Int(2)
        ]
    );
    assert_eq!(field(&steps[0], "entity"), &Value::Entity(p));
    assert_eq!(field(&steps[0], "animation"), &Value::Str("walk".into()));
    // A one-shot's event at its end fires once.
    g.w.update::<Skeleton, _>(p, |s| {
        s.animation = id(&g, "wave");
        s.looping = false;
    })
    .unwrap();
    let mut waved = Vec::new();
    for _ in 0..90 {
        g.s.step(&mut g.w, 1).unwrap();
        waved.extend(game_events(&g, "waved"));
        g.w.drain_events();
    }
    assert_eq!(waved.len(), 1);
    assert_eq!(field(&waved[0], "string"), &Value::Str("hello".into()));
}

#[test]
fn an_animator_plays_the_skeleton_and_the_pose_follows_its_frame() {
    let mut g = game("animator");
    let p = puppet(&mut g, |_| Skeleton::default());
    g.w.insert(
        p,
        Animator {
            machine: id(&g, "puppet/moves"),
            ..Default::default()
        },
    )
    .unwrap();
    g.w.insert_by_name(p, "Cue").unwrap();
    g.s.step(&mut g.w, 1).unwrap();
    assert!(
        game_events(&g, "anim.error").is_empty(),
        "{:?}",
        game_events(&g, "anim.error")
    );
    for _ in 0..50 {
        g.s.step(&mut g.w, 1).unwrap();
        let a: Animator = g.w.get(p).unwrap();
        let sk: Skeleton = g.w.get(p).unwrap();
        assert_eq!(a.state, id(&g, "walk"));
        assert_eq!(sk.animation, id(&g, "walk"));
        // The animator's clip time is the skeleton's: 30 frames a second.
        let want = (a.frame as f32 + a.phase / a.frame_len) / 30.0;
        assert!((sk.time - want).abs() < 1e-5, "{} vs {want}", sk.time);
        assert!(
            (sk.time - a.time % 1.0).abs() < 1e-3,
            "{} vs {}",
            sk.time,
            a.time
        );
    }
    g.w.drain_events();
    let arm = part(&g, p, ARM);
    let before = local(&g, arm);
    let walk_time = g.w.get::<Skeleton>(p).unwrap().time;
    // The cue crossfades into the wave: the walk's pose held, fading.
    g.w.set_field(p, "Cue.wave", true).unwrap();
    g.s.step(&mut g.w, 1).unwrap();
    let sk: Skeleton = g.w.get(p).unwrap();
    assert_eq!(sk.animation, id(&g, "wave"));
    assert_eq!(sk.from, id(&g, "walk"));
    assert_eq!(sk.from_time, walk_time);
    assert!((sk.mix - 1.0).abs() < 1e-6);
    assert!(
        near(local(&g, arm).pos, before.pos, 1e-3),
        "all the old pose"
    );
    g.w.set_field(p, "Cue.wave", false).unwrap();
    g.s.step(&mut g.w, 6).unwrap();
    let sk: Skeleton = g.w.get(p).unwrap();
    assert!(sk.mix > 0.0 && sk.mix < 1.0, "{}", sk.mix);
    g.s.step(&mut g.w, 12).unwrap();
    let sk: Skeleton = g.w.get(p).unwrap();
    assert_eq!(sk.mix, 0.0);
    assert_eq!(sk.from, StrId::EMPTY);
    // When the wave ends the animator goes back to walking, and its
    // event fired on the way.
    let mut waved = 0;
    for _ in 0..40 {
        g.s.step(&mut g.w, 1).unwrap();
        waved += game_events(&g, "waved").len();
        g.w.drain_events();
    }
    assert_eq!(waved, 1);
    let a: Animator = g.w.get(p).unwrap();
    assert_eq!(a.state, id(&g, "walk"));
}

#[test]
fn skins_attachment_keys_colours_and_draw_order_change_the_parts() {
    let mut g = game("slots");
    let p = puppet(&mut g, |w| Skeleton {
        skin: w.strings().intern("armored"),
        animation: w.strings().intern("blink"),
        looping: false,
        ..Default::default()
    });
    g.s.step(&mut g.w, 1).unwrap();
    // The skin's own head; what it does not have comes from the default.
    let head = part(&g, p, HEAD);
    assert_eq!(
        sprite(&g, head, "atlas"),
        Value::Str("puppet/images/helmet".into())
    );
    assert_eq!(sprite(&g, head, "size"), Value::Vec2(Vec2::new(34.0, 34.0)));
    let eyes = part(&g, p, EYES);
    assert_eq!(
        sprite(&g, eyes, "atlas"),
        Value::Str("puppet/images/eyes-closed".into())
    );
    let z = |g: &Game, e| match sprite(g, e, "z") {
        Value::Float(f) => f as f32,
        v => panic!("{v:?}"),
    };
    let arm = part(&g, p, ARM);
    assert!(z(&g, arm) > z(&g, part(&g, p, LEG)));
    g.s.step(&mut g.w, 12).unwrap();
    // At 0.2 the eyes open; from 0.1 the arm is drawn behind everything.
    assert_eq!(
        sprite(&g, eyes, "atlas"),
        Value::Str("puppet/images/eyes-open".into())
    );
    assert!(
        z(&g, arm) < z(&g, part(&g, p, LEG)),
        "the arm went to the back"
    );
    g.s.step(&mut g.w, 12).unwrap();
    // The head has faded to its last key's colour.
    assert_eq!(sprite(&g, head, "color"), Value::Str("#ff000080".into()));
    // A slot with nothing shows nothing.
    let mut sk_asset = parse_puppet().0;
    sk_asset.slots[SHIN as usize].attachment = None;
    let placed = pose::place_slots(
        &sk_asset,
        &pose::setup(&sk_asset),
        &pose::world(&sk_asset, &pose::setup(&sk_asset), false),
        "default",
    );
    assert_eq!(placed[SHIN as usize].image, None);
}

/// One puppet walking on its own, one an animator plays, cued to wave at
/// tick 40, and one that starts blinking at tick 50 (its colours, strings
/// the world has not seen before then).
fn crowd(tag: &str) -> (Game, [Entity; 2]) {
    let mut g = game(tag);
    puppet(&mut g, |w| Skeleton {
        animation: w.strings().intern("walk"),
        speed: 1.3,
        ..Default::default()
    });
    let b = puppet(&mut g, |_| Skeleton::default());
    g.w.insert(
        b,
        Animator {
            machine: id(&g, "puppet/moves"),
            ..Default::default()
        },
    )
    .unwrap();
    g.w.insert_by_name(b, "Cue").unwrap();
    let c = puppet(&mut g, |_| Skeleton {
        looping: false,
        ..Default::default()
    });
    (g, [b, c])
}

fn tick(g: &mut Game, [b, c]: [Entity; 2], t: usize) {
    if t == 40 || t == 41 {
        g.w.set_field(b, "Cue.wave", t == 40).unwrap();
    }
    if t == 50 {
        let blink = g.w.strings().intern("blink");
        g.w.update::<Skeleton, _>(c, |s| s.animation = blink)
            .unwrap();
    }
    g.s.step(&mut g.w, 1).unwrap();
}

#[test]
fn the_same_run_poses_the_same_and_a_seek_replays_it() {
    let run = |tag: &str| {
        let (mut g, b) = crowd(tag);
        (0..150)
            .map(|t| {
                tick(&mut g, b, t);
                g.w.digest()
            })
            .collect::<Vec<u64>>()
    };
    let one = run("same-a");
    assert_eq!(one, run("same-b"));
    // From a keyframe in the middle of the crossfade, before the blink,
    // the same ticks.
    let (mut g, b) = crowd("same-c");
    for t in 0..44 {
        tick(&mut g, b, t);
    }
    assert!(g.w.get::<Skeleton>(b[0]).unwrap().mix > 0.0);
    let snap = g.s.snapshot(&g.w);
    let later: Vec<u64> = (44..120)
        .map(|t| {
            tick(&mut g, b, t);
            g.w.digest()
        })
        .collect();
    assert_eq!(later[..], one[44..120]);
    g.s.restore(&mut g.w, &snap);
    for (i, want) in later.iter().enumerate() {
        tick(&mut g, b, 44 + i);
        assert_eq!(g.w.digest(), *want, "tick {} after the keyframe", i);
    }
}

#[test]
fn parts_come_and_go_with_their_skeleton() {
    let mut g = game("lifecycle");
    let p = puppet(&mut g, |_| Skeleton::default());
    let q = puppet(&mut g, |_| Skeleton::default());
    g.s.step(&mut g.w, 1).unwrap();
    assert_eq!(parts(&g, p).len(), 7);
    assert_eq!(parts(&g, q).len(), 7);
    // Hidden: the parts stay, drawn by nothing.
    g.w.update::<Skeleton, _>(p, |s| s.visible = false).unwrap();
    g.s.step(&mut g.w, 1).unwrap();
    assert_eq!(sprite(&g, part(&g, p, HEAD), "visible"), Value::Bool(false));
    // A part removed by hand comes back; a stray one goes.
    let head = part(&g, q, HEAD);
    g.w.despawn(head).unwrap();
    g.s.step(&mut g.w, 1).unwrap();
    assert_eq!(parts(&g, q).len(), 7);
    // The skeleton gone, its parts go with it.
    g.w.despawn(p).unwrap();
    g.s.step(&mut g.w, 1).unwrap();
    assert_eq!(parts(&g, p).len(), 0);
    assert_eq!(parts(&g, q).len(), 7);
    // A skeleton that is not one is said once, and draws nothing.
    g.w.update::<Skeleton, _>(q, |s| s.skeleton = g.w.strings().intern("nobody"))
        .unwrap();
    g.s.step(&mut g.w, 1).unwrap();
    let errs = game_events(&g, "skeletal.error");
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert_eq!(parts(&g, q).len(), 0);
    g.w.drain_events();
    g.s.step(&mut g.w, 3).unwrap();
    assert!(game_events(&g, "skeletal.error").is_empty());
}

#[test]
fn a_skin_or_an_animation_the_skeleton_lacks_is_said_once() {
    let mut g = game("lacks");
    let p = puppet(&mut g, |w| Skeleton {
        skin: w.strings().intern("velvet"),
        animation: w.strings().intern("dance"),
        ..Default::default()
    });
    g.s.step(&mut g.w, 3).unwrap();
    let errs = game_events(&g, "skeletal.error");
    let messages: Vec<&Value> = errs.iter().map(|f| field(f, "message")).collect();
    assert_eq!(
        messages,
        [
            &Value::Str("puppet/puppet has no skin velvet".into()),
            &Value::Str("puppet/puppet has no animation dance".into())
        ]
    );
    // It still draws: the default skin, the setup pose.
    assert_eq!(parts(&g, p).len(), 7);
    assert_eq!(
        sprite(&g, part(&g, p, HEAD), "atlas"),
        Value::Str("puppet/images/head".into())
    );
}

#[test]
fn a_scene_without_skeletons_pays_nothing() {
    let mut g = game("nothing");
    for i in 0..200 {
        let e = g.w.spawn();
        g.w.insert(e, Transform::at(i as f32, 0.0)).unwrap();
    }
    let before = g.w.entity_count();
    g.s.step(&mut g.w, 10).unwrap();
    assert_eq!(g.w.entity_count(), before);
    let p = g.s.prof_systems(10).unwrap();
    let row = p.iter().find(|r| r.name == argus_skeletal::SYSTEM).unwrap();
    assert!(row.mean_ms < 0.02, "{row:?}");
}

/// A crowd of puppets walking (release; ignored in debug runs).
#[test]
#[ignore]
fn perf_a_hundred_skeletons_walking() {
    let mut g = game("perf");
    for i in 0..100 {
        let e = puppet(&mut g, |w| Skeleton {
            animation: w.strings().intern("walk"),
            speed: 0.5 + (i % 7) as f32 * 0.1,
            ..Default::default()
        });
        g.w.update::<Transform, _>(e, |t| t.pos = Vec2::new(i as f32 * 2.0, 0.0))
            .unwrap();
    }
    g.s.step(&mut g.w, 30).unwrap();
    let mut ms = Vec::new();
    for _ in 0..300 {
        let t = std::time::Instant::now();
        g.s.step(&mut g.w, 1).unwrap();
        ms.push(t.elapsed().as_secs_f64() * 1000.0);
        g.w.drain_events();
    }
    ms.sort_by(f64::total_cmp);
    let mean = ms.iter().sum::<f64>() / ms.len() as f64;
    let p95 = ms[ms.len() * 95 / 100];
    println!("100 skeletons x 7 slots: {mean:.3} ms a tick mean, {p95:.3} p95");
    assert!(mean < 1.5, "{mean} ms");
}
