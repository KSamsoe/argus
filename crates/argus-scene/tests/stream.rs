//! Rooms streamed around an entity (phase 3 item 6, §4): `rooms.stream`
//! loads the rooms within the radius of the room the followed entity is
//! in, and the always-loaded ones, and unloads the rest; a room remembers
//! what happened in it; a transition is an event and plays the scene's
//! timeline; and the scene says what it cannot stream.

mod common;

use argus_ecs::{Event, SchemaBuilder, Transform, Value, Vec2, World};
use argus_scene::{Scene, StreamState};
use argus_sim::{Scheduler, SchedulerConfig};
use common::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Five rooms in a row, ten units wide, and a persistent one with the hero.
fn deep(root: &std::path::Path, follow: &str, bounds_r4: bool) {
    project(root);
    let mut scene = format!(
        "seed = 1\n\n[rooms]\nstart = \"r0\"\nradius = 1\nfollow = \"{follow}\"\nalways = [\"persistent\"]\ntransition = \"timeline:rooms.fade\"\n\n[rooms.graph]\n"
    );
    for r in 0..4 {
        scene.push_str(&format!("\"r{r}\" = [\"r{}\"]\n", r + 1));
    }
    write(root, "scenes/deep/scene.toml", &scene);
    write(root, "scenes/deep/rooms/persistent/room.toml", "");
    write(
        root,
        "scenes/deep/rooms/persistent/hero.toml",
        "[Transform]\npos = [5.0, 5.0]\n",
    );
    for r in 0..5 {
        let b = if r == 4 && !bounds_r4 {
            String::new()
        } else {
            format!("bounds = [{}.0, 0.0, {}.0, 10.0]\n", r * 10, r * 10 + 10)
        };
        write(root, &format!("scenes/deep/rooms/r{r}/room.toml"), &b);
    }
    write(
        root,
        "scenes/deep/rooms/r0/lamp.toml",
        "[Transform]\npos = [3.0, 2.0]\n\n[Light]\nintensity = 2.0\non = true\n",
    );
    write(
        root,
        "scenes/deep/rooms/r1/crate.toml",
        "[Transform]\npos = [13.0, 2.0]\n\n[Health]\ncur = 3\nmax = 3\n",
    );
    write(
        root,
        "scenes/deep/rooms/r2/door.toml",
        "prefab = \"door/base\"\n\n[Transform]\npos = [25.0, 1.0]\n\n[Door]\nleads_to = \"@r3/door\"\n",
    );
    write(
        root,
        "scenes/deep/rooms/r3/door.toml",
        "prefab = \"door/base\"\n\n[Transform]\npos = [35.0, 1.0]\n\n[Door]\nleads_to = \"@r2/door\"\n",
    );
}

struct Rig {
    w: World,
    s: Scheduler,
    scene: Rc<RefCell<Scene>>,
    hero: argus_ecs::Entity,
}

fn rig(t: &Temp) -> Rig {
    let p = open(t.path());
    let mut w = world();
    // A stand-in for the timelines' component, which the hook spawns.
    let tl = SchemaBuilder::new("Timeline")
        .field("name", argus_ecs::FieldType::Str)
        .field("actor", argus_ecs::FieldType::Entity)
        .build()
        .unwrap();
    w.register_schema(tl).unwrap();
    let mut scene = Scene::open(&p, "deep", w.registry()).unwrap();
    assert!(scene.issues().is_empty(), "{:?}", scene.issues());
    scene.spawn_around(&mut w, None).unwrap();
    let scene = Rc::new(RefCell::new(scene));
    let mut s = Scheduler::new(SchedulerConfig::default());
    argus_scene::stream::install(&mut w, &mut s, scene.clone()).unwrap();
    let hero = w.entity("@hero").unwrap();
    Rig { w, s, scene, hero }
}

impl Rig {
    fn go(&mut self, x: f32) -> Vec<(String, Vec<(String, Value)>)> {
        self.w
            .update::<Transform, _>(self.hero, |t| t.pos = Vec2::new(x, 5.0))
            .unwrap();
        self.s.step(&mut self.w, 1).unwrap();
        self.w
            .drain_events()
            .into_iter()
            .filter_map(|r| match r.event {
                Event::Game { name, fields } => Some((name, fields)),
                _ => None,
            })
            .collect()
    }
    fn loaded(&self) -> Vec<String> {
        self.scene
            .borrow()
            .loaded_rooms()
            .into_iter()
            .map(str::to_string)
            .collect()
    }
}

