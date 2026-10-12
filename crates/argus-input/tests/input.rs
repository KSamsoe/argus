//! Action maps, injected input, edges, toggles, the log, and replay (§6, §12).

use argus_ecs::{Value, Vec2, World};
use argus_input::{codes, install, ActionKind, ActionMap, InputState, LogEntry, PlayerSlot};
use argus_sim::{Scheduler, SchedulerConfig};
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
[actions.move]
type = "axis2"
doc  = "Walk direction"
[actions.move.bindings]
keyboard = ["wasd"]
gamepad  = ["left_stick"]

[actions.fire]
type = "button"
doc  = "Shoot"
[actions.fire.bindings]
keyboard = ["space"]
gamepad  = ["south"]

[actions.crouch]
type   = "button"
toggle = true

[actions.throttle]
type = "axis"

[slots.1]
devices = ["keyboard", "gamepad:0"]
[slots.2]
devices = ["gamepad:1"]
"#;

fn map() -> ActionMap {
    ActionMap::parse("input.toml", MAP).unwrap()
}

/// A world with two slot entities and a scheduler with the input system.
fn rig(discrete: bool) -> (World, Scheduler, Rc<RefCell<InputState>>) {
    let mut w = World::new();
    let mut s = Scheduler::new(if discrete {
        SchedulerConfig::discrete()
    } else {
        SchedulerConfig::continuous(60.0)
    });
    let state = Rc::new(RefCell::new(InputState::new(map(), discrete)));
    install(&mut w, &mut s, state.clone()).unwrap();
    for n in 1..=2u8 {
        let e = w.spawn_authored(&format!("p{n}")).unwrap();
        w.insert(e, PlayerSlot { slot: n }).unwrap();
        w.insert_by_name(e, "Actions").unwrap();
    }
    (w, s, state)
}

#[test]
fn the_map_parses_and_becomes_a_component() {
    let m = map();
    let names: Vec<&str> = m.actions.iter().map(|a| a.name.as_str()).collect();
    // And the engine's own: the kind of device the player last used.
    assert_eq!(names, ["crouch", "device", "fire", "move", "throttle"]);
    assert_eq!(m.action("move").unwrap().kind, ActionKind::Axis2);
    assert_eq!(
        m.action("move").unwrap().bindings["gamepad"],
        ["left_stick"]
    );
    assert!(m.action("crouch").unwrap().toggle);
    assert_eq!(m.slots.len(), 2);
    assert_eq!(m.slot(2).unwrap().devices, ["gamepad:1"]);
    let e = m.action("jump").unwrap_err();
    assert_eq!(e.code, codes::UNKNOWN_ACTION);
    assert!(e.hint.contains("crouch, device, fire, move, throttle"));
    assert_eq!(m.slot(3).unwrap_err().code, codes::BAD_SLOT);

    let schema = m.schema().unwrap();
    assert_eq!(schema.name, "Actions");
    let fields: Vec<&str> = schema.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        fields,
        [
            "crouch",
            "crouch_pressed",
            "crouch_released",
            "device",
            "fire",
            "fire_pressed",
            "fire_released",
            "move",
            "throttle"
        ]
    );
    assert_eq!(schema.field("throttle").unwrap().range, Some((-1.0, 1.0)));
    assert_eq!(schema.field("fire").unwrap().doc, "Shoot");
    assert_eq!(schema.source.as_deref(), Some("input.toml:1"));

    // Errors name the line.
    let e = ActionMap::parse("input.toml", "[actions.jump]\ntype = \"lever\"\n").unwrap_err();
    assert_eq!(e.code, codes::BAD_MAP);
    assert_eq!(e.r#ref.as_deref(), Some("input.toml:1"));
    let e = ActionMap::parse(
        "input.toml",
        "[actions.move]\ntype = \"axis2\"\ntoggle = true\n",
    )
    .unwrap_err();
    assert!(e.message.contains("toggle applies to buttons"));
    let e =
        ActionMap::parse("input.toml", "[actions.fire.bindings]\nwheel = [\"x\"]\n").unwrap_err();
    assert!(e.message.contains("unknown device"));
    let e = ActionMap::parse("input.toml", "[actions.1up]\n").unwrap_err();
    assert!(e.message.contains("not start with a digit"));
    // No map file: None. No slots: one keyboard slot.
    let t = temp("input-nomap");
    assert!(ActionMap::load(&t).unwrap().is_none());
    let m = ActionMap::parse("input.toml", "[actions.jump]\n").unwrap();
    assert_eq!(m.slots[0].index, 1);
    assert_eq!(m.action("jump").unwrap().kind, ActionKind::Button);
    let j = m.to_json();
    // After the engine's device action, by name.
    assert_eq!(j["actions"][1]["name"], "jump");
}

