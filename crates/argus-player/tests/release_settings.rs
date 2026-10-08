//! Phase 5 item 2: a player's settings, and a replay that keeps them. A
//! key rebound in a controls menu is in the run: a report replays to the
//! same world whether it starts before the rebind (the journal binds it
//! again) or after it (the keyframe holds the bindings).

use argus_input::DeviceInput;
use argus_player::Player;
use serde_json::{json, Value as Json};
use std::path::Path;

fn hollow() -> Player {
    Player::open(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hollow"),
        None,
        None,
    )
    .unwrap()
}

fn press(p: &mut Player, action: &str, value: Json) {
    p.inject(action, Some(&value), Some(1), false).unwrap();
    p.step(2).unwrap();
}

fn key(p: &mut Player, name: &str, pressed: bool) {
    p.devices(&[DeviceInput::Key {
        name: name.into(),
        pressed,
    }]);
}

fn jump_keys(p: &Player) -> Vec<String> {
    p.sources
        .input
        .as_ref()
        .unwrap()
        .borrow()
        .map()
        .bindings("jump", "keyboard")
}

/// Pause, Controls, Jump, and the X key, as a player does it.
fn rebind_jump_to_x(p: &mut Player) {
    press(p, "pause", json!(true));
    for _ in 0..2 {
        press(p, "ui_move", json!([0, -1]));
    }
    press(p, "ui_confirm", json!(true));
    press(p, "ui_confirm", json!(true));
    key(p, "KeyX", true);
    key(p, "KeyX", false);
    p.step(2).unwrap();
    assert_eq!(jump_keys(p), ["x"]);
    press(p, "ui_cancel", json!(true));
    press(p, "pause", json!(true));
}

/// Running right and jumping on X for `seconds`.
fn play(p: &mut Player, seconds: u64) {
    for _ in 0..seconds {
        p.inject("move", Some(&json!([1, 0])), Some(60), false)
            .unwrap();
        key(p, "KeyX", true);
        p.step(10).unwrap();
        key(p, "KeyX", false);
        p.step(50).unwrap();
    }
}

#[test]
fn a_key_rebound_in_the_controls_menu_is_in_the_report() {
    // The report starts before the rebind: its journal binds X again.
    let mut p = hollow();
    p.record();
    p.step(30).unwrap();
    rebind_jump_to_x(&mut p);
    play(&mut p, 5);
    let out = hollow()
        .play_report(&p.report("rebound", "report", "test").unwrap())
        .unwrap();
    assert_eq!(out["from"], 0, "{out}");
    assert_eq!(out["same"], true, "{out}");

    // It starts after: its keyframe holds the bindings.
    play(&mut p, 70);
    let bytes = p.report("later", "report", "test").unwrap();
    let mut fresh = hollow();
    assert_eq!(jump_keys(&fresh), ["space", "z"]);
    let out = fresh.play_report(&bytes).unwrap();
    assert!(out["from"].as_u64().unwrap() > 0, "{out}");
    assert_eq!(out["same"], true, "{out}");
    assert_eq!(jump_keys(&fresh), ["x"], "the keyframe's bindings");
}

#[test]
fn settings_last_between_launches_and_a_report_keeps_them() {
    use argus_player::settings::Store;
    // A player sets the text larger, high contrast on, and Jump on X.
    let mut p = hollow();
    let mut store = Store::open(&mut p, None, None);
    p.step(30).unwrap();
    rebind_jump_to_x(&mut p);
    press(&mut p, "pause", json!(true));
    for _ in 0..3 {
        press(&mut p, "ui_move", json!([0, -1]));
    }
    press(&mut p, "ui_confirm", json!(true)); // Settings
    press(&mut p, "ui_confirm", json!(true)); // Text size: 125%
    press(&mut p, "ui_move", json!([0, -1]));
    press(&mut p, "ui_confirm", json!(true)); // High contrast: on
    let text = store.changed(&p).expect("the settings changed");
    assert!(store.changed(&p).is_none(), "once");
    assert!(text.contains("text_scale = 1.25"), "{text}");
    assert!(text.contains("high_contrast = true"), "{text}");
    assert!(text.contains("[bindings.jump]"), "{text}");
    assert!(
        !text.contains("[bindings.pause]"),
        "only what changed: {text}"
    );

    // The next launch starts with them.
    let mut next = hollow();
    Store::open(&mut next, None, Some(&text));
    next.record();
    let ui = argus_ui::settings(&next.world).unwrap();
    assert_eq!((ui.text_scale, ui.high_contrast), (1.25, true));
    assert_eq!(jump_keys(&next), ["x"]);

    // A report of it replays in a player without them: the keyframe holds
    // the player's settings, not the viewer's.
    play(&mut next, 5);
    let out = hollow()
        .play_report(&next.report("larger text", "report", "test").unwrap())
        .unwrap();
    assert_eq!(out["same"], true, "{out}");
}

/// A turn-based game ticks only on a command, and the key a controls page
/// catches is one: the page shows it at once, without a second key to move
/// the game on, and no turn of play passes while the menu is open.
#[test]
fn a_caught_key_shows_at_once_in_a_turn_based_game() {
    let mut p = Player::open(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/delve"),
        None,
        None,
    )
    .unwrap();
    p.step(1).unwrap();
    let field = |p: &Player, id: &str, path: &str| {
        let e = p.world.entity(id).unwrap();
        p.world.get_field(e, path).unwrap()
    };
    let turn = field(&p, "@turn", "Turn.n");
    press(&mut p, "pause", json!(true));
    for _ in 0..2 {
        press(&mut p, "ui_move", json!([0, -1]));
    }
    press(&mut p, "ui_confirm", json!(true));
    press(&mut p, "ui_confirm", json!(true));
    let words = |p: &Player| field(p, "@controls.wait", "UiText.shown");
    assert_eq!(words(&p), argus_ecs::Value::Str("Wait: press a key".into()));
    let turns = p.devices(&[DeviceInput::Key {
        name: "KeyW".into(),
        pressed: true,
    }]);
    assert_eq!(turns, 1, "the caught key took the tick");
    assert_eq!(words(&p), argus_ecs::Value::Str("Wait: w".into()));
    assert_eq!(field(&p, "@turn", "Turn.n"), turn, "no turn of play");
}
