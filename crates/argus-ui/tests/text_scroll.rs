//! Issue 23: a text box the player types into, and a node that scrolls,
//! driven through the input state as a window drives them.

use argus_ecs::{Entity, Parent, StrId, Value, Vec2, World};
use argus_input::{ActionMap, DeviceInput, Devices, InputState};
use argus_sim::{Scheduler, SchedulerConfig};
use argus_ui::{Shared, UiButton, UiNode, UiRoot, UiScroll, UiText, UiTextBox};
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
[actions.ui_click]
type = "button"
[actions.ui_click.bindings]
mouse = ["left"]
[actions.ui_scroll]
type = "axis"
[actions.ui_scroll.bindings]
mouse = ["wheel"]
[actions.jump]
type = "button"
[actions.jump.bindings]
keyboard = ["space"]
[actions.pointer]
type = "pointer"
[actions.pointer.bindings]
mouse = ["position"]
[slots.1]
devices = ["keyboard", "mouse"]
"#;

struct Ui {
    w: World,
    s: Scheduler,
    input: Rc<RefCell<InputState>>,
    dev: Devices,
    player: Entity,
}

fn ui() -> Ui {
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::default());
    argus_ecs::hierarchy::register(&mut w).unwrap();
    argus_camera::register(&mut w).unwrap();
    let map = ActionMap::parse("input.toml", MAP).unwrap();
    let input = Rc::new(RefCell::new(InputState::new(map, false)));
    argus_input::install(&mut w, &mut s, input.clone()).unwrap();
    argus_ui::install(&mut w, &mut s, Rc::new(RefCell::new(Shared::default()))).unwrap();
    let player = w.spawn();
    w.insert_by_name(player, "PlayerSlot").unwrap();
    w.insert_by_name(player, "Actions").unwrap();
    Ui {
        w,
        s,
        input,
        dev: Devices::default(),
        player,
    }
}

fn id(w: &World, s: &str) -> StrId {
    w.strings().intern(s)
}

impl Ui {
    fn node(&mut self, parent: Option<Entity>, n: UiNode) -> Entity {
        let e = self.w.spawn();
        self.w.insert(e, n).unwrap();
        if let Some(p) = parent {
            self.w.insert(e, Parent { entity: p }).unwrap();
        }
        e
    }
    fn step(&mut self, n: u64) {
        self.s.step(&mut self.w, n).unwrap();
    }
    /// Device events, as a window's poll hands them over.
    fn devices(&mut self, evs: &[DeviceInput]) {
        for e in evs {
            self.dev.apply(e);
        }
        let now = self.s.tick();
        self.input.borrow_mut().apply_devices(now, &mut self.dev);
    }
    fn typed(&mut self, t: &str) {
        self.devices(&[DeviceInput::Text { text: t.into() }]);
        self.step(1);
    }
    fn key(&mut self, name: &str, pressed: bool) {
        self.devices(&[DeviceInput::Key {
            name: name.into(),
            pressed,
        }]);
    }
    fn inject(&mut self, action: &str, value: serde_json::Value, frames: u64) {
        let now = self.s.tick();
        self.input
            .borrow_mut()
            .inject(now, 1, action, Some(&value), Some(frames), false)
            .unwrap();
    }
    fn events(&mut self, name: &str) -> Vec<Vec<(String, Value)>> {
        self.w
            .drain_events()
            .into_iter()
            .filter_map(|r| match r.event {
                argus_ecs::Event::Game { name: n, fields } if n == name => Some(fields),
                _ => None,
            })
            .collect()
    }
    fn act(&self, field: &str) -> Value {
        self.w
            .get_field(self.player, &format!("Actions.{field}"))
            .unwrap()
    }
    fn tb(&self, e: Entity) -> (String, bool, String) {
        let t = self.w.get::<UiTextBox>(e).unwrap();
        let shown = self.w.get::<UiText>(e).unwrap();
        (
            self.w.strings().resolve(t.text).to_string(),
            t.typing,
            self.w.strings().resolve(shown.text).to_string(),
        )
    }
}

fn text_box(u: &mut Ui) -> (Entity, Entity) {
    let root = u.node(None, UiNode::default());
    u.w.insert(root, UiRoot::default()).unwrap();
    let field = u.node(
        Some(root),
        UiNode {
            size: Vec2::new(200.0, 20.0),
            ..UiNode::default()
        },
    );
    u.w.insert(field, UiButton::default()).unwrap();
    u.w.insert(field, UiText::default()).unwrap();
    let hint = id(&u.w, "Say something");
    let said = id(&u.w, "said");
    u.w.insert(
        field,
        UiTextBox {
            hint,
            event: said,
            ..UiTextBox::default()
        },
    )
    .unwrap();
    let other = u.node(
        Some(root),
        UiNode {
            size: Vec2::new(200.0, 20.0),
            ..UiNode::default()
        },
    );
    u.w.insert(other, UiButton::default()).unwrap();
    u.step(2);
    (field, other)
}