#[test]
fn injected_input_reaches_the_slot_entity_with_edges() {
    let (mut w, mut s, state) = rig(false);
    let p1 = w.entity("@p1").unwrap();
    let p2 = w.entity("@p2").unwrap();
    // Hold move right for 3 frames on slot 1, tap fire on slot 2.
    let r = state
        .borrow_mut()
        .inject(s.tick(), 1, "move", Some(&json!([1, 0])), Some(3), false)
        .unwrap();
    assert_eq!((r.tick, r.until), (0, Some(3)));
    state
        .borrow_mut()
        .inject(s.tick(), 2, "fire", None, None, false)
        .unwrap();

    s.step(&mut w, 1).unwrap();
    assert_eq!(
        w.get_field(p1, "Actions.move").unwrap(),
        Value::Vec2(Vec2::new(1.0, 0.0))
    );
    assert_eq!(
        w.get_field(p2, "Actions.move").unwrap(),
        Value::Vec2(Vec2::ZERO),
        "slots are separate"
    );
    assert_eq!(w.get_field(p2, "Actions.fire").unwrap(), Value::Bool(true));
    assert_eq!(
        w.get_field(p2, "Actions.fire_pressed").unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        w.get_field(p1, "Actions.fire_pressed").unwrap(),
        Value::Bool(false)
    );

    s.step(&mut w, 1).unwrap();
    assert_eq!(
        w.get_field(p1, "Actions.move").unwrap(),
        Value::Vec2(Vec2::new(1.0, 0.0))
    );
    assert_eq!(
        w.get_field(p2, "Actions.fire").unwrap(),
        Value::Bool(false),
        "a tap is one tick"
    );
    assert_eq!(
        w.get_field(p2, "Actions.fire_pressed").unwrap(),
        Value::Bool(false)
    );
    assert_eq!(
        w.get_field(p2, "Actions.fire_released").unwrap(),
        Value::Bool(true)
    );

    s.step(&mut w, 2).unwrap();
    assert_eq!(
        w.get_field(p1, "Actions.move").unwrap(),
        Value::Vec2(Vec2::ZERO),
        "released after 3 frames"
    );

    // The log is the change points, by tick.
    let log: Vec<String> = state.borrow().log().iter().map(LogEntry::line).collect();
    assert_eq!(
        log,
        [
            "0\t1\tmove\t[1.0,0.0]",
            "0\t2\tfire\ttrue",
            "1\t2\tfire\tfalse",
            "3\t1\tmove\t[0.0,0.0]"
        ]
    );

    // Errors: unknown action, wrong value shape, bad slot.
    let mut st = state.borrow_mut();
    assert_eq!(
        st.inject(4, 1, "jump", None, None, false).unwrap_err().code,
        codes::UNKNOWN_ACTION
    );
    let e = st
        .inject(4, 1, "move", Some(&json!(true)), None, false)
        .unwrap_err();
    assert_eq!(e.code, codes::BAD_VALUE);
    assert!(e.message.contains("takes [x, y]"), "{e}");
    assert_eq!(
        st.inject(4, 1, "throttle", Some(&json!(2.0)), None, false)
            .unwrap_err()
            .code,
        codes::BAD_VALUE
    );
    assert_eq!(
        st.inject(4, 3, "fire", None, None, false).unwrap_err().code,
        codes::BAD_SLOT
    );
}

