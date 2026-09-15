//! Action maps, injected input, edges, toggles, the log, and replay (§6, §12).

use argus_ecs::{Value, Vec2, World};
use argus_input::{codes, install, ActionKind, ActionMap, InputState, LogEntry, PlayerSlot};
use argus_sim::{Scheduler, SchedulerConfig};
use serde_json::json;
use std::cell::RefCell;
use std::rc::Rc;

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
    assert_eq!(names, ["crouch", "fire", "move", "throttle"]);
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
    assert!(e.hint.contains("crouch, fire, move, throttle"));
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
    let t = std::env::temp_dir().join("argus-input-nomap");
    let _ = std::fs::create_dir_all(&t);
    assert!(ActionMap::load(&t).unwrap().is_none());
    let m = ActionMap::parse("input.toml", "[actions.jump]\n").unwrap();
    assert_eq!(m.slots[0].index, 1);
    assert_eq!(m.action("jump").unwrap().kind, ActionKind::Button);
    let j = m.to_json();
    assert_eq!(j["actions"][0]["name"], "jump");
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
    let dir = std::env::temp_dir().join(format!("argus-input-replay-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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
    assert_eq!(state2.borrow_mut().replay(&entries).unwrap(), 5);
    let p1a = w.entity("@p1").unwrap();
    let p1b = w2.entity("@p1").unwrap();
    let mut a_trace = Vec::new();
    let mut b_trace = Vec::new();
    let (mut wa, mut sa, sta) = rig(false);
    sta.borrow_mut().replay(&entries).unwrap();
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
    let _ = std::fs::remove_dir_all(&dir);
}
