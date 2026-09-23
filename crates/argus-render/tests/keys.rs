//! The window sends a key under winit's name for it. These are the keys the
//! three example games bind, sent the way the window sends them, resolved
//! through each game's own input.toml. No window is opened: this covers the
//! step between a real key press and the tested input path.

use argus_input::{ActionMap, ActionValue, DeviceInput, Devices};
use argus_render::window::physical_key_name;
use std::path::Path;
use winit::keyboard::{KeyCode, PhysicalKey};

fn game(name: &str) -> ActionMap {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name);
    ActionMap::load(&root)
        .unwrap()
        .unwrap_or_else(|| panic!("{name} has no input.toml"))
}

fn press(d: &mut Devices, code: KeyCode, pressed: bool) {
    d.apply(&DeviceInput::Key {
        name: physical_key_name(&PhysicalKey::Code(code)),
        pressed,
    });
}

/// What slot 1 reads for `action` while only `code` is held.
fn held(map: &ActionMap, code: KeyCode, action: &str) -> Option<ActionValue> {
    let mut d = Devices::default();
    press(&mut d, code, true);
    let v: std::collections::BTreeMap<String, ActionValue> =
        d.resolve(map, 1).into_iter().collect();
    v.get(action).copied()
}

#[test]
fn the_window_names_keys_the_way_bindings_expect() {
    let name = |c| physical_key_name(&PhysicalKey::Code(c));
    assert_eq!(name(KeyCode::KeyW), "KeyW");
    assert_eq!(name(KeyCode::ArrowLeft), "ArrowLeft");
    assert_eq!(argus_input::key_name(&name(KeyCode::KeyW)), "w");
    assert_eq!(argus_input::key_name(&name(KeyCode::ArrowLeft)), "left");
    assert_eq!(argus_input::key_name(&name(KeyCode::Enter)), "enter");
    assert_eq!(argus_input::key_name(&name(KeyCode::NumpadEnter)), "enter");
}

#[test]
fn every_key_the_example_games_bind_reaches_its_action() {
    let axis = |x, y| Some(ActionValue::Axis2(x, y));
    let on = Some(ActionValue::Button(true));
    let cases = [
        ("swarm", KeyCode::KeyW, "move", axis(0.0, 1.0)),
        ("swarm", KeyCode::KeyA, "move", axis(-1.0, 0.0)),
        ("swarm", KeyCode::KeyS, "move", axis(0.0, -1.0)),
        ("swarm", KeyCode::KeyD, "move", axis(1.0, 0.0)),
        ("delve", KeyCode::ArrowUp, "move", axis(0.0, 1.0)),
        ("delve", KeyCode::ArrowLeft, "move", axis(-1.0, 0.0)),
        ("delve", KeyCode::ArrowDown, "move", axis(0.0, -1.0)),
        ("delve", KeyCode::ArrowRight, "move", axis(1.0, 0.0)),
        ("delve", KeyCode::Period, "wait", on),
        ("delve", KeyCode::Enter, "descend", on),
        ("hollow", KeyCode::KeyA, "move", axis(-1.0, 0.0)),
        ("hollow", KeyCode::ArrowRight, "move", axis(1.0, 0.0)),
        ("hollow", KeyCode::Space, "jump", on),
        ("hollow", KeyCode::KeyZ, "jump", on),
    ];
    for (name, code, action, want) in cases {
        let got = held(&game(name), code, action);
        assert_eq!(got, want, "{name}: {code:?} should drive {action}");
    }
}

#[test]
fn a_released_key_lets_go_and_losing_focus_lets_go_of_everything() {
    let map = game("hollow");
    let mut d = Devices::default();
    press(&mut d, KeyCode::KeyD, true);
    press(&mut d, KeyCode::Space, true);
    press(&mut d, KeyCode::Space, false);
    let v: std::collections::BTreeMap<String, ActionValue> =
        d.resolve(&map, 1).into_iter().collect();
    assert_eq!(v.get("move"), Some(&ActionValue::Axis2(1.0, 0.0)));
    assert_ne!(v.get("jump"), Some(&ActionValue::Button(true)));
    d.apply(&DeviceInput::ReleaseAll);
    let v: std::collections::BTreeMap<String, ActionValue> =
        d.resolve(&map, 1).into_iter().collect();
    assert_ne!(v.get("move"), Some(&ActionValue::Axis2(1.0, 0.0)));
}
