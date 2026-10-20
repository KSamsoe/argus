//! String tables (phase 3 item 4, §12 "Dialogue and localization"): a
//! language per file, falling back to the base key by key; a text keyed in
//! the UI shows in the world's language and changes when it does; a key is
//! written into its file with the file's layout kept.

use argus_ecs::{StrId, World};
use argus_input::{ActionMap, InputState};
use argus_sim::{Scheduler, SchedulerConfig};
use argus_ui::strings::{self, Strings};
use argus_ui::{Shared, UiNode, UiRoot, UiSettings, UiText};
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

const EN: &str = r#"
language.name = "English"

[menu]
resume = "Resume"
restart = "Restart the run"

[hud]
gems = "Gems: {@hero:Hero.gems}"
"#;

const FR: &str = r#"
language.name = "Français"
menu.resume = "Reprendre"
"#;

#[test]
fn a_language_falls_back_to_the_base_key_by_key() {
    let s = Strings::from_texts("en", &[("en", EN), ("fr", FR)]);
    assert_eq!(s.languages(), ["en", "fr"]);
    assert_eq!(s.get("fr", "menu.resume"), Some("Reprendre"));
    assert_eq!(
        s.get("fr", "menu.restart"),
        Some("Restart the run"),
        "the base's"
    );
    assert_eq!(
        s.get("", "menu.resume"),
        Some("Resume"),
        "no language: the base"
    );
    assert_eq!(
        s.text("fr", "menu.nope"),
        "[menu.nope]",
        "what a missing key looks like"
    );
    assert_eq!(
        s.get("fr", "dialogue.continue"),
        Some("Continue"),
        "the engine's own"
    );
    assert_eq!(s.missing("fr"), ["hud.gems", "menu.restart"]);
    assert!(s.missing("en").is_empty());
    assert!(s.problems().is_empty());
    // A value that is not words, and a file that does not read, are said.
    let bad = Strings::from_texts("en", &[("en", "x = 3\n"), ("de", "[broken")]);
    assert_eq!(bad.problems().len(), 2, "{:?}", bad.problems());
    assert!(bad.problems()[0].contains("x is not a string"));
}

#[test]
fn a_key_is_written_into_its_file_keeping_the_rest() {
    let root = temp("strings");
    std::fs::create_dir_all(root.join("strings")).unwrap();
    let file = root.join("strings/fr.toml");
    std::fs::write(
        &file,
        "# Le jeu en français.\n[menu]\nresume = \"Reprendre\" # le bouton\n",
    )
    .unwrap();
    let old = strings::set_in_file(&root, "fr", "menu.resume", Some("Continuer")).unwrap();
    assert_eq!(old.as_deref(), Some("Reprendre"));
    strings::set_in_file(&root, "fr", "menu.restart", Some("Recommencer")).unwrap();
    strings::set_in_file(&root, "fr", "hud.gems", Some("Gemmes : {n}")).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.starts_with("# Le jeu en français."), "{text}");
    assert!(text.contains("# le bouton"), "{text}");
    let s = Strings::load(&root, "en");
    assert_eq!(s.get("fr", "menu.resume"), Some("Continuer"));
    assert_eq!(s.get("fr", "menu.restart"), Some("Recommencer"));
    assert_eq!(s.get("fr", "hud.gems"), Some("Gemmes : {n}"));
    // Removed, and a new language's file made.
    assert_eq!(
        strings::set_in_file(&root, "fr", "menu.restart", None)
            .unwrap()
            .as_deref(),
        Some("Recommencer")
    );
    strings::set_in_file(&root, "de", "menu.resume", Some("Weiter")).unwrap();
    let s = Strings::load(&root, "en");
    assert_eq!(s.get("fr", "menu.restart"), None);
    assert_eq!(s.languages(), ["en", "de", "fr"]);
    // A key cannot go inside a string.
    assert!(strings::set_in_file(&root, "fr", "menu.resume.x", Some("?")).is_err());
}

const MAP: &str = r#"
[actions.ui_confirm]
type = "button"
[actions.ui_confirm.bindings]
keyboard = ["enter"]
[slots.1]
devices = ["keyboard"]
"#;

fn world(shared: Rc<RefCell<Shared>>) -> (World, Scheduler) {
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::default());
    argus_ecs::hierarchy::register(&mut w).unwrap();
    argus_camera::register(&mut w).unwrap();
    let map = ActionMap::parse("input.toml", MAP).unwrap();
    let input = Rc::new(RefCell::new(InputState::new(map, false)));
    argus_input::install(&mut w, &mut s, input).unwrap();
    argus_ui::install(&mut w, &mut s, shared).unwrap();
    (w, s)
}

