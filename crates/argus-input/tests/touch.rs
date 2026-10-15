//! Reaching players item 1: touch. Controls declared in `input.toml` and
//! bound to actions as keys are; a finger off them is the mouse.

use argus_ecs::{Value, Vec2, World};
use argus_input::{
    install, ActionMap, DeviceInput, Devices, InputState, PlayerSlot, TouchKind, TouchPhase,
};
use argus_sim::{Scheduler, SchedulerConfig};
use std::cell::RefCell;
use std::rc::Rc;

const MAP: &str = r#"
[actions.move]
type = "axis2"
[actions.move.bindings]
keyboard = ["wasd"]
touch = ["stick"]

[actions.walk]
type = "axis"
[actions.walk.bindings]
touch = ["stick.x"]

[actions.jump]
type = "button"
[actions.jump.bindings]
keyboard = ["space"]
touch = ["jump"]

[actions.aim]
type = "axis2"
[actions.aim.bindings]
touch = ["aim"]

[actions.click]
type = "button"
[actions.click.bindings]
mouse = ["left"]

[actions.pointer]
type = "pointer"
[actions.pointer.bindings]
mouse = ["position"]

[touch.stick]
kind = "stick"
anchor = "bottom-left"
at = [140, 140]
size = 180

[touch.jump]
kind = "button"
anchor = "bottom-right"
at = [100, 100]
size = 100
label = "Jump!"

[touch.aim]
kind = "stick"
anchor = "right"
at = [200, 0]
size = 120
floating = true

[slots.1]
devices = ["keyboard", "mouse"]
"#;

struct Rig {
    w: World,
    s: Scheduler,
    st: Rc<RefCell<InputState>>,
    d: Devices,
    p: argus_ecs::Entity,
}

impl Rig {
    fn new(discrete: bool) -> Rig {
        let mut w = World::new();
        let mut s = Scheduler::new(if discrete {
            SchedulerConfig::discrete()
        } else {
            SchedulerConfig::continuous(60.0)
        });
        let map = ActionMap::parse("input.toml", MAP).unwrap();
        let st = Rc::new(RefCell::new(InputState::new(map, discrete)));
        install(&mut w, &mut s, st.clone()).unwrap();
        let p = w.spawn_authored("p1").unwrap();
        w.insert(p, PlayerSlot { slot: 1 }).unwrap();
        w.insert_by_name(p, "Actions").unwrap();
        Rig {
            w,
            s,
            st,
            d: Devices::default(),
            p,
        }
    }

    /// A finger's event, the devices applied; the turns it took.
    fn touch(&mut self, id: u64, phase: TouchPhase, x: f32, y: f32) -> usize {
        self.d.apply(&DeviceInput::Touch { id, phase, x, y });
        self.st
            .borrow_mut()
            .apply_devices(self.s.tick(), &mut self.d)
    }

    fn tick(&mut self) {
        self.s.step(&mut self.w, 1).unwrap();
    }

    fn get(&self, field: &str) -> Value {
        self.w
            .get_field(self.p, &format!("Actions.{field}"))
            .unwrap()
    }
}

#[test]
fn a_stick_a_button_and_a_finger_that_is_the_mouse() {
    use TouchPhase::*;
    let mut r = Rig::new(false);
    // The stick's centre is (140, 400) on the 960 by 540 canvas, 90 across
    // its half: half way right is half its x.
    r.touch(1, Start, 185.0, 400.0);
    r.tick();
    assert_eq!(r.get("move"), Value::Vec2(Vec2::new(0.5, 0.0)));
    assert_eq!(r.get("walk"), Value::Float(0.5));
    // All the way up is up, positive; past its edge it stays one long.
    r.touch(1, Move, 140.0, 200.0);
    r.tick();
    assert_eq!(r.get("move"), Value::Vec2(Vec2::new(0.0, 1.0)));
    // A second finger on the button (centre 860, 440): pressed, as touch.
    r.touch(2, Start, 870.0, 430.0);
    r.tick();
    assert_eq!(r.get("jump"), Value::Bool(true));
    assert_eq!(
        r.get("device"),
        Value::Float(5.0),
        "touch is the device in use"
    );
    // A finger on no control is the mouse: its click, and where it is.
    r.touch(3, Start, 300.0, 100.0);
    r.tick();
    assert_eq!(r.get("click"), Value::Bool(true));
    assert_eq!(r.get("pointer"), Value::Vec2(Vec2::new(300.0, 100.0)));
    // Another off the controls does nothing; one on the held stick neither.
    r.touch(4, Start, 320.0, 120.0);
    r.touch(5, Start, 140.0, 400.0);
    r.touch(3, Move, 310.0, 110.0);
    r.tick();
    assert_eq!(r.get("pointer"), Value::Vec2(Vec2::new(310.0, 110.0)));
    assert_eq!(r.get("move"), Value::Vec2(Vec2::new(0.0, 1.0)));
    // The floating stick takes a finger anywhere in its half, centred where
    // it lands (60 across its half).
    r.touch(6, Start, 700.0, 200.0);
    r.touch(6, Move, 745.0, 200.0);
    r.tick();
    assert_eq!(r.get("aim"), Value::Vec2(Vec2::new(0.75, 0.0)));

    // What the engine draws: every control, the stick's knob where the
    // finger is, kept to its edge, the button with its words.
    let shown = r.d.touch_shown(r.st.borrow().map());
    assert_eq!(shown.len(), 3);
    let stick = shown.iter().find(|c| c.name == "stick").unwrap();
    assert!(stick.pressed && stick.kind == TouchKind::Stick);
    assert_eq!(stick.knob, (140.0, 310.0));
    let jump = shown.iter().find(|c| c.name == "jump").unwrap();
    assert_eq!((jump.label.as_str(), jump.pressed), ("Jump!", true));
    let aim = shown.iter().find(|c| c.name == "aim").unwrap();
    assert_eq!(aim.centre, (700.0, 200.0));

    // Lifted: the button lets go, the mouse's click too, where it was left.
    r.touch(2, End, 870.0, 430.0);
    r.touch(3, End, 310.0, 110.0);
    r.tick();
    assert_eq!(r.get("jump"), Value::Bool(false));
    assert_eq!(r.get("click"), Value::Bool(false));
    assert_eq!(r.get("pointer"), Value::Vec2(Vec2::new(310.0, 110.0)));
    // A key: the keyboard is the device in use, and the controls hide.
    r.d.apply(&DeviceInput::Key {
        name: "Space".into(),
        pressed: true,
    });
    r.st.borrow_mut().apply_devices(r.s.tick(), &mut r.d);
    r.tick();
    assert_eq!(r.get("device"), Value::Float(0.0));
    assert!(r.d.touch_shown(r.st.borrow().map()).is_empty());
    // Focus lost: every finger lets go.
    r.d.apply(&DeviceInput::ReleaseAll);
    r.st.borrow_mut().apply_devices(r.s.tick(), &mut r.d);
    r.tick();
    assert_eq!(r.get("move"), Value::Vec2(Vec2::ZERO));
    assert_eq!(r.get("aim"), Value::Vec2(Vec2::ZERO));
}

