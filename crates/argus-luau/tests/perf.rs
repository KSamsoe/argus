//! Release-only numbers for the script path. §17 gives Luau one target the
//! binding owns (hot reload < 100 ms, asserted in `limits_reload.rs`); the
//! per-entity throughput below is measured so the §19 risk ("Luau
//! performance ceiling") has a number next to it.
//! `cargo test -p argus-luau --release --test perf -- --ignored --nocapture`

use argus_ecs::World;
use argus_luau::{ScriptConfig, ScriptHost};
use argus_sim::{Scheduler, SchedulerConfig};
use std::time::Instant;

#[test]
#[ignore = "release-only perf probe; run with --release -- --ignored"]
fn perf_script_system_over_50k_entities() {
    let mut h = ScriptHost::new(ScriptConfig::default()).unwrap();
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    assert!(h.load_source(
        &mut w,
        &mut s,
        "components/c.luau",
        "component \"Pos\" { x = field.f32 {}, y = field.f32 {} }\ncomponent \"Vel\" { x = field.f32 { default = 1 }, y = field.f32 { default = 0.5 } }",
    ));
    assert!(h.load_source(
        &mut w,
        &mut s,
        "systems/move.luau",
        "return system \"sim.move\" { stage = \"sim\", reads = {\"Vel\"}, writes = {\"Pos\"}, run = function(q)\n for e, v, p in q:each(\"Vel\", \"Pos\") do p.x += v.x; p.y += v.y end end }",
    ), "{:?}", h.errors());
    let pos = w.component_id("Pos").unwrap();
    let vel = w.component_id("Vel").unwrap();
    for _ in 0..50_000 {
        let e = w.spawn();
        w.insert_default(e, pos).unwrap();
        w.insert_default(e, vel).unwrap();
    }
    s.step(&mut w, 2).unwrap();
    let t = Instant::now();
    s.step(&mut w, 10).unwrap();
    let per_tick = t.elapsed() / 10;
    eprintln!("Luau per-entity system over 50k entities (4 field ops each): {per_tick:?} per tick");
    assert!(w
        .events()
        .iter()
        .all(|r| !matches!(r.event, argus_ecs::Event::SystemFault { .. })));
    // Generous ceiling: this is the path the spec says to move to Rust when hot.
    assert!(per_tick.as_millis() < 500, "{per_tick:?}");
}
