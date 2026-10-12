//! Timelines (phase 3 item 5, §13): cues on tracks against time, played by
//! a Timeline entity: tweens, animator states, camera cuts, conversations,
//! and edits at their times; cues that wait; a skip; rows that do not read.

use argus_anim::Animator;
use argus_camera::{Camera, Zones};
use argus_dialogue::{Graph, Talk};
use argus_ecs::{Entity, Event, Transform, Value, Vec2, World};
use argus_sim::{Scheduler, SchedulerConfig};
use argus_timeline::{Timeline, TimelineCut, Timelines};
use argus_ui::{Shared, Strings};
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

const TIMELINES: &str = r#"
# A short scene: the second camera draws for a second, the hero slides
# right, a bell, and a line that holds everything until it is read.
["intro"]
length = 3.0

["intro.cut"]
timeline = "timeline:intro"
at = 0.25
kind = "camera"
who = "@wide"
duration = 1.0

["intro.slide"]
timeline = "timeline:intro"
track = "hero"
at = 0.5
kind = "tween"
path = "Transform.pos"
to = [10.0, 0.0]
duration = 1.0
easing = "in_out_sine"

["intro.roar"]
timeline = "timeline:intro"
at = 0.5
kind = "anim"
who = "other"
state = "roar"

["intro.bell"]
timeline = "timeline:intro"
at = 1.0
kind = "event"
effects = "Hero.gems += 1; emit bell"
sound = "sfx/bell"

["intro.never"]
timeline = "timeline:intro"
at = 1.0
kind = "event"
when = "Hero.gems > 5"
effects = "emit never"

["talk"]

["talk.hi"]
timeline = "timeline:talk"
at = 0.5
kind = "line"
who = "other"
line = "dialogue:hi"
wait = true

["talk.gate"]
timeline = "timeline:talk"
at = 1.0
kind = "event"
when = "Hero.gems >= 2"
wait = true
effects = "emit opened"

["talk.after"]
timeline = "timeline:talk"
at = 1.5
kind = "event"
effects = "Hero.gems += 10"
"#;

const DIALOGUE: &str = r#"
["hi"]
speaker = "npc.name"
line = "npc.hi"
"#;

#[derive(argus_ecs::Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
#[argus(doc = "The hero")]
struct Hero {
    #[argus(doc = "gems")]
    gems: u32,
}

struct Game {
    w: World,
    s: Scheduler,
    hero: Entity,
    npc: Entity,
    main: Entity,
    wide: Entity,
}

fn tables(tag: &str, text: &str) -> (argus_scene::Defs, Vec<argus_ecs::ArgusError>) {
    let root = temp(&format!("timeline-{tag}"));
    std::fs::create_dir_all(root.join("defs")).unwrap();
    std::fs::write(root.join("defs/timeline.toml"), text).unwrap();
    std::fs::write(root.join("defs/dialogue.toml"), DIALOGUE).unwrap();
    argus_scene::Defs::load(
        &root,
        &[argus_timeline::schema(), argus_dialogue::schema()],
        &[],
    )
}

