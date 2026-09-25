//! Dialogue (phase 3 item 4, §12): a def table's node graph played through
//! the UI: lines, choices offered by conditions on the world, effects as
//! edits, branches, the world's language, and the box's buttons pressed
//! with a player's actions.

use argus_dialogue::{graph, Graph, Talk, TalkPart};
use argus_ecs::{Entity, Event, StrId, Value, World};
use argus_input::{ActionMap, InputState};
use argus_sim::{Scheduler, SchedulerConfig};
use argus_ui::{Shared, Strings, UiButton, UiSettings, UiText};
use std::cell::RefCell;
use std::rc::Rc;

const DIALOGUE: &str = r#"
# Who you meet first is how you are met.
["hermit.start"]
next = "dialogue:hermit.hello"

["hermit.start.again"]
from = "dialogue:hermit.start"
when = "speaker:Npc.met"
next = "dialogue:hermit.again"

["hermit.hello"]
speaker = "hermit.name"
line = "hermit.hello"
next = "dialogue:hermit.ask"
effects = "speaker:Npc.met = true; emit hermit.met"
sound = "sfx/voice"

["hermit.again"]
speaker = "hermit.name"
line = "hermit.again"
next = "dialogue:hermit.ask"

["hermit.ask"]
speaker = "hermit.name"
line = "hermit.ask"

["hermit.ask.gem"]
from = "dialogue:hermit.ask"
line = "hermit.give"
when = "Hero.gems >= 1 and not speaker:Npc.has_gem"
effects = "Hero.gems -= 1; speaker:Npc.has_gem = true"
next = "dialogue:hermit.thanks"

["hermit.ask.bye"]
from = "dialogue:hermit.ask"
line = "hermit.bye"

["hermit.thanks"]
speaker = "hermit.name"
line = "hermit.thanks"
"#;

const EN: &str = r#"
[hermit]
name = "The hermit"
hello = "Few come this far. You carry {@hero:Hero.gems} gem."
again = "Back again?"
ask = "What will you do?"
give = "Give the gem"
bye = "Goodbye"
thanks = "It will light the way."
"#;

const FR: &str = r#"
[hermit]
name = "L'ermite"
hello = "Peu viennent si loin."
ask = "Que feras-tu ?"
give = "Donner la gemme"
bye = "Au revoir"
"#;

const MAP: &str = r#"
[actions.ui_move]
type = "axis2"
[actions.ui_move.bindings]
keyboard = ["arrows"]
[actions.ui_confirm]
type = "button"
[actions.ui_confirm.bindings]
keyboard = ["enter"]
[slots.1]
devices = ["keyboard"]
"#;

#[derive(argus_ecs::Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
#[argus(doc = "The hero")]
struct Hero {
    #[argus(doc = "gems")]
    gems: u32,
}

#[derive(argus_ecs::Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
#[argus(doc = "Someone to talk to")]
struct Npc {
    #[argus(doc = "has met the hero")]
    met: bool,
    #[argus(doc = "was given the gem")]
    has_gem: bool,
}

struct Game {
    w: World,
    s: Scheduler,
    input: Rc<RefCell<InputState>>,
    hero: Entity,
    npc: Entity,
}

