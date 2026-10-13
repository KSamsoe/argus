//! §2 instruction budgets, §7 hot reload ("recompiles the changed files,
//! rebinds system functions, and continues from the current frame"),
//! §17 Luau hot reload < 100 ms.

use argus_ecs::{Event, Value, World};
use argus_luau::{codes, ScriptConfig, ScriptHost};
use argus_sim::{Scheduler, SchedulerConfig};
use std::path::{Path, PathBuf};
use std::time::Instant;

fn faults(w: &World) -> Vec<(String, String)> {
    w.events()
        .iter()
        .filter_map(|r| match &r.event {
            Event::SystemFault {
                system, message, ..
            } => Some((system.clone(), message.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn an_unbounded_loop_hits_the_budget_and_is_disabled() {
    let mut h = ScriptHost::new(ScriptConfig {
        interrupt_budget: 10_000,
        ..Default::default()
    })
    .unwrap();
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::default());
    assert!(h.load_source(
        &mut w,
        &mut s,
        "components/c.luau",
        "component \"N\" { v = field.i32 {} }"
    ));
    let looping = "return system \"sim.forever\" {\n stage = \"sim\",\n run = function(q)\n  while true do end\n end }";
    assert!(
        h.load_source(&mut w, &mut s, "systems/forever.luau", looping),
        "{:?}",
        h.errors()
    );
    let good = "return system \"sim.fine\" { stage = \"sim\", writes = {\"N\"}, run = function(q) for e, n in q:each(\"N\") do n.v += 1 end end }";
    assert!(
        h.load_source(&mut w, &mut s, "systems/fine.luau", good),
        "{:?}",
        h.errors()
    );
    let e = w.spawn();
    w.insert_by_name(e, "N").unwrap();

    let t = Instant::now();
    s.step(&mut w, 3).unwrap();
    assert!(
        t.elapsed().as_secs() < 5,
        "budget stopped the loop promptly"
    );
    let f = faults(&w);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].0, "sim.forever");
    assert!(f[0].1.contains("instruction budget"), "{}", f[0].1);
    assert!(f[0].1.contains(&codes::BUDGET_EXCEEDED.to_string()));
    assert!(!s.is_enabled("sim.forever").unwrap());
    assert_eq!(
        w.get_field(e, "N.v").unwrap(),
        Value::Int(3),
        "the other system ran every tick"
    );
}

/// Issue 13: running out of the budget inside a `pcall` still stops the run
/// (the next check outside it runs out again), as the scripting chapter
/// says; and each script system's checks are counted, last run and most,
/// for the profile.
#[test]
fn pcall_cannot_catch_the_budget_and_each_run_s_checks_are_counted() {
    let mut h = ScriptHost::new(ScriptConfig {
        interrupt_budget: 10_000,
        ..Default::default()
    })
    .unwrap();
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::default());
    let caught = "return system \"sim.caught\" {\n stage = \"sim\",\n run = function(q)\n  local ok = pcall(function() while true do end end)\n  q:emit(\"after\", { ok = ok })\n end }";
    assert!(
        h.load_source(&mut w, &mut s, "systems/caught.luau", caught),
        "{:?}",
        h.errors()
    );
    let busy = "return system \"sim.busy\" {\n stage = \"sim\",\n run = function(q)\n  local n = 0\n  for i = 1, 100 * (q.tick + 1) do n += i end\n end }";
    assert!(
        h.load_source(&mut w, &mut s, "systems/busy.luau", busy),
        "{:?}",
        h.errors()
    );
    s.step(&mut w, 2).unwrap();
    let f = faults(&w);
    assert_eq!(f.len(), 1, "{f:?}");
    assert_eq!(f[0].0, "sim.caught");
    assert!(f[0].1.contains("instruction budget"), "{}", f[0].1);
    assert!(
        !w.events()
            .iter()
            .any(|r| matches!(&r.event, Event::Game { name, .. } if name == "after")),
        "the run stopped at the pcall"
    );
    let (last, most) = h.checks("sim.busy").expect("counted");
    assert!((200..10_000).contains(&last), "{last}");
    assert_eq!(most, last, "the second run did the most");
    assert_eq!(
        h.checks("sim.caught").map(|c| c.0),
        Some(10_000),
        "all of it"
    );
    assert_eq!(h.checks("no.such"), None);
}

#[test]
fn memory_limit_is_enforced() {
    let mut h = ScriptHost::new(ScriptConfig {
        memory_limit: 2 << 20,
        ..Default::default()
    })
    .unwrap();
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::default());
    let hog = "return system \"sim.hog\" { stage = \"sim\", run = function(q)\n local t = {}\n for i = 1, 10000000 do t[i] = tostring(i) end\n end }";
    assert!(
        h.load_source(&mut w, &mut s, "systems/hog.luau", hog),
        "{:?}",
        h.errors()
    );
    s.step(&mut w, 1).unwrap();
    let f = faults(&w);
    assert_eq!(f.len(), 1);
    assert!(f[0].1.contains("memory"), "{}", f[0].1);
    assert!(h.memory_used() < (4 << 20));
}

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

