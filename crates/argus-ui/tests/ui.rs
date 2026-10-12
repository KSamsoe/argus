//! The UI tree (phase 3 item 1, §12): laid out flexbox style on the canvas,
//! text sized by its words, hung from the world through the camera, and
//! driven by a player's actions: focus moves, presses, and backing out.

use argus_ecs::{Entity, Parent, StrId, Transform, Value, Vec2, World};
use argus_input::{ActionMap, InputState};
use argus_sim::{Scheduler, SchedulerConfig};
use argus_ui::{Shared, UiButton, UiNode, UiRect, UiRoot, UiText};
use serde_json::json;
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
}

fn ui() -> Ui {
    ui_with(Rc::new(RefCell::new(Shared::default())))
}

fn ui_with(shared: Rc<RefCell<Shared>>) -> Ui {
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::default());
    argus_ecs::hierarchy::register(&mut w).unwrap();
    argus_camera::register(&mut w).unwrap();
    let map = ActionMap::parse("input.toml", MAP).unwrap();
    let input = Rc::new(RefCell::new(InputState::new(map, false)));
    argus_input::install(&mut w, &mut s, input.clone()).unwrap();
    argus_ui::install(&mut w, &mut s, shared).unwrap();
    Ui { w, s, input }
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
    fn rect(&self, e: Entity) -> UiRect {
        self.w.get::<UiRect>(e).unwrap()
    }
    fn at(&self, e: Entity) -> [f32; 4] {
        let r = self.rect(e);
        [r.pos.x, r.pos.y, r.size.x, r.size.y]
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
}

#[test]
fn a_tree_lays_out_as_a_flexbox_on_the_canvas() {
    let mut u = ui();
    let top = u.node(
        None,
        UiNode {
            pad: Vec2::new(10.0, 10.0),
            gap: 5.0,
            ..UiNode::default()
        },
    );
    let a = u.node(
        Some(top),
        UiNode {
            size: Vec2::new(100.0, 20.0),
            offset: Vec2::new(3.0, 4.0),
            ..UiNode::default()
        },
    );
    let a1 = u.node(
        Some(a),
        UiNode {
            size: Vec2::new(10.0, 10.0),
            ..UiNode::default()
        },
    );
    let b = u.node(
        Some(top),
        UiNode {
            fill: Vec2::new(0.5, 0.0),
            size: Vec2::new(0.0, 30.0),
            ..UiNode::default()
        },
    );
    let hidden = u.node(
        Some(top),
        UiNode {
            size: Vec2::new(10.0, 100.0),
            visible: false,
            order: -1,
            ..UiNode::default()
        },
    );
    let corner = u.node(
        Some(top),
        UiNode {
            size: Vec2::new(50.0, 40.0),
            absolute: true,
            anchor: Vec2::new(1.0, 1.0),
            pivot: Vec2::new(1.0, 1.0),
            ..UiNode::default()
        },
    );
    let dir = id(&u.w, "row");
    let center = id(&u.w, "center");
    let row = u.node(
        Some(top),
        UiNode {
            direction: dir,
            justify: center,
            size: Vec2::new(0.0, 20.0),
            clip: true,
            opacity: 0.5,
            ..UiNode::default()
        },
    );
    let r1 = u.node(
        Some(row),
        UiNode {
            size: Vec2::new(100.0, 20.0),
            ..UiNode::default()
        },
    );
    let r2 = u.node(
        Some(row),
        UiNode {
            size: Vec2::new(100.0, 20.0),
            ..UiNode::default()
        },
    );
    u.s.run_load(&mut u.w).unwrap();

    assert_eq!(u.at(top), [0.0, 0.0, 960.0, 540.0], "the canvas");
    assert_eq!(u.at(a), [13.0, 14.0, 100.0, 20.0], "padded, then offset");
    assert_eq!(
        u.at(a1),
        [13.0, 14.0, 10.0, 10.0],
        "an offset moves the subtree"
    );
    assert_eq!(
        u.at(b),
        [10.0, 35.0, 470.0, 30.0],
        "half the content width, after the gap"
    );
    assert_eq!(
        u.at(corner),
        [910.0, 500.0, 50.0, 40.0],
        "bottom right by anchor and pivot"
    );
    assert!(!u.rect(hidden).shown, "hidden");
    assert_eq!(u.at(hidden)[2], 0.0, "and takes no space");
    // A row, centred: 940 of content, 200 of children.
    assert_eq!(u.at(row), [10.0, 70.0, 940.0, 20.0]);
    assert_eq!(u.at(r1), [380.0, 70.0, 100.0, 20.0]);
    assert_eq!(u.at(r2), [480.0, 70.0, 100.0, 20.0]);
    let r = u.rect(r1);
    assert_eq!(r.alpha, 0.5, "opacity passes down");
    assert_eq!(
        (r.clip_min, r.clip_max),
        (Vec2::new(10.0, 70.0), Vec2::new(950.0, 90.0)),
        "the row clips"
    );
    // Draw order: tree order.
    assert!(u.rect(top).depth < u.rect(a).depth && u.rect(a).depth < u.rect(a1).depth);
}

