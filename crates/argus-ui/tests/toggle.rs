//! Phase 5 item 2 (section 12: "input-hold and toggle alternatives are
//! action-map options"): a settings menu's button with an InputToggle
//! switches its action between holding and toggling, in the tick, so a
//! replay switches it too. Held, the action lets go when the key does;
//! toggled, a press holds it until the next.

use argus_ecs::{Value, Vec2, World};
use argus_input::{ActionMap, InputState, InputToggle};
use argus_sim::{Scheduler, SchedulerConfig};
use argus_ui::{Shared, UiButton, UiNode, UiRoot};
use serde_json::json;
use std::cell::RefCell;
use std::rc::Rc;

const MAP: &str = r#"
[actions.ui_move]
type = "axis2"
[actions.ui_move.bindings]
keyboard = ["arrows"]
[actions.ui_confirm]
type = "button"
[actions.ui_confirm.bindings]
keyboard = ["enter"]
[actions.ui_cancel]
type = "button"
[actions.ui_cancel.bindings]
keyboard = ["escape"]
[actions.fire]
type = "button"
[actions.fire.bindings]
keyboard = ["space"]
[slots.1]
devices = ["keyboard"]
"#;

#[test]
fn a_toggle_button_switches_an_action_between_holding_and_toggling() {
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::default());
    argus_ecs::hierarchy::register(&mut w).unwrap();
    argus_camera::register(&mut w).unwrap();
    let map = ActionMap::parse("input.toml", MAP).unwrap();
    let input = Rc::new(RefCell::new(InputState::new(map, false)));
    argus_input::install(&mut w, &mut s, input.clone()).unwrap();
    argus_ui::install(&mut w, &mut s, Rc::new(RefCell::new(Shared::default()))).unwrap();
    argus_input::rebind::install(&mut w, &mut s, input.clone()).unwrap();
    let player = w.spawn();
    w.insert(player, argus_input::PlayerSlot { slot: 1 })
        .unwrap();
    let actions = w.component_id("Actions").unwrap();
    w.insert_default(player, actions).unwrap();
    let menu = w.spawn();
    w.insert(menu, UiNode::default()).unwrap();
    w.insert(menu, UiRoot::default()).unwrap();
    let b = w.spawn();
    w.insert(
        b,
        UiNode {
            size: Vec2::new(200.0, 20.0),
            ..UiNode::default()
        },
    )
    .unwrap();
    w.insert(b, argus_ecs::Parent { entity: menu }).unwrap();
    w.insert(b, UiButton::default()).unwrap();
    let fire = w.strings().intern("fire");
    w.insert(
        b,
        InputToggle {
            action: fire,
            toggle: false,
        },
    )
    .unwrap();

    let press = |s: &mut Scheduler, w: &mut World, action: &str| {
        let now = s.tick();
        input
            .borrow_mut()
            .inject(now, 1, action, Some(&json!(true)), Some(1), false)
            .unwrap();
        s.step(w, 3).unwrap();
    };
    let held = |w: &World| w.get_field(player, "Actions.fire").unwrap() == Value::Bool(true);

    s.step(&mut w, 2).unwrap();
    // Holding: a press lets go with the key.
    press(&mut s, &mut w, "fire");
    assert!(!held(&w));

    // The button: fire toggles now.
    press(&mut s, &mut w, "ui_confirm");
    assert!(w.get::<InputToggle>(b).unwrap().toggle);
    assert!(input.borrow().map().action("fire").unwrap().toggle);
    let toggled = w.drain_events().into_iter().any(
        |r| matches!(&r.event, argus_ecs::Event::Game { name, .. } if name == "input.toggled"),
    );
    assert!(toggled, "input.toggled");
    press(&mut s, &mut w, "fire");
    assert!(held(&w), "toggled on, it stays");
    press(&mut s, &mut w, "fire");
    assert!(!held(&w), "and off");

    // Again: holding as before.
    press(&mut s, &mut w, "ui_confirm");
    assert!(!w.get::<InputToggle>(b).unwrap().toggle);
    press(&mut s, &mut w, "fire");
    assert!(!held(&w));
}
