//! Input glyphs and the platform's overlay (phase 5 item 9): §15's "input
//! glyphs" follow the device the player last used, which the log records
//! as an action, and the overlay is an input a game binds.

use argus_ecs::{FieldType, SchemaBuilder, StrId, World};
use argus_input::{ActionMap, ActionValue, DeviceInput, Devices, Family, InputGlyph, InputState};
use argus_sim::{Scheduler, SchedulerConfig};
use serde_json::json;
use std::cell::RefCell;
use std::rc::Rc;

const MAP: &str = r#"
[actions.jump]
type = "button"
[actions.jump.bindings]
keyboard = ["space"]
gamepad = ["south"]

[actions.pause]
type = "button"
[actions.pause.bindings]
keyboard = ["escape"]
gamepad = ["start"]
platform = ["overlay"]

[actions.shoot]
type = "button"
[actions.shoot.bindings]
mouse = ["left"]

[slots.1]
devices = ["keyboard", "mouse", "gamepad"]
"#;

fn map() -> ActionMap {
    ActionMap::parse("input.toml", MAP).unwrap()
}

fn key(name: &str, pressed: bool) -> DeviceInput {
    DeviceInput::Key {
        name: name.into(),
        pressed,
    }
}

fn pad(pad: u32, button: &str, pressed: bool) -> DeviceInput {
    DeviceInput::PadButton {
        pad,
        button: button.into(),
        pressed,
    }
}

fn device(d: &Devices, m: &ActionMap) -> f32 {
    match d
        .resolve(m, 1)
        .into_iter()
        .find(|(a, _)| a == "device")
        .map(|(_, v)| v)
    {
        Some(ActionValue::Axis(v)) => v,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_device_in_use_is_the_one_last_pressed() {
    let m = map();
    let mut d = Devices::default();
    assert_eq!(device(&d, &m), 0.0, "the keyboard until a pad is used");
    d.apply(&DeviceInput::PadConnected {
        pad: 0,
        name: "DualSense Wireless Controller".into(),
        connected: true,
    });
    // Connected is not used.
    assert_eq!(device(&d, &m), 0.0);
    d.apply(&pad(0, "south", true));
    assert_eq!(Family::from_index(device(&d, &m)), Family::PlayStation);
    // A stick pushed far is a pad in use; a key after it, the keyboard.
    d.apply(&key("Space", true));
    assert_eq!(device(&d, &m), 0.0);
    d.apply(&DeviceInput::PadAxis {
        pad: 0,
        axis: "left_x".into(),
        value: 0.9,
    });
    assert_eq!(Family::from_index(device(&d, &m)), Family::PlayStation);
    // The host can say what a pad is (Steam Input knows a Steam Deck).
    d.apply(&DeviceInput::PadFamily {
        pad: 0,
        family: Family::Switch,
    });
    assert_eq!(Family::from_index(device(&d, &m)), Family::Switch);
}

#[test]
fn the_overlay_is_an_input_a_game_binds() {
    let m = map();
    let mut d = Devices::default();
    let pause = |d: &Devices| {
        d.resolve(&m, 1)
            .into_iter()
            .find(|(a, _)| a == "pause")
            .map(|(_, v)| v)
    };
    assert_eq!(pause(&d), Some(ActionValue::Button(false)));
    d.apply(&DeviceInput::Platform {
        name: "overlay".into(),
        on: true,
    });
    assert_eq!(pause(&d), Some(ActionValue::Button(true)));
    d.apply(&DeviceInput::Platform {
        name: "overlay".into(),
        on: false,
    });
    assert_eq!(pause(&d), Some(ActionValue::Button(false)));
    // A name the platform does not have is refused in the map.
    let e = ActionMap::parse(
        "input.toml",
        "[actions.x.bindings]\nplatform = [\"store\"]\n",
    )
    .unwrap_err();
    assert!(e.message.contains("overlay"), "{}", e.message);
    // The device action is the engine's.
    let e = ActionMap::parse("input.toml", "[actions.device]\n").unwrap_err();
    assert!(e.message.contains("engine's"), "{}", e.message);
}

struct Rig {
    w: World,
    s: Scheduler,
    st: Rc<RefCell<InputState>>,
}

fn rig() -> Rig {
    let mut w = World::new();
    argus_ecs::hierarchy::register(&mut w).unwrap();
    // The UI's panel, by name, as the glyphs write it.
    let panel = SchemaBuilder::new("UiPanel")
        .field("image", FieldType::Str)
        .build()
        .unwrap();
    w.register_schema(panel).unwrap();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0).seed(1));
    let st = Rc::new(RefCell::new(InputState::new(map(), false)));
    argus_input::install(&mut w, &mut s, st.clone()).unwrap();
    argus_input::glyphs::install(&mut w, &mut s, st.clone()).unwrap();
    s.play(&w);
    Rig { w, s, st }
}