#[test]
fn text_is_as_big_as_its_words_wrapped_and_its_holes_are_filled() {
    let mut u = ui();
    let top = u.node(
        None,
        UiNode {
            gap: 5.0,
            align: StrId::EMPTY,
            ..UiNode::default()
        },
    );
    let start = id(&u.w, "start");
    let wrapped = u.node(
        Some(top),
        UiNode {
            size: Vec2::new(48.0, 0.0),
            align_self: start,
            ..UiNode::default()
        },
    );
    let t = id(&u.w, "one two three");
    u.w.insert(
        wrapped,
        UiText {
            text: t,
            size: 8.0,
            ..UiText::default()
        },
    )
    .unwrap();
    let line = u.node(
        Some(top),
        UiNode {
            align_self: start,
            pad: Vec2::new(4.0, 2.0),
            ..UiNode::default()
        },
    );
    let t = id(&u.w, "gap {@top:UiNode.gap}");
    u.w.insert(
        line,
        UiText {
            text: t,
            size: 8.0,
            wrap: false,
            ..UiText::default()
        },
    )
    .unwrap();
    // Name the top so the hole can find it.
    let named = u.w.spawn_authored("top").unwrap();
    u.w.insert(
        named,
        UiNode {
            gap: 5.0,
            visible: false,
            ..UiNode::default()
        },
    )
    .unwrap();
    u.s.run_load(&mut u.w).unwrap();

    assert_eq!(u.at(wrapped)[2..], [48.0, 20.0], "two lines of 10");
    let shown = u.w.get::<UiText>(line).unwrap().shown;
    assert_eq!(&*u.w.strings().resolve(shown), "gap 5");
    // "gap 5" is 5 characters of 6, padded 4 and 2.
    assert_eq!(u.at(line)[2..], [38.0, 14.0]);
}

#[test]
fn a_tree_hung_from_the_world_sits_at_its_place_on_screen() {
    let mut u = ui();
    let cam = u.w.spawn();
    u.w.insert(cam, Transform::at(0.0, 0.0)).unwrap();
    u.w.insert(
        cam,
        argus_camera::Camera {
            height: 10.0,
            ..Default::default()
        },
    )
    .unwrap();
    let sign = u.w.spawn();
    u.w.insert(sign, Transform::at(2.5, 0.0)).unwrap();
    let top = u.node(
        None,
        UiNode {
            pivot: Vec2::new(0.5, 1.0),
            ..UiNode::default()
        },
    );
    u.w.insert(
        top,
        UiRoot {
            follow: sign,
            follow_offset: Vec2::new(0.0, 1.0),
            interactive: false,
            ..UiRoot::default()
        },
    )
    .unwrap();
    let t = id(&u.w, "Mind the pit.");
    u.w.insert(
        top,
        UiText {
            text: t,
            size: 8.0,
            ..UiText::default()
        },
    )
    .unwrap();
    u.s.run_load(&mut u.w).unwrap();
    // The view is 10 tall and 16:9 wide on a 960 x 540 canvas: 54 pixels a
    // unit. (2.5, 1) is (615, 216); the words (13 x 6 = 78 by 10) sit
    // centred above it.
    let [x, y, w, h] = u.at(top);
    assert_eq!([w, h], [78.0, 10.0]);
    assert!(
        (x - (615.0 - 39.0)).abs() < 0.01 && (y - 206.0).abs() < 0.01,
        "{x}, {y}"
    );
}

