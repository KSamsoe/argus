//! Phase 5 item 2: the player's screen reader reads the game. Hollow's
//! pause menu, as the semantic frame has it, is an accessibility tree: the
//! buttons by their words, the focus where the menu's focus is, and a press
//! the screen reader asks for becomes the pointer and a click, which opens
//! the page the button opens.

use accesskit::{Action, ActionRequest, NodeId, Role, TreeUpdate};
use argus_input::DeviceInput;
use argus_player::Player;
use serde_json::json;
use std::path::Path;

fn hollow() -> Player {
    Player::open(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hollow"),
        None,
        None,
    )
    .unwrap()
}

/// The frame at the UI canvas's own size, so its pixels are the canvas's.
fn seen(p: &Player) -> (TreeUpdate, Vec<argus_render::semantic::UiSeen>) {
    let canvas = {
        let sh = p.ui.borrow();
        [sh.theme.width, sh.theme.height]
    };
    let sf = argus_render::semantic::semantic(
        &p.world,
        &argus_render::FrameSpec {
            width: canvas[0] as u32,
            height: canvas[1] as u32,
            limit: 0,
            groups: 0,
            assets: Some(&p.assets),
            particles: None,
            view: None,
            ui_canvas: Some(canvas),
        },
    );
    (argus_render::access::tree(&sf, &p.name), sf.ui)
}

fn label(t: &TreeUpdate, id: NodeId) -> String {
    t.nodes
        .iter()
        .find(|(n, _)| *n == id)
        .and_then(|(_, n)| n.label())
        .unwrap_or_default()
        .to_string()
}

fn button(t: &TreeUpdate, words: &str) -> NodeId {
    t.nodes
        .iter()
        .find(|(_, n)| n.role() == Role::Button && n.label() == Some(words))
        .map(|(id, _)| *id)
        .unwrap_or_else(|| panic!("no button {words:?}"))
}

fn press(p: &mut Player, action: &str, value: serde_json::Value) {
    p.inject(action, Some(&value), Some(1), false).unwrap();
    p.step(2).unwrap();
}

#[test]
fn a_screen_reader_reads_hollows_menu_and_presses_its_buttons() {
    let mut p = hollow();
    p.step(2).unwrap();
    let (t, _) = seen(&p);
    assert_eq!(t.tree.as_ref().unwrap().app_name.as_deref(), Some("hollow"));
    assert!(
        !t.nodes.iter().any(|(_, n)| n.role() == Role::Button),
        "no menu, no buttons"
    );

    // The menu: its buttons, the first the focus.
    press(&mut p, "pause", json!(true));
    let (t, _) = seen(&p);
    for words in ["Resume", "Restart the run", "Controls", "Settings"] {
        button(&t, words);
    }
    assert_eq!(label(&t, t.focus), "Resume");
    // The focus follows the menu's.
    press(&mut p, "ui_move", json!([0, -1]));
    let (t, ui) = seen(&p);
    assert_eq!(label(&t, t.focus), "Restart the run");

    // "Press Controls": the pointer to it and a click, as the game does it.
    let ask = ActionRequest {
        action: Action::Click,
        target: button(&t, "Controls"),
        data: None,
    };
    let (u, pressed) = argus_render::access::asked(&ask, &ui).expect("a button");
    assert!(pressed);
    let b = u.bounds;
    p.devices(&[DeviceInput::Pointer {
        x: (b[0] + b[2]) as f32 / 2.0,
        y: (b[1] + b[3]) as f32 / 2.0,
    }]);
    press(&mut p, "ui_click", json!(true));
    let shown = |p: &Player, id: &str| {
        let e = p.world.entity(id).unwrap();
        p.world.get_field(e, "UiRect.shown").unwrap()
    };
    assert_eq!(shown(&p, "@controls"), argus_ecs::Value::Bool(true));
    let (t, _) = seen(&p);
    assert!(
        label(&t, t.focus).starts_with("Jump: "),
        "{}",
        label(&t, t.focus)
    );
}