#[test]
fn toggles_holds_and_scripts() {
    let (mut w, mut s, state) = rig(false);
    let p1 = w.entity("@p1").unwrap();
    // Toggle: a press flips, the automatic release does nothing.
    state
        .borrow_mut()
        .inject(s.tick(), 1, "crouch", None, None, false)
        .unwrap();
    s.step(&mut w, 3).unwrap();
    assert_eq!(
        w.get_field(p1, "Actions.crouch").unwrap(),
        Value::Bool(true),
        "still crouching"
    );
    state
        .borrow_mut()
        .inject(s.tick(), 1, "crouch", None, None, false)
        .unwrap();
    s.step(&mut w, 1).unwrap();
    assert_eq!(
        w.get_field(p1, "Actions.crouch").unwrap(),
        Value::Bool(false)
    );
    // Hold until told otherwise.
    let r = state
        .borrow_mut()
        .inject(s.tick(), 1, "throttle", Some(&json!(0.5)), None, true)
        .unwrap();
    assert_eq!(r.until, None);
    s.step(&mut w, 10).unwrap();
    assert_eq!(
        w.get_field(p1, "Actions.throttle").unwrap(),
        Value::Float(0.5)
    );
    state
        .borrow_mut()
        .inject(s.tick(), 1, "throttle", Some(&json!(0)), None, false)
        .unwrap();
    s.step(&mut w, 1).unwrap();
    assert_eq!(
        w.get_field(p1, "Actions.throttle").unwrap(),
        Value::Float(0.0)
    );
    // A script: relative ticks.
    let now = s.tick();
    let reports = state
        .borrow_mut()
        .script(
            now,
            &[
                json!({ "at": 0, "action": "move", "value": [0, 1], "frames": 2 }),
                json!({ "at": 5, "action": "fire" }),
            ],
        )
        .unwrap();
    assert_eq!(reports[1].tick, now + 5);
    s.step(&mut w, 6).unwrap();
    assert_eq!(w.get_field(p1, "Actions.fire").unwrap(), Value::Bool(true));
    assert_eq!(
        w.get_field(p1, "Actions.move").unwrap(),
        Value::Vec2(Vec2::ZERO)
    );
    assert_eq!(state.borrow().state_json(1)["actions"]["fire"], true);
}

#[test]
fn discrete_runs_take_commands_by_turn() {
    let (mut w, mut s, state) = rig(true);
    let p1 = w.entity("@p1").unwrap();
    assert!(state.borrow().is_discrete());
    // frames is ignored: a command lasts one turn.
    state
        .borrow_mut()
        .inject(s.tick(), 1, "move", Some(&json!([0, -1])), Some(99), false)
        .unwrap();
    s.advance(&mut w, 1).unwrap();
    assert_eq!(
        w.get_field(p1, "Actions.move").unwrap(),
        Value::Vec2(Vec2::new(0.0, -1.0))
    );
    s.advance(&mut w, 1).unwrap();
    assert_eq!(
        w.get_field(p1, "Actions.move").unwrap(),
        Value::Vec2(Vec2::ZERO)
    );
    let log: Vec<String> = state.borrow().log().iter().map(LogEntry::line).collect();
    assert_eq!(log, ["0\t1\tmove\t[0.0,-1.0]", "1\t1\tmove\t[0.0,0.0]"]);
}

