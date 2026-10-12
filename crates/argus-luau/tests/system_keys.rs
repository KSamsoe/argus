//! A system declaration's keys (found at the phase 5 gate): a key the
//! declaration does not know is an error with the file and line, not a
//! no-op. A trial agent wrote `before = {...}` and a misspelt key, both
//! loaded silently, and the system ran wherever the scheduler put it; that
//! `before` was refused, and since the art passes needed one (a look
//! system's burst before `particles.update`) it is a key of its own.

use argus_ecs::World;
use argus_luau::{ScriptConfig, ScriptHost};
use argus_sim::{Scheduler, SchedulerConfig};

fn error_for(src: &str) -> Option<String> {
    let mut h = ScriptHost::new(ScriptConfig::default()).unwrap();
    let (mut w, mut s) = (World::new(), Scheduler::new(SchedulerConfig::default()));
    if h.load_source(&mut w, &mut s, "systems/x.luau", src) {
        return None;
    }
    h.errors()
        .first()
        .map(|e| format!("{} | {:?}", e.message, e.line))
}

#[test]
fn an_unknown_key_in_a_system_is_an_error() {
    let ok = "return system \"sim.x\" {\n  stage = \"sim\", after = {}, before = {}, reads = {}, writes = {},\n  rng = \"x\", doc = \"d\", on_load = false, unfrozen = false,\n  run = function(q) end,\n}";
    assert_eq!(error_for(ok), None, "every key a system takes");

    let typo = "return system \"sim.x\" {\n  stage = \"sim\",\n  writse = { \"Transform\" },\n  run = function(q) end,\n}";
    let e = error_for(typo).expect("a misspelt key is refused");
    assert!(e.contains("unknown key writse"), "{e}");
}

/// A script system runs before the system its `before` names, though that
/// one was registered first (an engine system, which no script can name
/// itself in).
#[test]
fn a_system_runs_before_the_system_it_names() {
    let mut h = ScriptHost::new(ScriptConfig::default()).unwrap();
    let (mut w, mut s) = (World::new(), Scheduler::new(SchedulerConfig::default()));
    let src = |name: &str, extra: &str| {
        format!(
            "return system \"{name}\" {{\n  stage = \"late\",{extra}\n  run = function(q) end,\n}}"
        )
    };
    assert!(h.load_source(&mut w, &mut s, "systems/a.luau", &src("late.engine", "")));
    assert!(h.load_source(&mut w, &mut s, "systems/b.luau", &src("late.other", "")));
    assert!(h.load_source(
        &mut w,
        &mut s,
        "systems/c.luau",
        &src("late.look", "\n  before = { \"late.engine\" },")
    ));
    let order: Vec<String> = s
        .systems(w.registry())
        .unwrap()
        .into_iter()
        .map(|i| i.name)
        .collect();
    // Only look-before-engine is promised; other was registered before
    // look, and nothing orders the two.
    assert_eq!(order, ["late.other", "late.look", "late.engine"]);
    let look = s
        .systems(w.registry())
        .unwrap()
        .into_iter()
        .find(|i| i.name == "late.look")
        .unwrap();
    assert_eq!(look.before, ["late.engine"]);
}