#[test]
fn a_menu_is_moved_through_and_pressed_with_actions() {
    let mut u = ui();
    let player = u.w.spawn();
    u.w.insert(player, argus_input::PlayerSlot { slot: 1 })
        .unwrap();
    let actions = u.w.component_id("Actions").unwrap();
    u.w.insert_default(player, actions).unwrap();
    let menu = u.node(
        None,
        UiNode {
            gap: 4.0,
            pad: Vec2::new(20.0, 20.0),
            ..UiNode::default()
        },
    );
    u.w.insert(menu, UiRoot::default()).unwrap();
    let mut buttons = Vec::new();
    for name in ["menu.resume", "menu.options", "menu.quit"] {
        let b = u.node(
            Some(menu),
            UiNode {
                size: Vec2::new(100.0, 20.0),
                ..UiNode::default()
            },
        );
        let ev = id(&u.w, name);
        u.w.insert(
            b,
            UiButton {
                event: ev,
                ..UiButton::default()
            },
        )
        .unwrap();
        buttons.push(b);
    }
    // Disabled: focus passes over it.
    u.w.update::<UiButton, _>(buttons[1], |b| b.disabled = true)
        .unwrap();
    u.s.run_load(&mut u.w).unwrap();
    u.step(1);
    let focus = |u: &Ui| u.w.get::<UiRoot>(menu).unwrap().focus;
    assert_eq!(focus(&u), buttons[0], "the first button takes focus");
    assert!(u.w.get::<UiButton>(buttons[0]).unwrap().focused);

    // Down once: over the disabled one to the last.
    u.events("ui.focused");
    u.inject("ui_move", json!([0, -1]), 1);
    u.step(2);
    assert_eq!(focus(&u), buttons[2]);
    assert_eq!(u.events("ui.focused").len(), 1);
    // Held up: once at once, then it repeats; nothing is above the first.
    u.inject("ui_move", json!([0, 1]), 60);
    u.step(60);
    assert_eq!(focus(&u), buttons[0]);

    // Confirm presses the focused one: ui.pressed and its own event.
    u.events("x");
    u.inject("ui_confirm", json!(true), 1);
    u.step(1);
    let evs = u.w.drain_events();
    let names: Vec<String> = evs
        .iter()
        .filter_map(|r| match &r.event {
            argus_ecs::Event::Game { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert!(names.contains(&"ui.pressed".to_string()), "{names:?}");
    assert!(names.contains(&"menu.resume".to_string()), "{names:?}");
    assert!(u.w.get::<UiButton>(buttons[0]).unwrap().pressed);
    u.step(1);
    assert!(
        !u.w.get::<UiButton>(buttons[0]).unwrap().pressed,
        "for one tick"
    );

    // Cancel backs out.
    u.inject("ui_cancel", json!(true), 1);
    u.step(1);
    assert_eq!(u.events("ui.cancel").len(), 1);
}

#[test]
fn the_pointer_focuses_what_it_moves_over_and_a_click_presses_it() {
    let mut u = ui();
    let player = u.w.spawn();
    u.w.insert(player, argus_input::PlayerSlot { slot: 1 })
        .unwrap();
    let actions = u.w.component_id("Actions").unwrap();
    u.w.insert_default(player, actions).unwrap();
    let menu = u.node(
        None,
        UiNode {
            gap: 10.0,
            pad: Vec2::new(20.0, 20.0),
            ..UiNode::default()
        },
    );
    u.w.insert(menu, UiRoot::default()).unwrap();
    let mut buttons = Vec::new();
    for name in ["a", "b", "c"] {
        let b = u.node(
            Some(menu),
            UiNode {
                size: Vec2::new(100.0, 20.0),
                ..UiNode::default()
            },
        );
        let ev = id(&u.w, name);
        u.w.insert(
            b,
            UiButton {
                event: ev,
                ..UiButton::default()
            },
        )
        .unwrap();
        buttons.push(b);
    }
    u.s.run_load(&mut u.w).unwrap();
    u.step(1);
    // The third button is at (20, 80) to (120, 100).
    u.inject("pointer", json!([50, 90]), 1);
    u.step(1);
    assert_eq!(u.w.get::<UiRoot>(menu).unwrap().focus, buttons[2]);
    assert!(u.w.get::<UiButton>(buttons[2]).unwrap().hovered);
    // A pointer stays where it was left.
    u.step(5);
    assert!(u.w.get::<UiButton>(buttons[2]).unwrap().hovered);
    // The keyboard moves focus away; the pointer, not moving, leaves it.
    u.inject("ui_move", json!([0, 1]), 1);
    u.step(2);
    assert_eq!(u.w.get::<UiRoot>(menu).unwrap().focus, buttons[1]);
    // A click presses what is under the pointer, and focuses it.
    u.events("x");
    u.inject("ui_click", json!(true), 1);
    u.step(1);
    let pressed = u.events("ui.pressed");
    assert_eq!(pressed.len(), 1);
    assert!(
        pressed[0].contains(&("entity".to_string(), Value::Entity(buttons[2]))),
        "{pressed:?}"
    );
    assert!(pressed[0].contains(&("by".to_string(), Value::Str("pointer".into()))));
    // A click over nothing presses nothing.
    u.inject("pointer", json!([500, 500]), 1);
    u.step(1);
    u.events("x");
    u.inject("ui_click", json!(true), 1);
    u.step(1);
    assert!(u.events("ui.pressed").is_empty());
}

/// §12 accessibility: "text scaling and high-contrast palettes are UI theme
/// tokens". A player's UiSettings scales every text and swaps the palette.
#[test]
fn a_players_settings_scale_the_text_and_swap_the_palette() {
    let mut u = ui();
    let top = u.node(
        None,
        UiNode {
            align: id(&u.w, "start"),
            ..UiNode::default()
        },
    );
    let label = u.node(Some(top), UiNode::default());
    let t = id(&u.w, "Hello");
    u.w.insert(
        label,
        UiText {
            text: t,
            size: 8.0,
            ..UiText::default()
        },
    )
    .unwrap();
    let panel = id(&u.w, "panel");
    u.w.insert(
        label,
        argus_ui::UiPanel {
            color: panel,
            ..Default::default()
        },
    )
    .unwrap();
    u.s.run_load(&mut u.w).unwrap();
    assert_eq!(
        u.at(label)[2..],
        [30.0, 10.0],
        "5 characters of 6 at size 8"
    );
    let shared = Shared::default();
    let quads = |w: &World| argus_ui::draw::draw_list(w, &shared, &|_, _| None);
    let before = quads(&u.w);
    let flat = |q: &[argus_ui::draw::Quad]| {
        q.iter()
            .find(|q| q.paint == argus_ui::draw::Paint::Flat)
            .map(|q| q.color)
            .unwrap()
    };
    let text = |q: &[argus_ui::draw::Quad]| {
        q.iter()
            .find(|q| matches!(q.paint, argus_ui::draw::Paint::Glyph { .. }))
            .map(|q| q.color)
            .unwrap()
    };
    assert_eq!(flat(&before), shared.theme.color("panel", false).unwrap());

    // Twice the text, high contrast.
    let s = u.w.spawn();
    u.w.insert(
        s,
        argus_ui::UiSettings {
            text_scale: 2.0,
            high_contrast: true,
            fullscreen: true,
            ..Default::default()
        },
    )
    .unwrap();
    u.step(1);
    assert!(argus_ui::wants_fullscreen(&u.w), "the window reads it");
    assert_eq!(
        u.at(label)[2..],
        [60.0, 20.0],
        "the text scaled, and its node"
    );
    let after = quads(&u.w);
    assert_eq!(
        flat(&after),
        shared.theme.color("panel", true).unwrap(),
        "the high-contrast panel"
    );
    assert_eq!(text(&after), [1.0, 1.0, 1.0, 1.0], "white text");
    assert_ne!(flat(&before), flat(&after));
}

/// §13: "UI slides are tweens". A tween of UiNode.offset moves the node.
#[test]
fn a_tween_slides_a_node() {
    let mut u = ui();
    argus_tween::install(&mut u.w, &mut u.s).unwrap();
    let top = u.node(None, UiNode::default());
    let slide = u.node(
        Some(top),
        UiNode {
            size: Vec2::new(50.0, 50.0),
            absolute: true,
            ..UiNode::default()
        },
    );
    let tw = u.w.spawn();
    let path = id(&u.w, "UiNode.offset");
    u.w.insert(
        tw,
        argus_tween::Tween {
            target: slide,
            path,
            from: Vec2::new(-200.0, 0.0),
            to: Vec2::new(0.0, 0.0),
            from_current: false,
            duration: 0.5,
            ..Default::default()
        },
    )
    .unwrap();
    u.s.run_load(&mut u.w).unwrap();
    u.step(1);
    let x0 = u.at(slide)[0];
    assert!(x0 < -150.0, "starts off to the left: {x0}");
    u.step(40);
    assert_eq!(u.at(slide)[0], 0.0, "slid in");
}

/// §17: "UI tree, 2,000 nodes with text: < 1 ms layout" ("inspector panels
/// and overview screens are most of what the player looks at"). An
/// inspector's worth: 400 rows of five cells, each cell with words. Run in
/// release: cargo test -p argus-ui --release -- --ignored perf.
#[test]
#[ignore]
fn perf_2000_nodes_with_text_lay_out_in_a_millisecond() {
    let mut u = ui();
    let top = u.node(
        None,
        UiNode {
            gap: 2.0,
            clip: true,
            ..UiNode::default()
        },
    );
    let row_dir = id(&u.w, "row");
    let mut n = 1;
    for r in 0..400 {
        let row = u.node(
            Some(top),
            UiNode {
                direction: row_dir,
                gap: 6.0,
                order: r,
                ..UiNode::default()
            },
        );
        n += 1;
        for c in 0..4 {
            let cell = u.node(
                Some(row),
                UiNode {
                    grow: 1.0,
                    order: c,
                    ..UiNode::default()
                },
            );
            let t = id(
                &u.w,
                &format!("colonist {r} need {c}: {}", (r * 7 + c * 3) % 100),
            );
            u.w.insert(
                cell,
                UiText {
                    text: t,
                    size: 12.0,
                    ..UiText::default()
                },
            )
            .unwrap();
            n += 1;
        }
    }
    assert!(n >= 2000, "{n}");
    u.s.run_load(&mut u.w).unwrap();
    u.step(5);
    // Change a text each tick, as a live inspector does.
    let mut worst = 0.0f64;
    let mut total = 0.0f64;
    let runs = 50;
    for i in 0..runs {
        let t = id(&u.w, &format!("tick {i}"));
        let first =
            u.w.query()
                .has(u.w.component_id_of::<UiText>().unwrap())
                .entities()[0];
        u.w.update::<UiText, _>(first, |x| x.text = t).unwrap();
        u.step(1);
        let rec = u.s.prof_last().unwrap();
        let ms = rec
            .systems
            .iter()
            .find(|s| &*s.name == argus_ui::LAYOUT)
            .map(|s| s.elapsed.as_secs_f64() * 1000.0)
            .unwrap();
        worst = worst.max(ms);
        total += ms;
    }
    let mean = total / runs as f64;
    eprintln!("ui.layout over {n} nodes: mean {mean:.3} ms, worst {worst:.3} ms");
    assert!(mean < 1.0, "mean {mean:.3} ms");
}

/// §12: "Rebinding at runtime is a UI over the same table". A controls
/// menu's button with an InputBinding shows the action's keys; pressed, it
/// waits for the next key, which is bound and which the game does not
/// hear; Escape cancels.
#[test]
fn a_controls_button_rebinds_an_action_to_the_next_key() {
    let mut u = ui();
    argus_input::rebind::install(&mut u.w, &mut u.s, u.input.clone()).unwrap();
    let player = u.w.spawn();
    u.w.insert(player, argus_input::PlayerSlot { slot: 1 })
        .unwrap();
    let actions = u.w.component_id("Actions").unwrap();
    u.w.insert_default(player, actions).unwrap();
    let menu = u.node(None, UiNode::default());
    u.w.insert(menu, UiRoot::default()).unwrap();
    let b = u.node(
        Some(menu),
        UiNode {
            size: Vec2::new(200.0, 20.0),
            ..UiNode::default()
        },
    );
    u.w.insert(b, UiButton::default()).unwrap();
    let (jump, kb) = (id(&u.w, "jump"), id(&u.w, "keyboard"));
    u.w.insert(
        b,
        argus_input::InputBinding {
            action: jump,
            device: kb,
            ..Default::default()
        },
    )
    .unwrap();
    let t = id(&u.w, "Jump: {InputBinding.keys}");
    u.w.insert(
        b,
        UiText {
            text: t,
            ..UiText::default()
        },
    )
    .unwrap();
    u.s.run_load(&mut u.w).unwrap();
    u.step(2);
    let shown = |u: &Ui| {
        let t = u.w.get::<UiText>(b).unwrap().shown;
        u.w.strings().resolve(t).to_string()
    };
    assert_eq!(shown(&u), "Jump: space");

    // Pressed: it waits for a key.
    u.inject("ui_confirm", json!(true), 1);
    u.step(2);
    assert!(u.w.get::<argus_input::InputBinding>(b).unwrap().listening);
    assert_eq!(shown(&u), "Jump: press a key");
    // The next key is taken for jump, and the game does not hear it.
    let x = argus_input::DeviceInput::Key {
        name: "KeyX".into(),
        pressed: true,
    };
    let next = u.s.tick();
    assert!(u.input.borrow_mut().capture(next, &x), "taken");
    u.events("x");
    u.step(2);
    let rebound = u.events("input.rebound");
    assert_eq!(rebound.len(), 1);
    assert!(
        rebound[0].contains(&("binding".to_string(), Value::Str("x".into()))),
        "{rebound:?}"
    );
    assert_eq!(shown(&u), "Jump: x");
    assert_eq!(u.input.borrow().map().bindings("jump", "keyboard"), ["x"]);
    // X now jumps; space does not.
    let mut dev = argus_input::Devices::default();
    dev.apply(&argus_input::DeviceInput::Key {
        name: "KeyX".into(),
        pressed: true,
    });
    let now = u.s.tick();
    u.input.borrow_mut().apply_devices(now, &dev);
    u.step(1);
    assert_eq!(
        u.w.get_field(player, "Actions.jump").unwrap(),
        Value::Bool(true)
    );

    // Escape cancels a wait and changes nothing.
    u.inject("ui_confirm", json!(true), 1);
    u.step(2);
    let esc = argus_input::DeviceInput::Key {
        name: "Escape".into(),
        pressed: true,
    };
    let next = u.s.tick();
    assert!(u.input.borrow_mut().capture(next, &esc));
    u.events("x");
    u.step(2);
    assert_eq!(u.events("input.rebind_cancelled").len(), 1);
    assert_eq!(shown(&u), "Jump: x");
}

// ----- SDF text with shaping ----------------------------------------------------

/// Delve's font (Liberation Sans, OFL), imported as a project would.
fn sans() -> argus_assets::FontAsset {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = temp(&format!("ui-font-{n}"));
    std::fs::create_dir_all(dir.join("assets/fonts")).unwrap();
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/delve/assets/fonts/LiberationSans-Regular.ttf"),
        dir.join("assets/fonts/sans.ttf"),
    )
    .unwrap();
    let mut imp = argus_assets::Importer::new(&dir);
    imp.scan();
    let f = imp.fonts().next().expect("the font imported").1.clone();
    drop(dir);
    f
}

#[test]
fn sdf_text_is_shaped_so_it_kerns_and_composes() {
    use argus_ui::text::{lines, size_of};
    use argus_ui::{FontFace, SdfFont};
    let f = sans();
    let font = SdfFont::from_asset("fonts/sans", &f);
    assert!(font.sdf());
    assert_eq!(&*font.atlas(), "fonts/sans");
    // Kerning: "AV" is narrower than an A and a V set apart.
    let av = font.measure("AV", 32.0);
    let apart = font.measure("A", 32.0) + font.measure("V", 32.0);
    assert!(av < apart - 0.5, "AV {av} vs A + V {apart}");
    // Composition: an e and a combining acute are the one glyph é.
    assert_eq!(font.glyph_ids("e\u{301}"), font.glyph_ids("\u{e9}"));
    // Other scripts the font has shape to their own glyphs (Greek and
    // Cyrillic: baked when a sidecar's chars asks for them).
    let face = ttf_parser::Face::parse(&f.bytes, 0).unwrap();
    for word in [
        "\u{3b1}\u{3b2}\u{3b3}",
        "\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}",
    ] {
        let want: Vec<u16> = word
            .chars()
            .map(|c| face.glyph_index(c).unwrap().0)
            .collect();
        assert_eq!(font.glyph_ids(word), want, "{word}");
    }
    // Proportional, and linear in the size.
    assert!(font.measure("i", 16.0) < font.measure("m", 16.0) * 0.5);
    let (a, b) = (font.measure("Hello", 32.0), font.measure("Hello", 16.0));
    assert!((a - 2.0 * b).abs() < 1e-3, "{a} {b}");
    // Glyphs: a space draws nothing; each sits in its line, within the
    // field's reach of it.
    let g = font.glyphs("Hi there", 20.0);
    assert_eq!(g.len(), 7);
    let reach = f.spread * 20.0 / f.size + 1.0;
    for gl in &g {
        assert!(gl.y > -reach && gl.y + gl.h < 25.0 + reach, "{gl:?}");
    }
    assert!(g.windows(2).all(|p| p[0].x < p[1].x), "left to right");
    // Breaking a line measures shaped words, and agrees with sizing.
    for text in [
        "one two three",
        "AVATAR WAVE",
        "a\n\nb",
        "Tr\u{e8}s bien, \u{e9}t\u{e9}",
    ] {
        for max in [None, Some(30.0), Some(60.0), Some(1000.0)] {
            let l = lines(&font, text, 16.0, max);
            assert_eq!(
                size_of(&font, text, 16.0, max),
                (l.width, l.height),
                "{text:?} {max:?}"
            );
        }
    }
}

#[test]
fn a_font_asset_lays_out_by_its_advances_and_draws_from_its_field() {
    use argus_ui::{FontFace, SdfFont};
    let font = Rc::new(SdfFont::from_asset("fonts/sans", &sans()));
    let shared = Rc::new(RefCell::new(Shared::default()));
    shared.borrow_mut().fonts.add("fonts/sans", font.clone());
    let mut u = ui_with(shared.clone());
    let top = u.node(None, UiNode::default());
    let label = u.node(
        Some(top),
        UiNode {
            absolute: true,
            ..UiNode::default()
        },
    );
    let (t, sans) = (id(&u.w, "Wave to AVA"), id(&u.w, "fonts/sans"));
    u.w.insert(
        label,
        UiText {
            text: t,
            size: 20.0,
            font: sans,
            wrap: false,
            ..UiText::default()
        },
    )
    .unwrap();
    u.s.run_load(&mut u.w).unwrap();
    // As wide as its words and spaces, in whole pixels up (the line shaped
    // whole kerns across a space and is a little narrower).
    let r = u.at(label);
    let want = argus_ui::text::size_of(font.as_ref(), "Wave to AVA", 20.0, None).0;
    assert_eq!(r[2], want.ceil());
    assert!(font.measure("Wave to AVA", 20.0) <= want);
    assert_eq!(r[3], 25.0, "one line");
    let quads = argus_ui::draw::draw_list(&u.w, &shared.borrow(), &|_, _| None);
    let glyphs: Vec<_> = quads
        .iter()
        .filter_map(|q| match &q.paint {
            argus_ui::draw::Paint::Glyph { atlas, sdf, .. } => Some((atlas.to_string(), *sdf)),
            _ => None,
        })
        .collect();
    assert_eq!(glyphs.len(), 9, "the letters, not the spaces");
    assert!(glyphs.iter().all(|(a, sdf)| a == "fonts/sans" && *sdf));
    // Drawn at the width it was laid out at, it stays on its one line (a
    // Delve menu title wrapped under its buttons before).
    let tops: Vec<f32> = quads
        .iter()
        .filter(|q| matches!(q.paint, argus_ui::draw::Paint::Glyph { .. }))
        .map(|q| q.pos[1] + q.size[1])
        .collect();
    let spread = tops.iter().cloned().fold(f32::MIN, f32::max)
        - tops.iter().cloned().fold(f32::MAX, f32::min);
    assert!(spread < 12.0, "one line of glyphs: {tops:?}");

    // A font replaced (reimported): the layout measures again.
    shared
        .borrow_mut()
        .fonts
        .add("fonts/sans", Rc::new(argus_ui::PixelFont::default()));
    u.step(1);
    assert_eq!(u.at(label)[2], 11.0 * 15.0, "11 pixel-font cells of 15");
}