fn game(tag: &str) -> Game {
    let root = std::env::temp_dir().join(format!("argus-dialogue-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("defs")).unwrap();
    std::fs::write(root.join("defs/dialogue.toml"), DIALOGUE).unwrap();
    let (defs, errors) = argus_scene::Defs::load(&root, &[graph::schema()], &[]);
    assert!(errors.is_empty(), "{errors:?}");
    let g = Graph::from_defs(&defs);
    assert!(g.problems.is_empty(), "{:?}", g.problems);
    let _ = std::fs::remove_dir_all(&root);

    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::default());
    argus_ecs::hierarchy::register(&mut w).unwrap();
    argus_camera::register(&mut w).unwrap();
    w.register::<Hero>().unwrap();
    w.register::<Npc>().unwrap();
    let map = ActionMap::parse("input.toml", MAP).unwrap();
    let input = Rc::new(RefCell::new(InputState::new(map, false)));
    argus_input::install(&mut w, &mut s, input.clone()).unwrap();
    let shared = Rc::new(RefCell::new(Shared {
        strings: Strings::from_texts("en", &[("en", EN), ("fr", FR)]),
        ..Shared::default()
    }));
    argus_ui::install(&mut w, &mut s, shared.clone()).unwrap();
    argus_dialogue::install(&mut w, &mut s, Rc::new(RefCell::new(g)), shared).unwrap();
    let hero = w.spawn_authored("hero").unwrap();
    w.insert(hero, Hero { gems: 1 }).unwrap();
    w.insert(hero, argus_input::PlayerSlot { slot: 1 }).unwrap();
    let actions = w.component_id("Actions").unwrap();
    w.insert_default(hero, actions).unwrap();
    let npc = w.spawn_authored("hermit").unwrap();
    w.insert(npc, Npc::default()).unwrap();
    Game {
        w,
        s,
        input,
        hero,
        npc,
    }
}

impl Game {
    fn talk(&mut self) -> Entity {
        let t = self.w.spawn();
        let start = self.w.strings().intern("dialogue:hermit.start");
        self.w
            .insert(
                t,
                Talk {
                    start,
                    speaker: self.npc,
                    listener: self.hero,
                    ..Talk::default()
                },
            )
            .unwrap();
        t
    }
    fn step(&mut self) {
        self.s.step(&mut self.w, 1).unwrap();
    }
    fn get(&self, t: Entity) -> Talk {
        self.w.get::<Talk>(t).unwrap()
    }
    fn s(&self, id: StrId) -> String {
        self.w.strings().resolve(id).to_string()
    }
    fn events(&mut self) -> Vec<(String, Vec<(String, Value)>)> {
        self.w
            .drain_events()
            .into_iter()
            .filter_map(|r| match r.event {
                Event::Game { name, fields } => Some((name, fields)),
                _ => None,
            })
            .collect()
    }
    /// The words of the talk's box, top to bottom, and which are buttons.
    fn shown(&self, t: Entity) -> Vec<(String, bool)> {
        let part = self.w.component_id_of::<TalkPart>().unwrap();
        let mut rows: Vec<(u32, String, bool)> = Vec::new();
        for e in self.w.query().has(part).entities() {
            if self.w.get::<TalkPart>(e).unwrap().talk != t {
                continue;
            }
            if let Ok(text) = self.w.get::<UiText>(e) {
                let r = self.w.get::<argus_ui::UiRect>(e).unwrap();
                rows.push((
                    r.pos.y as u32,
                    self.s(text.shown),
                    self.w.get::<UiButton>(e).is_ok(),
                ));
            }
        }
        rows.sort();
        rows.into_iter().map(|(_, s, b)| (s, b)).collect()
    }
    fn confirm(&mut self) {
        let now = self.s.tick();
        self.input
            .borrow_mut()
            .inject(
                now,
                1,
                "ui_confirm",
                Some(&serde_json::json!(true)),
                Some(1),
                false,
            )
            .unwrap();
    }
}

fn names(ev: &[(String, Vec<(String, Value)>)]) -> Vec<String> {
    ev.iter().map(|(n, _)| n.clone()).collect()
}

#[test]
fn a_conversation_shows_lines_offers_choices_and_makes_its_edits() {
    let mut g = game("walk");
    let t = g.talk();
    g.step();
    // The start branches to hello (not met yet); hello's edits are made.
    let talk = g.get(t);
    assert_eq!(g.s(talk.node), "hermit.hello");
    assert_eq!(g.s(talk.who), "The hermit");
    assert_eq!(g.s(talk.line), "Few come this far. You carry 1 gem.");
    assert_eq!(talk.choices, 0);
    assert!(
        g.w.get::<Npc>(g.npc).unwrap().met,
        "an effect: an edit of the world"
    );
    let ev = g.events();
    let n = names(&ev);
    for want in [
        "dialogue.started",
        "hermit.met",
        "sound.play",
        "dialogue.line",
    ] {
        assert!(n.contains(&want.to_string()), "{want} in {n:?}");
    }
    // The box: the name, the line, and a button to go on.
    assert_eq!(
        g.shown(t),
        [
            ("The hermit".to_string(), false),
            ("Few come this far. You carry 1 gem.".to_string(), false),
            ("Continue".to_string(), true)
        ]
    );
    // Going on (a script sets advance): two choices, the gem's condition met.
    g.w.update::<Talk, _>(t, |x| x.advance = true).unwrap();
    g.step();
    let talk = g.get(t);
    assert_eq!(g.s(talk.node), "hermit.ask");
    assert_eq!(talk.choices, 2);
    assert_eq!(
        g.shown(t)
            .iter()
            .filter(|r| r.1)
            .map(|r| r.0.clone())
            .collect::<Vec<_>>(),
        ["Give the gem", "Goodbye"]
    );
    // Taking the first: its edits, then thanks, then the end.
    g.w.update::<Talk, _>(t, |x| x.pick = 0).unwrap();
    g.step();
    assert_eq!(g.w.get::<Hero>(g.hero).unwrap().gems, 0);
    assert!(g.w.get::<Npc>(g.npc).unwrap().has_gem);
    assert_eq!(g.s(g.get(t).node), "hermit.thanks");
    let ev = g.events();
    let chose = ev.iter().find(|e| e.0 == "dialogue.chose").unwrap();
    assert!(chose
        .1
        .contains(&("choice".to_string(), Value::Str("hermit.ask.gem".into()))));
    g.w.update::<Talk, _>(t, |x| x.advance = true).unwrap();
    g.step();
    assert!(!g.w.is_alive(t), "an ended conversation is gone");
    assert!(g.shown(t).is_empty(), "and its box");
    let ev = g.events();
    let ended = ev.iter().find(|e| e.0 == "dialogue.ended").unwrap();
    assert!(ended
        .1
        .contains(&("node".to_string(), Value::Str("hermit.thanks".into()))));
}

#[test]
fn a_second_meeting_branches_and_a_spent_choice_is_not_offered() {
    let mut g = game("again");
    g.w.update::<Npc, _>(g.npc, |n| {
        n.met = true;
        n.has_gem = true;
    })
    .unwrap();
    let t = g.talk();
    g.step();
    assert_eq!(
        g.s(g.get(t).node),
        "hermit.again",
        "the branch whose when holds"
    );
    g.w.update::<Talk, _>(t, |x| x.advance = true).unwrap();
    g.step();
    assert_eq!(g.get(t).choices, 1, "the gem is given: only goodbye");
    g.w.update::<Talk, _>(t, |x| x.pick = 0).unwrap();
    g.step();
    assert!(!g.w.is_alive(t), "goodbye has no next: the end");
}

#[test]
fn the_box_is_driven_by_a_players_actions_and_speaks_the_worlds_language() {
    let mut g = game("keys");
    let set = g.w.spawn();
    let fr = g.w.strings().intern("fr");
    g.w.insert(
        set,
        UiSettings {
            language: fr,
            ..UiSettings::default()
        },
    )
    .unwrap();
    let t = g.talk();
    g.step();
    assert_eq!(g.s(g.get(t).line), "Peu viennent si loin.");
    assert_eq!(g.s(g.get(t).who), "L'ermite");
    // Confirm presses the focused button (go on), then the first choice.
    g.confirm();
    g.step();
    g.step();
    assert_eq!(g.s(g.get(t).node), "hermit.ask");
    assert_eq!(
        g.shown(t),
        [
            ("L'ermite".to_string(), false),
            ("Que feras-tu ?".to_string(), false),
            ("Donner la gemme".to_string(), true),
            ("Au revoir".to_string(), true)
        ]
    );
    g.confirm();
    g.step();
    g.step();
    assert_eq!(
        g.w.get::<Hero>(g.hero).unwrap().gems,
        0,
        "the focused choice was the gem"
    );
    // Untranslated: thanks shows in the base language.
    assert_eq!(g.s(g.get(t).line), "It will light the way.");
}

#[test]
fn rows_that_do_not_read_are_said_and_a_missing_node_ends_the_talk() {
    let bad = r#"
["a"]
line = "x"
when = "Hero.gems >>"
next = "dialogue:nowhere"

["b"]
line = "y"
effects = "Hero.gems *= 2"
"#;
    let root = std::env::temp_dir().join(format!("argus-dialogue-bad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("defs")).unwrap();
    std::fs::write(root.join("defs/dialogue.toml"), bad).unwrap();
    let (defs, _) = argus_scene::Defs::load(&root, &[graph::schema()], &[]);
    let g = Graph::from_defs(&defs);
    let msgs: Vec<String> = g
        .problems
        .iter()
        .map(|p| format!("{}: {}", p.0, p.1))
        .collect();
    assert_eq!(msgs.len(), 3, "{msgs:?}");
    assert!(
        msgs.iter()
            .any(|m| m.contains("a: when") && m.contains("dialogue.toml:")),
        "{msgs:?}"
    );
    assert!(msgs.iter().any(|m| m.contains("b: effects")), "{msgs:?}");
    assert!(msgs.iter().any(|m| m.contains("nowhere")), "{msgs:?}");
    let _ = std::fs::remove_dir_all(&root);

    // Conditions and effects as the table writes them.
    let c = graph::parse_cond("Hero.gems >= 1 and not speaker:Npc.met or @hero:Hero.gems == 3")
        .unwrap();
    assert_eq!(c.0.len(), 2);
    assert_eq!(c.0[0].len(), 2);
    assert!(c.0[0][1].not);
    let e =
        graph::parse_effects("Hero.gems -= 2; speaker:Npc.met = true; emit door.opened").unwrap();
    assert_eq!(e.len(), 3);

    let mut g = game("missing");
    let t = g.w.spawn();
    let start = g.w.strings().intern("nobody.here");
    g.w.insert(
        t,
        Talk {
            start,
            speaker: g.npc,
            listener: g.hero,
            ..Talk::default()
        },
    )
    .unwrap();
    g.step();
    assert!(!g.w.is_alive(t));
    let n = names(&g.events());
    assert!(
        n.contains(&"dialogue.problem".to_string()) && n.contains(&"dialogue.ended".to_string()),
        "{n:?}"
    );
}