#[test]
fn a_keyed_text_shows_in_the_worlds_language_and_changes_with_it() {
    let shared = Rc::new(RefCell::new(Shared {
        strings: Strings::from_texts("en", &[("en", EN), ("fr", FR)]),
        ..Shared::default()
    }));
    let (mut w, mut s) = world(shared);
    #[derive(argus_ecs::Component, Clone, Copy, Debug, Default, PartialEq)]
    #[repr(C)]
    #[argus(doc = "A hero")]
    struct Hero {
        #[argus(doc = "gems")]
        gems: u32,
    }
    w.register::<Hero>().unwrap();
    let hero = w.spawn_authored("hero").unwrap();
    w.insert(hero, Hero { gems: 3 }).unwrap();
    let root = w.spawn();
    w.insert(root, UiRoot::default()).unwrap();
    w.insert(root, UiNode::default()).unwrap();
    let label = |w: &mut World, key: &str| {
        let e = w.spawn();
        w.insert(e, UiNode::default()).unwrap();
        w.insert(e, argus_ecs::Parent { entity: root }).unwrap();
        let key = w.strings().intern(key);
        w.insert(
            e,
            UiText {
                key,
                ..UiText::default()
            },
        )
        .unwrap();
        e
    };
    let resume = label(&mut w, "menu.resume");
    let restart = label(&mut w, "menu.restart");
    let gems = label(&mut w, "hud.gems");
    let missing = label(&mut w, "menu.nope");
    let shown = |w: &World, e| {
        w.strings()
            .resolve(w.get::<UiText>(e).unwrap().shown)
            .to_string()
    };
    s.step(&mut w, 1).unwrap();
    assert_eq!(shown(&w, resume), "Resume");
    assert_eq!(
        shown(&w, gems),
        "Gems: 3",
        "a keyed string's holes are filled"
    );
    assert_eq!(shown(&w, missing), "[menu.nope]");
    // The player picks French: the next layout is in French, where it can be.
    let set = w.spawn();
    let fr = w.strings().intern("fr");
    w.insert(
        set,
        UiSettings {
            language: fr,
            ..UiSettings::default()
        },
    )
    .unwrap();
    s.step(&mut w, 1).unwrap();
    assert_eq!(shown(&w, resume), "Reprendre");
    assert_eq!(
        shown(&w, restart),
        "Restart the run",
        "untranslated: the base"
    );
    assert_eq!(strings::language(&w), "fr");
    w.update::<UiSettings, _>(set, |u| u.language = StrId::EMPTY)
        .unwrap();
    s.step(&mut w, 1).unwrap();
    assert_eq!(shown(&w, resume), "Resume");
}

#[test]
fn touch_controls_say_their_words_in_the_worlds_language() {
    // A control's label is a string key when the tables have it, else
    // touch.<name> is, else its words show as written.
    let tables = Strings::from_texts(
        "en",
        &[
            ("en", "[touch]\njump = \"Jump\"\n[hud]\npause = \"Menu\"\n"),
            ("fr", "[touch]\njump = \"Sauter\"\n[hud]\npause = \"Menu principal\"\n"),
        ],
    );
    let control = |name: &str, label: &str| argus_input::TouchShown {
        name: name.into(),
        kind: argus_input::TouchKind::Button,
        centre: (0.0, 0.0),
        radius: 10.0,
        knob: (0.0, 0.0),
        pressed: false,
        label: label.into(),
    };
    let mut w = World::new();
    argus_ui::register(&mut w).unwrap();
    let words = |w: &World| {
        let mut c = vec![
            control("jump", "Jump"),
            control("menu", "hud.pause"),
            control("talk", "Talk"),
        ];
        strings::touch_words(w, &tables, &mut c);
        c.into_iter().map(|c| c.label).collect::<Vec<_>>()
    };
    assert_eq!(words(&w), ["Jump", "Menu", "Talk"]);
    let set = w.spawn();
    let fr = w.strings().intern("fr");
    w.insert(
        set,
        UiSettings {
            language: fr,
            ..UiSettings::default()
        },
    )
    .unwrap();
    assert_eq!(words(&w), ["Sauter", "Menu principal", "Talk"]);
}