#[test]
fn a_text_box_types_sends_and_stops() {
    let mut u = ui();
    let (field, other) = text_box(&mut u);
    assert_eq!(
        u.tb(field),
        (String::new(), false, "Say something".into()),
        "the hint"
    );
    // Text before it types goes nowhere.
    u.typed("lost");
    assert_eq!(u.tb(field).0, "");

    // Pressed (it has the focus): it types, and the keyboard is its.
    u.inject("ui_confirm", json!(true), 1);
    u.step(1);
    assert!(u.tb(field).1);
    assert_eq!(u.act("typing"), Value::Bool(true));
    u.step(1);
    u.key("Space", true);
    u.typed("hi ");
    assert_eq!(
        u.act("jump"),
        Value::Bool(false),
        "space types, it does not jump"
    );
    assert_eq!(u.tb(field), ("hi ".into(), true, "hi |".into()));
    u.key("Space", false);
    u.typed("\u{8}x");
    assert_eq!(u.tb(field).0, "hix", "backspace takes one off");

    // Enter sends it, empties it, and stops; Enter still down presses
    // nothing until it is let go.
    u.events("said");
    u.key("Enter", true);
    u.typed("\r");
    let said = u.events("said");
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains(&("text".to_string(), Value::Str("hix".into()))));
    assert_eq!(u.tb(field), (String::new(), false, "Say something".into()));
    assert_eq!(u.act("typing"), Value::Bool(false));
    u.step(1);
    u.key("Enter", true);
    u.step(2);
    assert!(
        !u.tb(field).1,
        "the Enter that sent it did not start it again"
    );
    u.key("Enter", false);
    u.step(1);
    u.key("Enter", true);
    u.step(2);
    assert!(u.tb(field).1, "a fresh press does");
    u.key("Enter", false);

    // Escape stops without sending; so does a click elsewhere.
    u.step(1);
    u.typed("abc\u{1b}");
    assert_eq!(u.tb(field), ("abc".into(), false, "abc".into()));
    assert!(u.events("said").is_empty());
    u.inject("ui_confirm", json!(true), 1);
    u.step(2);
    assert!(u.tb(field).1);
    let at = u.w.get::<argus_ui::UiRect>(other).unwrap();
    u.inject("pointer", json!([at.pos.x + 5.0, at.pos.y + 5.0]), 3);
    u.inject("ui_click", json!(true), 1);
    u.step(2);
    assert!(!u.tb(field).1, "a click elsewhere stops it");

    // A box that keeps typing: Enter sends and it goes on.
    u.w.update::<UiTextBox, _>(field, |t| {
        t.keep = true;
        t.typing = true;
    })
    .unwrap();
    u.step(2);
    u.typed("one\rtwo\r");
    let sent: Vec<Value> = u
        .events("ui.submitted")
        .into_iter()
        .map(|f| f.iter().find(|(k, _)| k == "text").unwrap().1.clone())
        .collect();
    assert_eq!(
        sent,
        vec![Value::Str("abcone".into()), Value::Str("two".into())],
        "Escape kept what was typed"
    );
    assert!(u.tb(field).1, "still typing");
}

#[test]
fn a_log_keeps_its_newest_line_in_view_until_scrolled_back() {
    let mut u = ui();
    let root = u.node(None, UiNode::default());
    u.w.insert(root, UiRoot::default()).unwrap();
    let log = u.node(
        Some(root),
        UiNode {
            size: Vec2::new(300.0, 100.0),
            ..UiNode::default()
        },
    );
    u.w.insert(log, UiScroll::default()).unwrap();
    let add = |u: &mut Ui, n: usize| -> Entity {
        let e = u.node(
            Some(log),
            UiNode {
                size: Vec2::new(0.0, 20.0),
                ..UiNode::default()
            },
        );
        let t = id(&u.w, &format!("line {n}"));
        u.w.insert(
            e,
            UiText {
                text: t,
                ..UiText::default()
            },
        )
        .unwrap();
        e
    };
    let lines: Vec<Entity> = (0..10).map(|n| add(&mut u, n)).collect();
    u.step(2);
    let s = u.w.get::<UiScroll>(log).unwrap();
    assert_eq!((s.content, s.view), (200.0, 100.0), "{s:?}");
    assert_eq!(s.y, 100.0, "at the end");
    assert!(s.at_end);
    let last = u.w.get::<argus_ui::UiRect>(*lines.last().unwrap()).unwrap();
    let box_ = u.w.get::<argus_ui::UiRect>(log).unwrap();
    assert_eq!(
        last.pos.y + last.size.y,
        box_.pos.y + box_.size.y,
        "the newest line at the bottom"
    );
    let first = u.w.get::<argus_ui::UiRect>(lines[0]).unwrap();
    assert!(
        first.pos.y < first.clip_min.y,
        "the oldest scrolled out of view"
    );
    // A new line: it follows.
    add(&mut u, 10);
    u.step(2);
    assert_eq!(u.w.get::<UiScroll>(log).unwrap().y, 120.0);

    // The wheel over it scrolls back, and a new line leaves it there.
    let p = [box_.pos.x + 10.0, box_.pos.y + 10.0];
    u.inject("pointer", json!(p), 10);
    u.inject("ui_scroll", json!(1.0), 1);
    u.step(2);
    let s = u.w.get::<UiScroll>(log).unwrap();
    assert_eq!(s.y, 80.0, "one notch, 40 px back");
    assert!(!s.at_end);
    add(&mut u, 11);
    u.step(2);
    assert_eq!(
        u.w.get::<UiScroll>(log).unwrap().y,
        80.0,
        "it stays where it was put"
    );
    // Scrolled down to the end again, it follows again.
    u.w.update::<UiScroll, _>(log, |s| s.y = 1000.0).unwrap();
    u.step(2);
    let s = u.w.get::<UiScroll>(log).unwrap();
    assert_eq!(s.y, 140.0, "clamped to the end");
    assert!(s.at_end);
    add(&mut u, 12);
    u.step(2);
    assert_eq!(u.w.get::<UiScroll>(log).unwrap().y, 160.0);
}