#[test]
fn a_recorded_log_replays_to_the_same_run() {
    let dir = temp("input-replay");
    let path = dir.join("runs").join("one").join("input.log");

    let (mut w, mut s, state) = rig(false);
    state.borrow_mut().start_recording(&path).unwrap();
    assert_eq!(state.borrow().recording(), Some(path.as_path()));
    state
        .borrow_mut()
        .inject(0, 1, "move", Some(&json!([1, 0])), Some(4), false)
        .unwrap();
    state
        .borrow_mut()
        .inject(2, 2, "fire", None, None, false)
        .unwrap();
    state
        .borrow_mut()
        .inject(3, 1, "crouch", None, None, false)
        .unwrap();
    s.step(&mut w, 8).unwrap();
    let stopped = state.borrow_mut().stop_recording().unwrap();
    assert_eq!(stopped, path);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("# argus input log"), "{text}");
    assert_eq!(text.lines().count(), 1 + 5);

    // Replay into a fresh rig: the per-tick Actions are identical.
    let entries = InputState::read_log(&path, &map()).unwrap();
    assert_eq!(entries.len(), 5);
    let (mut w2, mut s2, state2) = rig(false);
    assert_eq!(state2.borrow_mut().replay(0, &entries).unwrap(), 5);
    let p1a = w.entity("@p1").unwrap();
    let p1b = w2.entity("@p1").unwrap();
    let mut a_trace = Vec::new();
    let mut b_trace = Vec::new();
    let (mut wa, mut sa, sta) = rig(false);
    sta.borrow_mut().replay(0, &entries).unwrap();
    for _ in 0..8 {
        sa.step(&mut wa, 1).unwrap();
        s2.step(&mut w2, 1).unwrap();
        a_trace.push(
            wa.get_component(
                wa.entity("@p1").unwrap(),
                wa.component_id("Actions").unwrap(),
            )
            .unwrap(),
        );
        b_trace.push(
            w2.get_component(p1b, w2.component_id("Actions").unwrap())
                .unwrap(),
        );
    }
    assert_eq!(a_trace, b_trace);
    assert_eq!(
        state2
            .borrow()
            .log()
            .iter()
            .map(LogEntry::line)
            .collect::<Vec<_>>(),
        state
            .borrow()
            .log()
            .iter()
            .map(LogEntry::line)
            .collect::<Vec<_>>()
    );
    let _ = p1a;
    let e = LogEntry::parse("x\t1\tfire\ttrue", &map()).unwrap_err();
    assert_eq!(e.code, codes::BAD_LOG);
}

// ----- phase 1 item 2: real devices through the same schedule --------------

use argus_input::{DeviceInput, Devices};

fn key(name: &str, pressed: bool) -> DeviceInput {
    DeviceInput::Key {
        name: name.to_string(),
        pressed,
    }
}

