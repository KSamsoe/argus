//! §7: a script has no clock and no unseeded randomness, so a run, its
//! replay, and a seek back give the same results. Luau's standard library
//! came with `os` (time, date, clock) and `math.random`, which scripts could
//! read; the manual's writers found it. `os` is gone, and `math.random`
//! fails with what to use instead.

use argus_ecs::{Event, Value, World};
use argus_luau::{ScriptConfig, ScriptHost};
use argus_sim::{Scheduler, SchedulerConfig};

const PROBE: &str = r#"
component "Probe" {
  has_os = field.bool { default = true },
  random_ok = field.bool { default = true },
  math_ok = field.bool { default = false },
}
"#;

const LOOK: &str = r#"
return system "sim.look" {
  stage = "sim",
  reads = { "Probe" },
  writes = { "Probe" },
  run = function(q)
    local p = q:entity("@probe"):get("Probe")
    p.has_os = os ~= nil
    p.random_ok = pcall(math.random)
    -- The rest of math is still there.
    p.math_ok = math.floor(2.5) == 2 and math.abs(-1) == 1
  end,
}
"#;

const ROLL: &str = r#"
return system "sim.roll" {
  stage = "sim",
  reads = { "Probe" },
  writes = { "Probe" },
  run = function(q)
    local n = math.random(1, 6)
  end,
}
"#;

#[test]
fn a_script_has_no_clock_and_no_unseeded_random() {
    let mut h = ScriptHost::new(ScriptConfig::default()).unwrap();
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0).seed(1));
    assert!(
        h.load_source(&mut w, &mut s, "components/probe.luau", PROBE),
        "{:?}",
        h.errors()
    );
    assert!(
        h.load_source(&mut w, &mut s, "systems/look.luau", LOOK),
        "{:?}",
        h.errors()
    );
    let e = w.spawn_authored("probe").unwrap();
    w.insert_by_name(e, "Probe").unwrap();
    s.step(&mut w, 1).unwrap();
    assert_eq!(
        w.get_field(e, "Probe.has_os").unwrap(),
        Value::Bool(false),
        "os is gone"
    );
    assert_eq!(
        w.get_field(e, "Probe.random_ok").unwrap(),
        Value::Bool(false),
        "math.random fails"
    );
    assert_eq!(
        w.get_field(e, "Probe.math_ok").unwrap(),
        Value::Bool(true),
        "the rest of math works"
    );
}

#[test]
fn math_random_says_to_use_the_systems_stream() {
    let mut h = ScriptHost::new(ScriptConfig::default()).unwrap();
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0).seed(1));
    assert!(
        h.load_source(&mut w, &mut s, "components/probe.luau", PROBE),
        "{:?}",
        h.errors()
    );
    assert!(
        h.load_source(&mut w, &mut s, "systems/roll.luau", ROLL),
        "{:?}",
        h.errors()
    );
    let e = w.spawn_authored("probe").unwrap();
    w.insert_by_name(e, "Probe").unwrap();
    s.step(&mut w, 1).unwrap();
    let fault = w
        .events()
        .iter()
        .find_map(|r| match &r.event {
            Event::SystemFault {
                system, message, ..
            } if system == "sim.roll" => Some(message.clone()),
            _ => None,
        })
        .expect("sim.roll faults");
    assert!(
        fault.contains("q:rng()") && fault.contains("replay"),
        "{fault}"
    );
}

/// q:within and q:nearest take an entity as the point, as the reference
/// says (the manual's writers found they refused one).
#[test]
fn a_spatial_query_can_search_from_an_entity() {
    let mut h = ScriptHost::new(ScriptConfig::default()).unwrap();
    let mut w = World::new();
    argus_ecs::hierarchy::register(&mut w).unwrap();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0).seed(1));
    let comps = r#"
component "Seeker" { found = field.u32 { default = 0 }, near = field.f32 { default = -1 } }
component "Crumb" {}
"#;
    let sys = r#"
return system "sim.seek" {
  stage = "sim",
  reads = { "Seeker", "Crumb", "Transform" },
  writes = { "Seeker" },
  run = function(q)
    local me = q:entity("@seeker")
    local s = me:get("Seeker")
    s.found = 0
    for _ in q:within(me, 3, "Crumb") do s.found += 1 end
    local _, d = q:nearest(me, 10, "Crumb")
    s.near = d or -1
  end,
}
"#;
    assert!(
        h.load_source(&mut w, &mut s, "components/c.luau", comps),
        "{:?}",
        h.errors()
    );
    assert!(
        h.load_source(&mut w, &mut s, "systems/seek.luau", sys),
        "{:?}",
        h.errors()
    );
    let me = w.spawn_authored("seeker").unwrap();
    w.insert_by_name(me, "Seeker").unwrap();
    w.insert_by_name(me, "Transform").unwrap();
    w.set_field(me, "Transform.pos", argus_ecs::Vec2 { x: 5.0, y: 5.0 })
        .unwrap();
    for (x, y) in [(6.0, 5.0), (5.0, 7.0), (20.0, 20.0)] {
        let c = w.spawn();
        w.insert_by_name(c, "Crumb").unwrap();
        w.insert_by_name(c, "Transform").unwrap();
        w.set_field(c, "Transform.pos", argus_ecs::Vec2 { x, y })
            .unwrap();
    }
    w.rebuild_broadphase();
    s.step(&mut w, 1).unwrap();
    assert_eq!(
        w.get_field(me, "Seeker.found").unwrap(),
        Value::UInt(2),
        "two crumbs within 3"
    );
    assert_eq!(
        w.get_field(me, "Seeker.near").unwrap(),
        Value::Float(1.0),
        "the nearest is 1 away"
    );
}
