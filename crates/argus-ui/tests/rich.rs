//! Reaching players item 3: rich text in a `UiText`. Colour, weight,
//! slant, size, a key the player presses on their own device, and a
//! picture; the plain words for what reads them; what does not read, said.

use argus_ecs::{Entity, Parent, StrId, World};
use argus_input::{ActionMap, InputState};
use argus_sim::{Scheduler, SchedulerConfig};
use argus_ui::draw::{draw_list, Paint, Quad};
use argus_ui::{Shared, UiNode, UiRect, UiRoot, UiText};
use serde_json::json;
use std::cell::RefCell;
use std::rc::Rc;

const MAP: &str = r#"
[actions.jump]
type = "button"
[actions.jump.bindings]
keyboard = ["space"]
gamepad = ["south"]
[slots.1]
devices = ["keyboard", "gamepad"]
"#;

struct Ui {
    w: World,
    s: Scheduler,
    input: Rc<RefCell<InputState>>,
    shared: Rc<RefCell<Shared>>,
    root: Entity,
}

fn ui() -> Ui {
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::default());
    argus_ecs::hierarchy::register(&mut w).unwrap();
    argus_camera::register(&mut w).unwrap();
    let map = ActionMap::parse("input.toml", MAP).unwrap();
    let input = Rc::new(RefCell::new(InputState::new(map, false)));
    argus_input::install(&mut w, &mut s, input.clone()).unwrap();
    let shared = Rc::new(RefCell::new(Shared {
        input: Some(input.clone()),
        ..Shared::default()
    }));
    argus_ui::install(&mut w, &mut s, shared.clone()).unwrap();
    let player = w.spawn();
    w.insert_by_name(player, "PlayerSlot").unwrap();
    w.insert_by_name(player, "Actions").unwrap();
    // Children as wide as their words, not stretched across the canvas.
    let root = w.spawn();
    let start = w.strings().intern("start");
    w.insert(
        root,
        UiNode {
            align: start,
            ..UiNode::default()
        },
    )
    .unwrap();
    w.insert(root, UiRoot::default()).unwrap();
    Ui {
        w,
        s,
        input,
        shared,
        root,
    }
}

impl Ui {
    fn text(&mut self, words: &str, size: Option<f32>) -> Entity {
        let e = self.w.spawn();
        self.w
            .insert(
                e,
                UiNode {
                    size: argus_ecs::Vec2::new(size.unwrap_or(0.0), 0.0),
                    ..UiNode::default()
                },
            )
            .unwrap();
        self.w.insert(e, Parent { entity: self.root }).unwrap();
        let t = self.id(words);
        self.w
            .insert(
                e,
                UiText {
                    text: t,
                    wrap: size.is_some(),
                    ..UiText::default()
                },
            )
            .unwrap();
        e
    }
    fn id(&self, s: &str) -> StrId {
        self.w.strings().intern(s)
    }
    fn step(&mut self, n: u64) {
        self.s.step(&mut self.w, n).unwrap();
    }
    fn shown(&self, e: Entity) -> String {
        let t = self.w.get::<UiText>(e).unwrap();
        self.w.strings().resolve(t.shown).to_string()
    }
    fn quads(&self) -> Vec<Quad> {
        draw_list(&self.w, &self.shared.borrow(), &|_, _| Some([16.0, 16.0]))
    }
}

#[test]
fn rich_text_is_drawn_in_pieces_and_its_key_follows_the_device() {
    let mut u = ui();
    let e = u.text(
        "Press [glyph=jump] to [color=#FF0000][b]jump[/b][/color], [i]now[/i] [img=icons:2]",
        Some(600.0),
    );
    u.step(2);
    // The key is the player's own, on the keyboard.
    assert_eq!(
        u.shown(e),
        "Press [key]Space[/key] to [color=#FF0000][b]jump[/b][/color], [i]now[/i] [img=icons:2]"
    );
    assert_eq!(
        argus_ui::rich::plain(&u.shown(e)),
        "Press Space to jump, now "
    );
    let qs = u.quads();
    // Red and bold: the pixel font draws each glyph twice.
    let red = qs
        .iter()
        .filter(|q| {
            matches!(q.paint, Paint::Glyph { bold: true, .. })
                && q.color[0] > 0.9
                && q.color[1] < 0.1
        })
        .count();
    assert_eq!(red, 8, "jump, twice over");
    assert!(qs
        .iter()
        .any(|q| matches!(q.paint, Paint::Glyph { slant, .. } if slant > 0.0)));
    assert!(qs.iter().any(
        |q| matches!(&q.paint, Paint::Image { sprite, frame: 2, .. } if &**sprite == "icons")
    ));
    // The key on its plate.
    assert!(qs
        .iter()
        .any(|q| matches!(q.paint, Paint::Flat) && (q.color[3] - 0.3).abs() < 0.01));

    // A pad in use (the device action, as a pad press makes it): its button.
    let now = u.s.tick();
    u.input
        .borrow_mut()
        .inject(now, 1, "device", Some(&json!(1)), None, true)
        .unwrap();
    u.step(2);
    assert!(
        u.shown(e).starts_with("Press [key]A[/key] to"),
        "{}",
        u.shown(e)
    );
}

#[test]
fn rich_text_measures_by_its_pieces_and_says_what_does_not_read() {
    let mut u = ui();
    let big = u.text("[size=32]Big[/size] small", None);
    let plain = u.text("Big small", None);
    let bad = u.text("half [/b] done [glyph=fly]", None);
    u.step(2);
    let h = |e: Entity| u.w.get::<UiRect>(e).unwrap().size.y;
    assert_eq!(h(big), 32.0 * argus_ui::text::LINE);
    assert_eq!(h(plain), 16.0 * argus_ui::text::LINE);
    // Wider than its words alone: "Big" at twice the size.
    assert!(u.w.get::<UiRect>(big).unwrap().size.x > u.w.get::<UiRect>(plain).unwrap().size.x);
    // Shown as written, and said with the node.
    assert_eq!(
        argus_ui::rich::plain(&u.shown(bad)),
        "half [/b] done [glyph=fly]"
    );
    let problems = u.shared.borrow().problems.borrow().clone();
    assert!(
        problems.iter().any(|p| p.contains("[/b] with no [b]"))
            && problems.iter().any(|p| p.contains("[glyph=fly]")),
        "{problems:?}"
    );
}