fn game(tag: &str) -> Game {
    let (defs, errors) = tables(tag, TIMELINES);
    assert!(errors.is_empty(), "{errors:?}");
    let lines = Timelines::from_defs(&defs);
    assert!(lines.problems.is_empty(), "{:?}", lines.problems);
    let graph = Graph::from_defs(&defs);

    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::default());
    argus_ecs::hierarchy::register(&mut w).unwrap();
    w.register::<Hero>().unwrap();
    argus_camera::install(&mut w, &mut s, Rc::new(RefCell::new(Zones::default()))).unwrap();
    argus_anim::register(&mut w).unwrap();
    argus_tween::install(&mut w, &mut s).unwrap();
    let shared = Rc::new(RefCell::new(Shared {
        strings: Strings::from_texts(
            "en",
            &[("en", "[npc]\nname = \"The npc\"\nhi = \"Hello.\"\n")],
        ),
        ..Shared::default()
    }));
    argus_ui::install(&mut w, &mut s, shared.clone()).unwrap();
    argus_dialogue::install(&mut w, &mut s, Rc::new(RefCell::new(graph)), shared).unwrap();
    let mut lines = lines;
    lines.check(w.registry());
    assert!(lines.problems.is_empty(), "{:?}", lines.problems);
    argus_timeline::install(&mut w, &mut s, Rc::new(RefCell::new(lines))).unwrap();

    let hero = w.spawn_authored("hero").unwrap();
    w.insert(hero, Hero { gems: 0 }).unwrap();
    w.insert(hero, Transform::default()).unwrap();
    let npc = w.spawn_authored("npc").unwrap();
    w.insert(npc, Animator::default()).unwrap();
    let main = w.spawn_authored("main").unwrap();
    w.insert(main, Transform::default()).unwrap();
    w.insert(
        main,
        Camera {
            blend: 0.5,
            ..Camera::default()
        },
    )
    .unwrap();
    let wide = w.spawn_authored("wide").unwrap();
    w.insert(
        wide,
        Transform {
            pos: Vec2::new(20.0, 0.0),
            ..Transform::default()
        },
    )
    .unwrap();
    w.insert(
        wide,
        Camera {
            active: false,
            priority: 5,
            height: 60.0,
            ..Camera::default()
        },
    )
    .unwrap();
    Game {
        w,
        s,
        hero,
        npc,
        main,
        wide,
    }
}

impl Game {
    fn play(&mut self, name: &str) -> Entity {
        let t = self.w.spawn();
        let name = self.w.strings().intern(name);
        self.w
            .insert(
                t,
                Timeline {
                    name,
                    actor: self.hero,
                    other: self.npc,
                    ..Timeline::default()
                },
            )
            .unwrap();
        t
    }
    fn step(&mut self, n: u64) {
        self.s.step(&mut self.w, n).unwrap();
    }
    fn time(&self, t: Entity) -> f32 {
        self.w.get::<Timeline>(t).unwrap().time
    }
    fn names(&mut self) -> Vec<(String, Vec<(String, Value)>)> {
        self.w
            .drain_events()
            .into_iter()
            .filter_map(|r| match r.event {
                Event::Game { name, fields } => Some((name, fields)),
                _ => None,
            })
            .collect()
    }
}

fn count(ev: &[(String, Vec<(String, Value)>)], name: &str) -> usize {
    ev.iter().filter(|e| e.0 == name).count()
}

fn field<'a>(ev: &'a (String, Vec<(String, Value)>), k: &str) -> Option<&'a Value> {
    ev.1.iter().find(|f| f.0 == k).map(|f| &f.1)
}