fn glyph(r: &Rig, e: argus_ecs::Entity) -> (String, String, String) {
    let g: InputGlyph = r.w.get(e).unwrap();
    let res = |id: StrId| r.w.strings().resolve(id);
    (res(g.label), res(g.image), res(g.shown))
}

#[test]
fn a_prompt_names_the_button_on_the_device_in_use() {
    let mut r = rig();
    let e = r.w.spawn();
    let jump = r.w.strings().intern("jump");
    r.w.insert(
        e,
        InputGlyph {
            action: jump,
            panel: true,
            ..Default::default()
        },
    )
    .unwrap();
    r.w.insert_by_name(e, "UiPanel").unwrap();
    r.s.step(&mut r.w, 1).unwrap();
    assert_eq!(glyph(&r, e), ("Space".into(), "".into(), "keyboard".into()));
    // A player on a PlayStation pad: its button, and the picture's name.
    let now = r.s.tick();
    r.st.borrow_mut()
        .inject(now, 1, "device", Some(&json!(2)), None, true)
        .unwrap();
    r.s.step(&mut r.w, 1).unwrap();
    assert_eq!(
        glyph(&r, e),
        (
            "Cross".into(),
            "glyph/playstation/south".into(),
            "playstation".into()
        )
    );
    assert_eq!(
        r.w.get_field(e, "UiPanel.image").unwrap(),
        argus_ecs::Value::Str("glyph/playstation/south".into())
    );
    // A family asked for is shown whatever the player uses.
    let switch = r.w.strings().intern("switch");
    r.w.update::<InputGlyph, _>(e, |g| g.family = switch)
        .unwrap();
    r.s.step(&mut r.w, 1).unwrap();
    assert_eq!(glyph(&r, e).0, "B");
    // A mouse action on the keyboard family is its click.
    let shoot = r.w.strings().intern("shoot");
    let keyboard = r.w.strings().intern("keyboard");
    r.w.update::<InputGlyph, _>(e, |g| {
        g.action = shoot;
        g.family = keyboard
    })
    .unwrap();
    r.s.step(&mut r.w, 1).unwrap();
    assert_eq!(glyph(&r, e).0, "Left click");
    // The device a player used is in the log, as any action: a replay
    // shows the same prompts.
    let log = r.st.borrow().log().to_vec();
    assert!(
        log.iter()
            .any(|l| l.action == "device" && l.value == ActionValue::Axis(2.0)),
        "{log:?}"
    );
}

/// A slot's own keys (`[slots.2.keys]`) are what its prompts name.
#[test]
fn a_slot_with_keys_of_its_own_is_prompted_with_them() {
    let m = ActionMap::parse(
        "input.toml",
        "[actions.attack]\ntype = \"button\"\n[actions.attack.bindings]\nkeyboard = [\"f\"]\ngamepad = [\"south\"]\n[slots.1]\ndevices = [\"keyboard\"]\n[slots.2]\ndevices = [\"keyboard\"]\n[slots.2.keys]\nattack = [\"period\"]\n",
    )
    .unwrap();
    let one = argus_input::glyphs::prompt_for(&m, "attack", Family::Keyboard, 1).unwrap();
    let two = argus_input::glyphs::prompt_for(&m, "attack", Family::Keyboard, 2).unwrap();
    assert_eq!(one.1, "F");
    assert_eq!(two.1, ".");
    // A pad is the action's own whatever the slot.
    let pad = argus_input::glyphs::prompt_for(&m, "attack", Family::Xbox, 2).unwrap();
    assert_eq!(
        pad,
        argus_input::glyphs::prompt(&m, "attack", Family::Xbox).unwrap()
    );
}