fn project(name: &str) -> Temp {
    let root = std::env::temp_dir().join(format!("argus-luau-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("components")).unwrap();
    std::fs::create_dir_all(root.join("systems")).unwrap();
    Temp(root)
}

#[test]
fn reload_swaps_code_and_keeps_world_state_and_rng_streams() {
    let root = project("reload");
    std::fs::write(
        root.join("components/hp.luau"),
        "return component \"Hp\" { cur = field.i32 { default = 0 } }",
    )
    .unwrap();
    let v1 = "return system \"sim.grow\" { stage = \"sim\", writes = {\"Hp\"}, rng = \"grow\", run = function(q)\n for e, hp in q:each(\"Hp\") do hp.cur += 1 + q:rng():range(0, 0) end end }";
    std::fs::write(root.join("systems/grow.luau"), v1).unwrap();

    let mut h = ScriptHost::new(ScriptConfig::default()).unwrap();
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0).seed(5));
    assert!(
        h.load_project(&mut w, &mut s, &root).unwrap(),
        "{:?}",
        h.errors()
    );
    let e = w.spawn_authored("p").unwrap();
    w.insert_by_name(e, "Hp").unwrap();
    s.step(&mut w, 5).unwrap();
    assert_eq!(w.get_field(e, "Hp.cur").unwrap(), Value::Int(5));
    let rng_before = s.state().rng.clone();

    // Unchanged project: nothing reloads.
    let r = h.reload(&mut w, &mut s).unwrap();
    assert!(r.reloaded.is_empty() && r.removed.is_empty());

    // Change the system body: +10 per tick. Same stream name keeps its state.
    let v2 = v1.replace("hp.cur += 1", "hp.cur += 10");
    std::fs::write(root.join("systems/grow.luau"), v2).unwrap();
    let t = Instant::now();
    let r = h.reload(&mut w, &mut s).unwrap();
    let took = t.elapsed();
    assert_eq!(r.reloaded, vec!["systems/grow.luau"]);
    assert_eq!(r.errors, 0);
    assert!(took.as_millis() < 100, "reload took {took:?}");
    assert_eq!(
        w.get_field(e, "Hp.cur").unwrap(),
        Value::Int(5),
        "world state untouched by reload"
    );
    assert_eq!(
        s.state().rng,
        rng_before,
        "stream state carried across reload"
    );
    assert_eq!(s.tick(), 5, "continues from the current frame");
    s.step(&mut w, 1).unwrap();
    assert_eq!(w.get_field(e, "Hp.cur").unwrap(), Value::Int(15));
    assert!(faults(&w).is_empty());

    // A file with a syntax error: the error is reported, the old system stays disabled? No:
    // the previous binding is kept because the file failed to produce a new declaration.
    std::fs::write(
        root.join("systems/grow.luau"),
        "return system \"sim.grow\" {\n stage = \n",
    )
    .unwrap();
    let r = h.reload(&mut w, &mut s).unwrap();
    assert_eq!(r.errors, 1);
    let err = h.errors()[0].clone();
    assert_eq!(err.file, "systems/grow.luau");
    assert_eq!(err.line, Some(3));

    // A faulted system comes back on reload.
    std::fs::write(
        root.join("systems/grow.luau"),
        "return system \"sim.grow\" { stage = \"sim\", run = function(q) error(\"boom\") end }",
    )
    .unwrap();
    h.reload(&mut w, &mut s).unwrap();
    s.step(&mut w, 1).unwrap();
    assert!(!s.is_enabled("sim.grow").unwrap());
    std::fs::write(root.join("systems/grow.luau"), v1).unwrap();
    let r = h.reload(&mut w, &mut s).unwrap();
    assert_eq!(r.errors, 0);
    assert!(s.is_enabled("sim.grow").unwrap(), "replace re-enables");
    s.step(&mut w, 1).unwrap();
    assert_eq!(w.get_field(e, "Hp.cur").unwrap(), Value::Int(16));

    // Deleting the file removes its system.
    std::fs::remove_file(root.join("systems/grow.luau")).unwrap();
    let r = h.reload(&mut w, &mut s).unwrap();
    assert_eq!(r.removed, vec!["systems/grow.luau"]);
    assert!(s.is_enabled("sim.grow").is_err());
    s.step(&mut w, 1).unwrap();
    assert_eq!(w.get_field(e, "Hp.cur").unwrap(), Value::Int(16));
}

#[test]
fn reload_of_a_changed_component_shape_is_a_reported_conflict() {
    let root = project("shape");
    std::fs::write(
        root.join("components/hp.luau"),
        "return component \"Hp\" { cur = field.i32 {} }",
    )
    .unwrap();
    let mut h = ScriptHost::new(ScriptConfig::default()).unwrap();
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::default());
    assert!(h.load_project(&mut w, &mut s, &root).unwrap());
    std::fs::write(
        root.join("components/hp.luau"),
        "return component \"Hp\" { cur = field.f32 {}, max = field.f32 {} }",
    )
    .unwrap();
    let r = h.reload(&mut w, &mut s).unwrap();
    assert_eq!(r.errors, 1);
    let e = h.errors()[0].clone();
    assert!(e.message.contains("different schema"), "{}", e.message);
    assert!(
        e.message.contains("migration"),
        "hint names the fix: {}",
        e.message
    );
    // The registered schema is unchanged, so running code keeps working.
    assert_eq!(w.describe("Hp").unwrap().fields.len(), 1);
}