#[test]
fn keys_and_pads_resolve_through_the_bindings_per_slot() {
    let m = map();
    let mut d = Devices::default();
    // winit spellings and binding spellings are the same key.
    d.apply(&key("KeyW", true));
    d.apply(&key("KeyD", true));
    d.apply(&key("Space", true));
    let v: std::collections::BTreeMap<_, _> = d.resolve(&m, 1).into_iter().collect();
    let (x, y) = match v["move"] {
        argus_input::ActionValue::Axis2(x, y) => (x, y),
        other => panic!("{other:?}"),
    };
    assert!(
        (x - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3
            && (y - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3,
        "diagonals are unit length"
    );
    assert_eq!(v["fire"], argus_input::ActionValue::Button(true));
    assert!(
        !v.contains_key("crouch"),
        "an action no device binds is left to injects"
    );
    // Slot 2 lists only the second pad: the keyboard does not reach it.
    assert!(d.resolve(&m, 2).is_empty(), "no second pad connected yet");

    // Two pads are two players (section 12): pad index is connection order.
    for (pad, name) in [(10, "first"), (11, "second")] {
        d.apply(&DeviceInput::PadConnected {
            pad,
            name: name.into(),
            connected: true,
        });
    }
    d.apply(&DeviceInput::PadAxis {
        pad: 11,
        axis: "left_x".into(),
        value: -1.0,
    });
    d.apply(&DeviceInput::PadAxis {
        pad: 11,
        axis: "left_y".into(),
        value: 0.1,
    }); // dead zone
    d.apply(&DeviceInput::PadButton {
        pad: 11,
        button: "south".into(),
        pressed: true,
    });
    let v2: std::collections::BTreeMap<_, _> = d.resolve(&m, 2).into_iter().collect();
    assert_eq!(v2["move"], argus_input::ActionValue::Axis2(-1.0, 0.0));
    assert_eq!(v2["fire"], argus_input::ActionValue::Button(true));
    // The first pad's stick is idle, so slot 1 keeps the keyboard's larger vector.
    let v1: std::collections::BTreeMap<_, _> = d.resolve(&m, 1).into_iter().collect();
    assert!(matches!(v1["move"], argus_input::ActionValue::Axis2(x, _) if x > 0.7));

    d.apply(&DeviceInput::ReleaseAll);
    let v1: std::collections::BTreeMap<_, _> = d.resolve(&m, 1).into_iter().collect();
    assert_eq!(
        v1["fire"],
        argus_input::ActionValue::Button(false),
        "focus loss releases"
    );
}

#[test]
fn device_changes_are_logged_as_actions_and_replay_without_devices() {
    let (mut w, mut s, st) = rig(false);
    let mut d = Devices::default();
    let p1 = w.entity("@p1").unwrap();
    // Tick 0 with nothing held; then space and d go down before tick 1.
    s.step(&mut w, 1).unwrap();
    d.apply(&key("Space", true));
    d.apply(&key("d", true));
    assert_eq!(st.borrow_mut().apply_devices(s.tick(), &d), 0);
    s.step(&mut w, 1).unwrap();
    assert_eq!(w.get_field(p1, "Actions.fire").unwrap(), Value::Bool(true));
    assert_eq!(
        w.get_field(p1, "Actions.fire_pressed").unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        w.get_field(p1, "Actions.move").unwrap(),
        Value::Vec2(Vec2::new(1.0, 0.0))
    );
    // Held: nothing new is scheduled, the value stays, the edge is gone.
    st.borrow_mut().apply_devices(s.tick(), &d);
    s.step(&mut w, 1).unwrap();
    assert_eq!(w.get_field(p1, "Actions.fire").unwrap(), Value::Bool(true));
    assert_eq!(
        w.get_field(p1, "Actions.fire_pressed").unwrap(),
        Value::Bool(false)
    );
    d.apply(&key("Space", false));
    st.borrow_mut().apply_devices(s.tick(), &d);
    s.step(&mut w, 1).unwrap();
    assert_eq!(
        w.get_field(p1, "Actions.fire_released").unwrap(),
        Value::Bool(true)
    );

    // The log is actions by tick, never keys.
    let log: Vec<String> = st.borrow().log().iter().map(|e| e.line()).collect();
    assert_eq!(log.len(), 3, "{log:?}");
    assert!(log
        .iter()
        .all(|l| !l.contains("space") && !l.contains("KeyD")));
    assert!(log[0].starts_with("1\t1\t"), "{log:?}");

    // A fresh run replays the log with no devices at all and ends the same.
    let entries = st.borrow().log().to_vec();
    let (mut w2, mut s2, st2) = rig(false);
    st2.borrow_mut().replay(0, &entries).unwrap();
    s2.step(&mut w2, 4).unwrap();
    let q1 = w2.entity("@p1").unwrap();
    for f in ["Actions.fire", "Actions.move"] {
        assert_eq!(
            w2.get_field(q1, f).unwrap(),
            w.get_field(p1, f).unwrap(),
            "{f}"
        );
    }
}

#[test]
fn in_a_discrete_scene_a_press_is_one_command_for_the_next_turn() {
    let (mut w, mut s, st) = rig(true);
    let mut d = Devices::default();
    let p1 = w.entity("@p1").unwrap();
    d.apply(&key("ArrowUp", true)); // not bound: wasd only
    d.apply(&key("w", true));
    let turns = st.borrow_mut().apply_devices(s.tick(), &d);
    assert_eq!(turns, 1, "one command, so the caller advances one turn");
    s.advance(&mut w, turns as u64).unwrap();
    assert_eq!(
        w.get_field(p1, "Actions.move").unwrap(),
        Value::Vec2(Vec2::new(0.0, 1.0))
    );
    // Still held: no second command, and the move is back at rest next turn.
    assert_eq!(st.borrow_mut().apply_devices(s.tick(), &d), 0);
    s.advance(&mut w, 1).unwrap();
    assert_eq!(
        w.get_field(p1, "Actions.move").unwrap(),
        Value::Vec2(Vec2::new(0.0, 0.0))
    );
    // Release then press again: a second command.
    d.apply(&key("w", false));
    assert_eq!(st.borrow_mut().apply_devices(s.tick(), &d), 0);
    d.apply(&key("w", true));
    assert_eq!(st.borrow_mut().apply_devices(s.tick(), &d), 1);
}

#[test]
fn unknown_gamepad_and_mouse_names_are_refused_at_load() {
    let e = ActionMap::parse(
        "input.toml",
        "[actions.jump]\ntype = \"button\"\n[actions.jump.bindings]\ngamepad = [\"souht\"]\n",
    )
    .unwrap_err();
    assert!(e.message.contains("unknown gamepad input \"souht\""), "{e}");
    assert!(e.message.contains("south"));
    assert!(ActionMap::parse(
        "input.toml",
        "[actions.jump]\ntype = \"button\"\n[actions.jump.bindings]\nkeyboard = [\"anything\"]\nmouse = [\"left\"]\n",
    )
    .is_ok());
}

/// Found building Hollow for the phase-1 gate: a newer inject of the same
/// action replaces an older one, so the older one's release, still to
/// come, no longer cuts the newer one short. A script's later steps, and
/// holds that have not started, keep their own.
#[test]
fn a_newer_inject_is_not_cut_short_by_an_older_ones_release() {
    let (mut w, mut s, st) = rig(false);
    let move_of = |w: &World| match w
        .get_field(w.entity("@p1").unwrap(), "Actions.move")
        .unwrap()
    {
        Value::Vec2(v) => v,
        other => panic!("{other:?}"),
    };
    st.borrow_mut()
        .inject(0, 1, "move", Some(&json!([1, 0])), Some(10), false)
        .unwrap();
    s.step(&mut w, 5).unwrap();
    // Before the first lets go at 10, a longer one takes over.
    st.borrow_mut()
        .inject(5, 1, "move", Some(&json!([1, 0])), Some(100), false)
        .unwrap();
    s.step(&mut w, 20).unwrap();
    assert_eq!(
        move_of(&w),
        Vec2::new(1.0, 0.0),
        "still held at 25, past the first's release"
    );
    s.step(&mut w, 90).unwrap();
    assert_eq!(
        move_of(&w),
        Vec2::new(0.0, 0.0),
        "the second lets go at 105"
    );
    // Out of order in a script: a later press keeps its release.
    let r = st
        .borrow_mut()
        .script(
            115,
            &[
                json!({ "at": 30, "action": "fire", "frames": 5 }),
                json!({ "at": 0, "action": "fire", "frames": 5 }),
            ],
        )
        .unwrap();
    assert_eq!((r[0].until, r[1].until), (Some(150), Some(120)));
    s.step(&mut w, 60).unwrap();
    let fire = w
        .get_field(w.entity("@p1").unwrap(), "Actions.fire")
        .unwrap();
    assert_eq!(fire, Value::Bool(false), "the press at 145 let go at 150");
}

// ----- phase 2 item 0: the journal, for seek ---------------------------------

/// Run ticks `from..to` and write down every slot-1 field each tick.
fn run_ticks(st: &mut InputState, from: u64, to: u64) -> Vec<(u64, Vec<(String, Value)>)> {
    (from..to)
        .map(|t| {
            st.begin_tick(t);
            (t, st.fields_for(1))
        })
        .collect()
}

/// Found planning phase 2: the value log cannot rebuild the input state
/// between keyframes (a toggle's recorded release flips it again; an
/// inject's own release is scheduled, not logged, until it happens). The
/// commands are journaled, and a restore re-issues them as their ticks come.
#[test]
fn a_restore_replays_the_journaled_commands_exactly() {
    let mut st = InputState::new(map(), false);
    st.inject(0, 1, "move", Some(&json!([1, 0])), Some(30), false)
        .unwrap();
    let mut first = run_ticks(&mut st, 0, 10);
    let key = st.snapshot(); // next tick to run: 10
                             // After the keyframe: a toggle pressed and pressed again, a script,
                             // and an inject that replaces the held move, whose own release (at 30)
                             // must not cut the new one short.
    st.inject(10, 1, "crouch", None, None, false).unwrap();
    first.extend(run_ticks(&mut st, 10, 14));
    st.script(
        14,
        &[
            json!({"at": 0, "action": "fire"}),
            json!({"at": 3, "action": "throttle", "value": 0.5, "frames": 4}),
        ],
    )
    .unwrap();
    st.inject(14, 1, "crouch", None, None, false).unwrap();
    first.extend(run_ticks(&mut st, 14, 25));
    st.inject(25, 1, "move", Some(&json!([0, 1])), Some(20), false)
        .unwrap();
    first.extend(run_ticks(&mut st, 25, 50));
    let log_len = st.log().len();

    st.restore(&key);
    assert_eq!(st.future().len(), 4, "the commands after the keyframe wait");
    let again = run_ticks(&mut st, 10, 50);
    assert_eq!(again, first[10..].to_vec(), "the same values, tick by tick");
    assert!(st.future().is_empty());
    assert_eq!(st.log().len(), log_len, "replayed ticks do not log twice");
    assert_eq!(st.take_branch(), None, "replaying is not branching");
}

#[test]
fn a_live_command_after_a_restore_branches_and_drops_the_recorded_future() {
    let mut st = InputState::new(map(), false);
    run_ticks(&mut st, 0, 5);
    let key = st.snapshot();
    st.inject(5, 1, "fire", None, Some(10), false).unwrap();
    st.inject(20, 1, "throttle", Some(&json!(1.0)), Some(5), false)
        .unwrap();
    run_ticks(&mut st, 5, 30);
    assert_eq!(st.journal().len(), 2);

    st.restore(&key);
    run_ticks(&mut st, 5, 12); // the fire inject came in again at 5
    assert_eq!(st.future().len(), 1, "the throttle at 20 still waits");
    st.inject(12, 1, "move", Some(&json!([-1, 0])), Some(3), false)
        .unwrap();
    assert_eq!(st.take_branch(), Some(12));
    assert!(st.future().is_empty(), "the throttle never happens now");
    assert_eq!(st.journal().len(), 2, "fire, then the new move");
    assert!(
        st.log().iter().all(|e| e.tick < 12),
        "the log after the branch is gone: {:?}",
        st.log()
    );
    let after = run_ticks(&mut st, 12, 30);
    let throttle = |fields: &Vec<(String, Value)>| {
        fields
            .iter()
            .find(|(k, _)| k == "throttle")
            .map(|(_, v)| v.clone())
    };
    assert!(after
        .iter()
        .all(|(_, f)| throttle(f) == Some(Value::Float(0.0))));
}

#[test]
fn a_restore_can_move_forward_along_the_recorded_run() {
    let mut st = InputState::new(map(), false);
    let k0 = st.snapshot();
    st.inject(0, 1, "fire", None, Some(3), false).unwrap();
    run_ticks(&mut st, 0, 10);
    st.inject(10, 1, "crouch", None, None, false).unwrap();
    run_ticks(&mut st, 10, 20);
    let k20 = st.snapshot();
    let at20 = st.fields_for(1);
    st.restore(&k0);
    assert_eq!(st.future().len(), 2);
    st.restore(&k20);
    assert!(st.future().is_empty(), "past both commands");
    assert_eq!(st.fields_for(1), at20);
}

// ----- host channel item 0: messages from the host ----------------------------

/// Run ticks `from..to` and write down the inbox each tick: (tick, channel, text).
fn run_inbox(st: &mut InputState, from: u64, to: u64) -> Vec<(u64, String, String)> {
    let mut out = Vec::new();
    for t in from..to {
        st.begin_tick(t);
        for (_, c, x) in st.inbox() {
            out.push((t, c.clone(), x.clone()));
        }
    }
    out
}

#[test]
fn a_message_reaches_the_tick_it_is_for_and_only_that_tick() {
    let mut st = InputState::new(map(), true);
    run_inbox(&mut st, 0, 3);
    st.message(3, 1, "gm.ruling", "{\"intent\": \"tie the door\"}")
        .unwrap();
    st.message(3, 1, "gm.note", "second").unwrap();
    let got = run_inbox(&mut st, 3, 6);
    assert_eq!(
        got,
        vec![
            (
                3,
                "gm.ruling".into(),
                "{\"intent\": \"tie the door\"}".into()
            ),
            (3, "gm.note".into(), "second".into()),
        ],
        "both at tick 3, in the order sent, and gone after"
    );
    assert_eq!(st.journal().len(), 2, "journaled like an inject");
}

#[test]
fn a_restore_brings_the_messages_in_again_including_one_pending_at_the_keyframe() {
    let mut st = InputState::new(map(), true);
    run_inbox(&mut st, 0, 4);
    // Sent before the keyframe, for the tick after it: the keyframe must carry it.
    st.message(4, 1, "gm.ruling", "pending").unwrap();
    let key = st.snapshot();
    let mut first = run_inbox(&mut st, 4, 6);
    st.message(6, 1, "gm.ruling", "after").unwrap();
    st.inject(6, 1, "fire", None, None, false).unwrap();
    first.extend(run_inbox(&mut st, 6, 10));
    assert_eq!(first.len(), 2);

    st.restore(&key);
    let again = run_inbox(&mut st, 4, 10);
    assert_eq!(again, first, "the same messages at the same ticks");
    assert!(st.future().is_empty());
    assert_eq!(st.take_branch(), None, "replaying is not branching");

    // A live message after a restore branches the run, as an inject does.
    st.restore(&key);
    run_inbox(&mut st, 4, 5);
    st.message(5, 1, "gm.ruling", "another answer").unwrap();
    assert_eq!(st.take_branch(), Some(5));
    let branched = run_inbox(&mut st, 5, 10);
    assert_eq!(
        branched,
        vec![(5, "gm.ruling".into(), "another answer".into())]
    );
}

#[test]
fn messages_survive_a_keyframe_and_a_journal_on_disk() {
    use argus_ecs::codec::{Reader, Writer};
    let mut st = InputState::new(map(), true);
    run_inbox(&mut st, 0, 2);
    let text = "a ruling\nwith lines, \"quotes\" and {braces}, and \u{e9}";
    st.message(2, 1, "gm.ruling", text).unwrap();
    let key = st.snapshot();

    let mut w = Writer::new();
    key.encode(&mut w, 0);
    for e in st.journal() {
        e.encode(&mut w);
    }
    let bytes = w.into_bytes();
    let mut r = Reader::new(&bytes, "test");
    let back = argus_input::InputSnapshot::decode_as(&mut r, true, true).unwrap();
    let entry = argus_input::JournalEntry::decode(&mut r).unwrap();
    assert_eq!(&entry, &st.journal()[0]);

    let mut fresh = InputState::new(map(), true);
    fresh.load_journal(Vec::new(), &back);
    assert_eq!(
        run_inbox(&mut fresh, 2, 3),
        vec![(2, "gm.ruling".into(), text.into())]
    );
}

#[test]
fn a_bad_channel_or_a_text_too_long_is_refused() {
    let mut st = InputState::new(map(), true);
    for bad in ["", "has space", "slash/no", &"x".repeat(65)] {
        let e = st.message(0, 1, bad, "hi").unwrap_err();
        assert!(e.message.contains("channel"), "{bad:?}: {}", e.message);
    }
    let long = "x".repeat(argus_input::MAX_MESSAGE + 1);
    let e = st.message(0, 1, "gm.ruling", &long).unwrap_err();
    assert!(e.message.contains("too long"), "{}", e.message);
    assert!(st.journal().is_empty(), "nothing refused is journaled");
}

#[test]
fn a_message_arrives_as_a_host_message_event_for_the_scripts() {
    let (mut w, mut s, state) = rig(true);
    state
        .borrow_mut()
        .message(s.tick(), 1, "gm.ruling", "{\"ok\": true}")
        .unwrap();
    s.step(&mut w, 1).unwrap();
    let got: Vec<_> = w
        .events()
        .iter()
        .filter_map(|r| match &r.event {
            argus_ecs::Event::Game { name, fields } if name == "host.message" => {
                Some(fields.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(got.len(), 1);
    let field = |k: &str| got[0].iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    assert_eq!(field("channel"), Some(Value::Str("gm.ruling".into())));
    assert_eq!(field("text"), Some(Value::Str("{\"ok\": true}".into())));
    assert_eq!(field("slot"), Some(Value::UInt(1)));
}