#[test]
fn a_timeline_fires_its_cues_at_their_times_and_ends() {
    let mut g = game("fires");
    g.step(1);
    assert_eq!(argus_camera::current(&g.w), Some(g.main));
    let t = g.play("timeline:intro");
    g.step(1);
    let ev = g.names();
    assert_eq!(count(&ev, "timeline.started"), 1);
    assert_eq!(g.w.get::<Timeline>(t).unwrap().length, 3.0);

    // 0.25 s: the wide camera draws; the main one blends back later.
    g.step(15);
    let ev = g.names();
    assert_eq!(count(&ev, "timeline.cue"), 1, "{ev:?}");
    assert_eq!(argus_camera::current(&g.w), Some(g.wide));
    assert_eq!(count(&ev, "camera.cut"), 1);
    assert_eq!(
        g.w.query()
            .has(g.w.component_id_of::<TimelineCut>().unwrap())
            .count(),
        1
    );

    // 0.5 s: the slide starts, and the npc is sent to "roar".
    g.step(15);
    let ev = g.names();
    let cues: Vec<String> = ev
        .iter()
        .filter(|e| e.0 == "timeline.cue")
        .filter_map(|e| match field(e, "cue") {
            Some(Value::Str(s)) => Some(s.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(cues, vec!["intro.slide", "intro.roar"]);
    let roar = g.w.strings().intern("roar");
    assert_eq!(g.w.get::<Animator>(g.npc).unwrap().play, roar);
    // Half way through the slide it is near the middle (in and out).
    g.step(30);
    let x = g.w.get::<Transform>(g.hero).unwrap().pos.x;
    assert!((x - 5.0).abs() < 0.6, "half way: {x}");
    let ev = g.names();
    // 1.0 s: the bell rings, its edit made; the cue whose when fails passes.
    assert_eq!(count(&ev, "bell"), 1, "{ev:?}");
    assert_eq!(count(&ev, "never"), 0);
    assert_eq!(g.w.get::<Hero>(g.hero).unwrap().gems, 1);
    let passed = ev
        .iter()
        .find(|e| {
            e.0 == "timeline.cue" && field(e, "cue") == Some(&Value::Str("intro.never".into()))
        })
        .unwrap();
    assert_eq!(field(passed, "passed"), Some(&Value::Bool(true)));
    assert!(ev
        .iter()
        .any(|e| e.0 == "sound.play" && field(e, "sound") == Some(&Value::Str("sfx/bell".into()))));

    // 1.25 s: the cut is over; the main camera draws again, blending.
    g.step(16);
    assert_eq!(argus_camera::current(&g.w), Some(g.main));
    assert!(!g.w.get::<Camera>(g.wide).unwrap().active);
    let ev = g.names();
    let cut = ev.iter().find(|e| e.0 == "camera.cut").expect("a cut back");
    assert_eq!(field(cut, "blend"), Some(&Value::Float(0.5)));
    let c = g.w.get::<Camera>(g.main).unwrap();
    assert!(c.blend_left > 0.0 && c.blend_left < 0.5, "{}", c.blend_left);
    // Mid-blend the view is between the two cameras' views.
    let mid = (c.view_min.x + c.view_max.x) * 0.5;
    assert!(mid > 0.5 && mid < 20.0, "blending: {mid}");

    // 1.5 s: the slide has arrived; 3 s: the timeline ends and goes.
    g.step(20);
    assert!((g.w.get::<Transform>(g.hero).unwrap().pos.x - 10.0).abs() < 1e-4);
    g.step(90);
    let ev = g.names();
    assert_eq!(count(&ev, "timeline.ended"), 1, "{ev:?}");
    assert!(!g.w.is_alive(t));
}

#[test]
fn a_cue_that_waits_holds_the_timeline() {
    let mut g = game("waits");
    let t = g.play("talk");
    g.step(31);
    // 0.5 s: the npc speaks to the hero, and the timeline holds.
    let talk_id = g.w.component_id_of::<Talk>().unwrap();
    let talks = g.w.query().has(talk_id).entities();
    assert_eq!(talks.len(), 1);
    let k = g.w.get::<Talk>(talks[0]).unwrap();
    assert_eq!((k.speaker, k.listener), (g.npc, g.hero));
    let tl = g.w.get::<Timeline>(t).unwrap();
    assert!(tl.held && tl.waiting == talks[0]);
    let held = g.time(t);
    g.step(60);
    assert_eq!(g.time(t), held, "held while the line is up");
    // The line read, the timeline moves on, then waits for two gems.
    g.w.update::<Talk, _>(talks[0], |k| k.advance = true)
        .unwrap();
    g.step(2);
    let ev = g.names();
    assert_eq!(count(&ev, "timeline.resumed"), 1, "{ev:?}");
    assert!(g.time(t) > held);
    g.step(40);
    let tl = g.w.get::<Timeline>(t).unwrap();
    assert!(tl.held && tl.waiting.is_null(), "waiting on its when");
    let ev = g.names();
    assert_eq!(count(&ev, "opened"), 0);
    g.w.update::<Hero, _>(g.hero, |h| h.gems = 2).unwrap();
    g.step(2);
    let ev = g.names();
    assert_eq!(count(&ev, "opened"), 1);
    g.step(40);
    assert_eq!(g.w.get::<Hero>(g.hero).unwrap().gems, 12);
    let ev = g.names();
    assert_eq!(count(&ev, "timeline.ended"), 1);
}

#[test]
fn a_skip_jumps_to_the_end() {
    let mut g = game("skip");
    let t = g.play("intro");
    g.step(40);
    // Mid-way: the wide camera draws and the slide has begun.
    assert_eq!(argus_camera::current(&g.w), Some(g.wide));
    g.w.update::<Timeline, _>(t, |x| x.skip = true).unwrap();
    g.step(1);
    let ev = g.names();
    let ended = ev.iter().find(|e| e.0 == "timeline.ended").unwrap();
    assert_eq!(field(ended, "skipped"), Some(&Value::Bool(true)));
    assert!(!g.w.is_alive(t));
    // The slide is at its end, the bell's edit made, the cut over.
    assert!((g.w.get::<Transform>(g.hero).unwrap().pos.x - 10.0).abs() < 1e-4);
    assert_eq!(g.w.get::<Hero>(g.hero).unwrap().gems, 1);
    assert_eq!(count(&ev, "bell"), 1);
    assert!(!g.w.get::<Camera>(g.wide).unwrap().active);
    g.step(1);
    assert_eq!(argus_camera::current(&g.w), Some(g.main));

    // A line skipped is not said; one that was up is stopped.
    let t = g.play("talk");
    g.step(35);
    let talk_id = g.w.component_id_of::<Talk>().unwrap();
    assert_eq!(g.w.query().has(talk_id).count(), 1);
    g.w.update::<Timeline, _>(t, |x| x.skip = true).unwrap();
    g.step(2);
    assert_eq!(g.w.query().has(talk_id).count(), 0);
    let part = g.w.component_id_of::<argus_dialogue::TalkPart>().unwrap();
    assert_eq!(g.w.query().has(part).count(), 0, "its box goes too");
    // The gate's edit waited for two gems; skipped, a when that fails is passed.
    assert_eq!(g.w.get::<Hero>(g.hero).unwrap().gems, 11);
    let ev = g.names();
    assert_eq!(count(&ev, "dialogue.ended"), 1);
    assert_eq!(count(&ev, "opened"), 0);
}

#[test]
fn rows_that_do_not_read_are_said_and_a_missing_timeline_ends() {
    let (defs, errors) = tables(
        "bad",
        r#"
["a"]
length = 1.0

["a.kind"]
timeline = "timeline:a"
kind = "dance"

["a.path"]
timeline = "timeline:a"
kind = "tween"
path = "Transform"

["a.who"]
timeline = "timeline:a"
kind = "anim"
who = "everyone"
state = "x"

["a.late"]
timeline = "timeline:a"
at = 2.0
kind = "event"
effects = "emit late"

["a.field"]
timeline = "timeline:a"
kind = "event"
effects = "Hero.nothing = 1"

["a.lost"]
kind = "event"
effects = "emit x"
"#,
    );
    assert!(errors.is_empty(), "{errors:?}");
    let mut lines = Timelines::from_defs(&defs);
    let mut w = World::new();
    argus_ecs::hierarchy::register(&mut w).unwrap();
    w.register::<Hero>().unwrap();
    lines.check(w.registry());
    let msgs: Vec<String> = lines
        .problems
        .iter()
        .map(|(at, m)| format!("{at}: {m}"))
        .collect();
    for want in [
        "a.kind: kind \"dance\"",
        "a.path: a tween's path",
        "a.who: who \"everyone\"",
        "a.late: at 2 is past the end",
        "a.field:",
        "a.lost: a cue (event) needs timeline",
    ] {
        assert!(msgs.iter().any(|m| m.contains(want)), "{want}: {msgs:?}");
    }
    assert!(
        msgs.iter().all(|m| m.contains("timeline.toml:")),
        "{msgs:?}"
    );

    let mut g = game("missing");
    let t = g.play("nothing.here");
    g.step(1);
    assert!(!g.w.is_alive(t));
    let ev = g.names();
    assert_eq!(count(&ev, "timeline.problem"), 1);
    assert_eq!(count(&ev, "timeline.ended"), 1);
}