#[test]
fn rooms_stream_around_the_followed_entity_and_remember() {
    let t = temp("stream");
    deep(t.path(), "@hero", true);
    let mut r = rig(&t);
    assert_eq!(r.loaded(), ["persistent", "r0", "r1"]);
    // Inside its room: nothing moves.
    assert!(r.go(8.0).iter().all(|e| e.0 != "room.transition"));

    // What happens in r0 and r1 while they are loaded.
    let lamp = r.w.entity("@r0/lamp").unwrap();
    r.w.update::<Light, _>(lamp, |l| {
        l.intensity = 7.0;
        l.on = false;
    })
    .unwrap();
    r.w.insert(lamp, Health { cur: 1, max: 1 }).unwrap();
    let crate_ = r.w.entity("@r1/crate").unwrap();
    r.w.despawn(crate_).unwrap();

    // The room it is in is data on it.
    let room = |r: &Rig| {
        let id = r.w.get::<argus_scene::stream::InRoom>(r.hero).unwrap().room;
        r.w.strings().resolve(id).to_string()
    };
    assert_eq!(room(&r), "r0");
    // Into r2: r0 goes, r3 comes; the hook's timeline starts.
    let ev = r.go(25.0);
    assert_eq!(room(&r), "r2");
    assert_eq!(r.loaded(), ["persistent", "r1", "r2", "r3"]);
    let tr = ev
        .iter()
        .find(|e| e.0 == "room.transition")
        .expect("a transition");
    assert!(tr.1.contains(&("to".to_string(), Value::Str("r2".into()))));
    assert!(ev.iter().any(|e| e.0 == "room.unloaded"));
    assert!(r.w.entity("@r0/lamp").is_err(), "r0 is not loaded");
    let tl = r.w.component_id("Timeline").unwrap();
    let timelines = r.w.query().has(tl).entities();
    assert_eq!(timelines.len(), 1);
    assert_eq!(
        r.w.get_field(timelines[0], "Timeline.name").unwrap(),
        Value::Str("timeline:rooms.fade".into())
    );
    assert_eq!(
        r.w.get_field(timelines[0], "Timeline.actor").unwrap(),
        Value::Entity(r.hero)
    );
    // The two doors point at each other across rooms, children and all.
    let d2 = r.w.entity("@r2/door").unwrap();
    let d3 = r.w.entity("@r3/door").unwrap();
    assert_eq!(r.w.get::<Door>(d2).unwrap().leads_to, d3);
    assert!(r.w.entity("@r2/door/light").is_ok());

    // To the far end: at most four rooms are ever loaded.
    r.go(45.0);
    assert_eq!(r.loaded(), ["persistent", "r3", "r4"]);
    {
        let sc = r.scene.borrow();
        let m = sc.memory_of("r0/lamp").expect("r0 remembers its lamp");
        assert!(m
            .set
            .contains(&("Light.intensity".to_string(), Value::Float(7.0))));
        assert!(m
            .set
            .contains(&("Light.on".to_string(), Value::Bool(false))));
        assert_eq!(m.added, ["Health"]);
        assert!(sc.memory_of("r1/crate").is_some_and(|m| m.gone));
        assert!(sc.memory_of("r3/door").is_none(), "r3 is loaded");
    }
    // A keyframe keeps the rooms and what they remember.
    let state = r.scene.borrow().stream_state();
    let mut w = argus_ecs::codec::Writer::new();
    state.encode(&mut w);
    let bytes = w.into_bytes();
    let back = StreamState::decode(&mut argus_ecs::codec::Reader::new(&bytes, "test")).unwrap();
    assert_eq!(back, state);

    // Back to the start: the lamp as it was left, the crate still gone.
    r.go(15.0);
    r.go(5.0);
    assert_eq!(r.loaded(), ["persistent", "r0", "r1"]);
    let lamp = r.w.entity("@r0/lamp").unwrap();
    let l = r.w.get::<Light>(lamp).unwrap();
    assert_eq!((l.intensity, l.on), (7.0, false));
    assert!(r.w.get::<Health>(lamp).is_ok());
    assert!(
        r.w.entity("@r1/crate").is_err(),
        "a despawned entity stays gone"
    );
    assert!(r.scene.borrow().memory_of("r0/lamp").is_none());
}

#[test]
fn what_cannot_be_streamed_is_an_issue() {
    let t = temp("stream-issues");
    // The followed entity in a room that streams out, and a room without
    // bounds it could never be found in.
    deep(t.path(), "@r0/lamp", false);
    let p = open(t.path());
    let w = world();
    let s = Scene::open(&p, "deep", w.registry()).unwrap();
    let msgs: Vec<String> = s.issues().iter().map(|e| e.message.clone()).collect();
    assert!(
        msgs.iter().any(|m| m.contains("would stream itself out")),
        "{msgs:?}"
    );
    assert!(
        msgs.iter()
            .any(|m| m.contains("without bounds") && m.contains("r4")),
        "{msgs:?}"
    );
}
