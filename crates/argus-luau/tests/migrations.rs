//! Save migrations declared in Luau (§6, phase 3 item 0): a component's
//! `migrate` is a table of steps by version (rename, default, drop, or a
//! function) or one function for any older version, and the script host
//! runs it on every row a world saved at an older version holds.

use argus_ecs::{Entity, Value, World};
use argus_luau::{ScriptConfig, ScriptHost};
use argus_sim::{Scheduler, SchedulerConfig};

fn host(src: &str) -> (ScriptHost, World) {
    let mut h = ScriptHost::new(ScriptConfig::default()).unwrap();
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::default());
    assert!(
        h.load_source(&mut w, &mut s, "components/needs.luau", src),
        "{:?}",
        h.errors()
    );
    (h, w)
}

const V1: &str = r#"
return component "Needs" {
  food = field.u8 { default = 50 },
  mood = field.str {},
  home = field.entity {},
}
"#;

/// Version 3, two steps from version 1: food renamed hunger and mood
/// dropped (a table), then rest worked out from hunger (a function).
const V3: &str = r#"
return component "Needs" {
  version = 3,
  hunger = field.f32 {},
  rest = field.f32 {},
  shift = field.u8 {},
  home = field.entity {},
  migrate = {
    [2] = { rename = { food = "hunger" }, drop = { "mood" }, default = { shift = 2 } },
    [3] = function(row)
      row.hunger = row.hunger / 100
      row.rest = 1 - row.hunger
      return row
    end,
  },
}
"#;

fn saved_v1() -> (Vec<u8>, Vec<Entity>, Entity) {
    let (_, mut w) = host(V1);
    let camp = w.spawn();
    let mut who = Vec::new();
    for i in 0..4u64 {
        let c = w.spawn();
        w.insert_by_name(c, "Needs").unwrap();
        w.set_field(c, "Needs.food", Value::UInt(i * 25)).unwrap();
        w.set_field(c, "Needs.mood", "fine").unwrap();
        w.set_field(c, "Needs.home", camp).unwrap();
        who.push(c);
    }
    (w.encode_state(), who, camp)
}

#[test]
fn steps_by_version_bring_a_row_up_from_any_older_one() {
    let (bytes, who, camp) = saved_v1();
    let (mut h, mut w) = host(V3);
    assert!(h.has_migration("Needs"));
    let report = w.decode_state_with(&bytes, Some(&mut h)).unwrap();
    let m = &report.migrated[0];
    assert_eq!(
        (m.component.as_str(), m.from, m.to, m.rows, m.declared),
        ("Needs", 1, 3, 4, true)
    );
    assert!(m.left.is_empty(), "{:?}", m.left);
    for (i, c) in who.iter().enumerate() {
        let hunger = i as f64 * 0.25;
        assert_eq!(
            w.get_field(*c, "Needs.hunger").unwrap(),
            Value::Float(hunger)
        );
        assert_eq!(
            w.get_field(*c, "Needs.rest").unwrap(),
            Value::Float(1.0 - hunger)
        );
        assert_eq!(w.get_field(*c, "Needs.shift").unwrap(), Value::UInt(2));
        // An entity reference passes through a function untouched.
        assert_eq!(w.get_field(*c, "Needs.home").unwrap(), Value::Entity(camp));
    }
}

#[test]
fn one_function_takes_the_row_and_the_version_it_was_saved_at() {
    let (bytes, who, _) = saved_v1();
    let (mut h, mut w) = host(
        r#"
return component "Needs" {
  version = 2,
  hunger = field.f32 {},
  note = field.str {},
  home = field.entity {},
  migrate = function(row, from)
    return { hunger = row.food / 100, note = row.mood .. " (from v" .. from .. ")", home = row.home }
  end,
}
"#,
    );
    let report = w.decode_state_with(&bytes, Some(&mut h)).unwrap();
    assert_eq!(report.migrated[0].rows, 4);
    assert_eq!(
        w.get_field(who[2], "Needs.note").unwrap(),
        Value::Str("fine (from v1)".into())
    );
    assert_eq!(
        w.get_field(who[2], "Needs.hunger").unwrap(),
        Value::Float(0.5)
    );
}

#[test]
fn a_migration_is_checked_where_it_is_declared_and_where_it_runs() {
    // A step past the component's version is a declaration error.
    let mut h = ScriptHost::new(ScriptConfig::default()).unwrap();
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::default());
    assert!(!h.load_source(
        &mut w,
        &mut s,
        "components/needs.luau",
        "return component \"Needs\" { version = 2, hunger = field.f32 {}, migrate = { [3] = { drop = { \"x\" } } } }",
    ));
    let e = &h.errors()[0];
    assert!(e.message.contains("steps are 2 to 2"), "{}", e.message);

    // A function that fails names the component, the step, and the line.
    let (bytes, _, _) = saved_v1();
    let (mut h, mut w) = host(
        r#"
return component "Needs" {
  version = 2,
  hunger = field.f32 {},
  migrate = { [2] = function(row) error("no food field") end },
}
"#,
    );
    let e = w.decode_state_with(&bytes, Some(&mut h)).unwrap_err();
    assert!(
        e.message.starts_with("Needs v1→v2 on e:1: step 2:"),
        "{}",
        e.message
    );
    assert!(e.message.contains("no food field"), "{}", e.message);
    assert_eq!(e.r#ref.as_deref(), Some("components/needs.luau:5"));
}