#[test]
fn on_a_discrete_clock_a_tap_is_a_turn_and_a_stick_pushed_is_one() {
    use TouchPhase::*;
    let mut r = Rig::new(true);
    assert_eq!(r.touch(1, Start, 860.0, 440.0), 1, "the button is a turn");
    assert_eq!(r.touch(1, End, 860.0, 440.0), 0, "letting go is nothing");
    assert_eq!(r.touch(2, Start, 140.0, 400.0), 0, "the stick at rest");
    // Pushed: a command (two here, move and walk both read the stick; a
    // host takes one turn for any number at once).
    assert_eq!(r.touch(2, Move, 230.0, 400.0), 2, "pushed: a step");
    // A finger off the controls places the pointer and clicks: a click is a
    // button like any (here bound to nothing that moves the hero).
    assert_eq!(r.touch(3, Start, 300.0, 100.0), 1);
}

#[test]
fn a_touch_binding_names_a_control_of_the_right_kind() {
    let with = |extra: &str| ActionMap::parse("input.toml", &format!("{MAP}\n{extra}"));
    for (extra, says) in [
        (
            "[actions.fire]\ntype = \"button\"\n[actions.fire.bindings]\ntouch = [\"shoot\"]\n",
            "\"shoot\" is not a button control's name; [touch] declares: stick, jump, aim",
        ),
        (
            "[actions.fire]\ntype = \"button\"\n[actions.fire.bindings]\ntouch = [\"stick\"]\n",
            "not a button control's name",
        ),
        (
            "[actions.lean]\ntype = \"axis\"\n[actions.lean.bindings]\ntouch = [\"stick\"]\n",
            "with .x or .y",
        ),
        (
            "[actions.at]\ntype = \"pointer\"\n[actions.at.bindings]\ntouch = [\"jump\"]\n",
            "a finger off the controls is the mouse",
        ),
    ] {
        let e = with(extra).unwrap_err();
        assert!(e.message.contains(says), "{extra}: {}", e.message);
    }
    for (extra, says) in [
        (
            "[touch.b]\nkind = \"button\"\nfloating = true\n",
            "floating applies to sticks only",
        ),
        ("[touch.b]\nkind = \"pad\"\n", "is not button or stick"),
        (
            "[touch.b]\nkind = \"button\"\nanchor = \"middle\"\n",
            "is not one of",
        ),
        (
            "[touch.b]\nkind = \"button\"\nsize = 0\n",
            "a number above 0",
        ),
        (
            "[touch.b]\nkind = \"button\"\ncolour = \"red\"\n",
            "unknown key \"colour\"",
        ),
    ] {
        let e = with(extra).unwrap_err();
        assert!(e.message.contains(says), "{extra}: {}", e.message);
    }
}

#[test]
fn a_control_is_placed_from_its_anchor_on_the_canvas() {
    let map = ActionMap::parse("input.toml", MAP).unwrap();
    let mut st = InputState::new(map, false);
    st.set_canvas([640.0, 360.0]);
    let l = st.touch_layout();
    let at = |n: &str| l.iter().find(|c| c.name == n).unwrap().centre;
    assert_eq!(at("stick"), (140.0, 220.0));
    assert_eq!(at("jump"), (540.0, 260.0));
    assert_eq!(
        at("aim"),
        (440.0, 180.0),
        "right: from the right edge, at mid height"
    );
}
