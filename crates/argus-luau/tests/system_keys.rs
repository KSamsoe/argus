//! A system declaration's keys (found at the phase 5 gate): a key the
//! declaration does not know is an error with the file and line, not a
//! no-op. A trial agent wrote `before = {...}` and a misspelt key, both
//! loaded silently, and the system ran wherever the scheduler put it.

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
fn an_unknown_key_in_a_system_is_an_error_and_before_says_what_to_do() {
    let ok = "return system \"sim.x\" {\n  stage = \"sim\", after = {}, reads = {}, writes = {},\n  rng = \"x\", doc = \"d\", on_load = false, unfrozen = false,\n  run = function(q) end,\n}";
    assert_eq!(error_for(ok), None, "every key a system takes");

    let before = "return system \"sim.x\" {\n  stage = \"sim\",\n  before = { \"physics.move\" },\n  run = function(q) end,\n}";
    let e = error_for(before).expect("before is refused");
    assert!(e.contains("unknown key before"), "{e}");

    let typo = "return system \"sim.x\" {\n  stage = \"sim\",\n  writse = { \"Transform\" },\n  run = function(q) end,\n}";
    let e = error_for(typo).expect("a misspelt key is refused");
    assert!(e.contains("unknown key writse"), "{e}");
}
