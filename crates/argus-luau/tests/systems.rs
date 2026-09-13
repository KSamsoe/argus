//! §3 script systems: `q:each` over declared components, field access with
//! range checks, declared access enforced at run time, spawn/despawn/emit/rng
//! from script, and the "no references across frames" rule (§7).

use argus_ecs::{Event, Value, World};
use argus_luau::{codes, ScriptConfig, ScriptHost};
use argus_sim::{Scheduler, SchedulerConfig, Stage, SystemDesc};

fn host(seed: u64) -> (ScriptHost, World, Scheduler) {
    (
        ScriptHost::new(ScriptConfig::default()).unwrap(),
        World::new(),
        Scheduler::new(SchedulerConfig::continuous(60.0).seed(seed)),
    )
}

const COMPONENTS: &str = r#"
component "HitStop" {
  frames = field.u8 { default = 0, range = {0, 30} },
  scale  = field.f32{ default = 0.0, range = {0, 1}, tune = true },
}
component "TimeScale" { value = field.f32 { default = 1.0 } }
component "Health" { cur = field.i32 { default = 10 }, max = field.i32 { default = 10 } }
component "Enemy" {}
component "Target" { who = field.entity {} }
"#;

const HITSTOP_SYSTEM: &str = r#"
return system "sim.hitstop" {
  stage  = "sim",
  reads  = { "HitStop" },
  writes = { "HitStop", "TimeScale" },
  run = function(q)
    for e, hs, ts in q:each("HitStop", "TimeScale") do
      if hs.frames > 0 then
        hs.frames -= 1
        ts.value = hs.scale
      else
        ts.value = 1
      end
    end
  end,
}
"#;

fn fault_messages(w: &World) -> Vec<(String, String, Option<String>)> {
    w.events()
        .iter()
        .filter_map(|r| match &r.event {
            Event::SystemFault {
                system,
                message,
                r#ref,
            } => Some((system.clone(), message.clone(), r#ref.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn the_spec_example_system_runs() {
    let (mut h, mut w, mut s) = host(1);
    assert!(
        h.load_source(&mut w, &mut s, "components/all.luau", COMPONENTS),
        "{:?}",
        h.errors()
    );
    assert!(
        h.load_source(&mut w, &mut s, "systems/hitstop.luau", HITSTOP_SYSTEM),
        "{:?}",
        h.errors()
    );
    let e = w.spawn_authored("player").unwrap();
    w.insert_by_name(e, "HitStop").unwrap();
    w.insert_by_name(e, "TimeScale").unwrap();
    w.set_field(e, "HitStop.frames", 2i64).unwrap();
    w.set_field(e, "HitStop.scale", 0.25).unwrap();

    s.step(&mut w, 1).unwrap();
    assert_eq!(w.get_field(e, "HitStop.frames").unwrap(), Value::UInt(1));
    assert_eq!(
        w.get_field(e, "TimeScale.value").unwrap(),
        Value::Float(0.25)
    );
    s.step(&mut w, 1).unwrap();
    assert_eq!(w.get_field(e, "HitStop.frames").unwrap(), Value::UInt(0));
    s.step(&mut w, 1).unwrap();
    assert_eq!(
        w.get_field(e, "TimeScale.value").unwrap(),
        Value::Float(1.0)
    );
    assert!(fault_messages(&w).is_empty());
    let info = s.systems(w.registry()).unwrap();
    let hs = info.iter().find(|i| i.name == "sim.hitstop").unwrap();
    assert_eq!(hs.reads, vec!["HitStop"]);
    assert_eq!(hs.writes, vec!["HitStop", "TimeScale"]);
    assert_eq!(hs.source.as_deref(), Some("systems/hitstop.luau:2"));
    assert_eq!(hs.stage, Stage::Sim);
}

#[test]
fn range_violations_from_script_carry_the_schema_ref() {
    let (mut h, mut w, mut s) = host(1);
    h.load_source(&mut w, &mut s, "components/all.luau", COMPONENTS);
    let sys = r#"
return system "sim.bad" {
  stage = "sim", writes = {"HitStop"},
  run = function(q)
    for e, hs in q:each("HitStop") do hs.scale = 1.5 end
  end,
}"#;
    assert!(
        h.load_source(&mut w, &mut s, "systems/bad.luau", sys),
        "{:?}",
        h.errors()
    );
    let e = w.spawn();
    w.insert_by_name(e, "HitStop").unwrap();
    s.step(&mut w, 1).unwrap();
    let faults = fault_messages(&w);
    assert_eq!(faults.len(), 1);
    let (system, message, r#ref) = &faults[0];
    assert_eq!(system, "sim.bad");
    assert!(
        message.contains("HitStop.scale must be in range [0, 1]; got 1.5"),
        "{message}"
    );
    assert_eq!(
        r#ref.as_deref(),
        Some("components/all.luau:2"),
        "ref is the schema line"
    );
    assert!(!s.is_enabled("sim.bad").unwrap());
    assert_eq!(
        w.get_field(e, "HitStop.scale").unwrap(),
        Value::Float(0.0),
        "rejected write left the value"
    );
}

#[test]
fn undeclared_access_is_an_error_at_run_time() {
    let (mut h, mut w, mut s) = host(1);
    h.load_source(&mut w, &mut s, "components/all.luau", COMPONENTS);
    // Reads HitStop only; tries to write it, and to read TimeScale.
    let sys = r#"
return system "sim.sneaky" {
  stage = "sim", reads = {"HitStop"},
  run = function(q)
    local ok, err = pcall(function()
      for e, hs in q:each("HitStop") do hs.frames = 3 end
    end)
    assert(not ok, "write should fail")
    assert(string.find(tostring(err), "writes HitStop.frames without declaring"), tostring(err))
    local ok2, err2 = pcall(function() for e, ts in q:each("TimeScale") do end end)
    assert(not ok2)
    assert(string.find(tostring(err2), "reads TimeScale without declaring"), tostring(err2))
    -- A read-only handle can still read.
    for e, hs in q:each("HitStop") do assert(hs.frames == 0) end
  end,
}"#;
    assert!(
        h.load_source(&mut w, &mut s, "systems/sneaky.luau", sys),
        "{:?}",
        h.errors()
    );
    let e = w.spawn();
    w.insert_by_name(e, "HitStop").unwrap();
    w.insert_by_name(e, "TimeScale").unwrap();
    s.step(&mut w, 1).unwrap();
    assert!(fault_messages(&w).is_empty(), "{:?}", fault_messages(&w));
    assert_eq!(w.get_field(e, "HitStop.frames").unwrap(), Value::UInt(0));
}

#[test]
fn spawn_despawn_insert_emit_entities_and_rng() {
    let (mut h, mut w, mut s) = host(7);
    h.load_source(&mut w, &mut s, "components/all.luau", COMPONENTS);
    let sys = r#"
return system "sim.spawner" {
  stage = "sim", reads = {"Health"}, writes = {"Health", "Enemy", "Target"}, rng = "spawn",
  run = function(q)
    if q.tick == 0 then
      local player = q:entity("@player")
      assert(player:name() == "player")
      assert(tostring(player) == "e:0")
      for i = 1, 3 do
        local e = q:spawn()
        e:insert("Enemy")
        e:insert("Health", { cur = 2 + i, max = 5 })
        e:insert("Target", { who = player })
        assert(e:has("Enemy") and e:get("Health").cur == 2 + i)
        assert(e:get("Target").who == player)
      end
      assert(q:count("Enemy") == 3, q:count("Enemy"))
      assert(q:count("Health") == 4)
      q:emit("wave.spawned", { count = 3, note = "first" })
    else
      local rng = q:rng()
      for e, hp in q:each("Health", "Enemy") do
        local roll = rng:range(1, 6)
        assert(roll >= 1 and roll <= 6)
        hp.cur -= 1
        if hp.cur <= 0 then
          e:despawn()
          q:emit("enemy.died", { entity = e })
        end
      end
      assert(rng:next() < 1 and rng:below(4) < 4)
      local _ = rng:chance(0.5)
    end
    assert(q.dt > 0 and q.time >= 0 and q.name == "sim.spawner")
  end,
}"#;
    assert!(
        h.load_source(&mut w, &mut s, "systems/spawner.luau", sys),
        "{:?}",
        h.errors()
    );
    let p = w.spawn_authored("player").unwrap();
    w.insert_by_name(p, "Health").unwrap();
    s.step(&mut w, 1).unwrap();
    assert!(fault_messages(&w).is_empty(), "{:?}", fault_messages(&w));
    assert_eq!(w.query().has(w.component_id("Enemy").unwrap()).count(), 3);
    let spawned = w
        .events()
        .iter()
        .find(|r| matches!(&r.event, Event::Game { name, .. } if name == "wave.spawned"));
    match &spawned.unwrap().event {
        Event::Game { fields, .. } => {
            assert_eq!(
                fields,
                &vec![
                    ("count".to_string(), Value::Int(3)),
                    ("note".to_string(), Value::Str("first".into()))
                ]
            );
        }
        _ => unreachable!(),
    }
    // Ticks 1..: enemies lose 1 hp per tick; the weakest (3 hp) dies on tick 3.
    s.step(&mut w, 3).unwrap();
    assert!(fault_messages(&w).is_empty(), "{:?}", fault_messages(&w));
    assert_eq!(w.query().has(w.component_id("Enemy").unwrap()).count(), 2);
    let died = w
        .events()
        .iter()
        .filter(|r| matches!(&r.event, Event::Game { name, .. } if name == "enemy.died"))
        .count();
    assert_eq!(died, 1);
    s.step(&mut w, 2).unwrap();
    assert_eq!(w.query().has(w.component_id("Enemy").unwrap()).count(), 0);
    assert!(fault_messages(&w).is_empty());
}

#[test]
fn script_systems_are_deterministic_under_the_same_seed() {
    let sys = r#"
return system "sim.roll" {
  stage = "sim", writes = {"Health"}, rng = "dice",
  run = function(q)
    for e, hp in q:each("Health") do hp.cur += q:rng():range(0, 3) end
  end,
}"#;
    let run = |seed: u64| {
        let (mut h, mut w, mut s) = host(seed);
        h.load_source(&mut w, &mut s, "components/all.luau", COMPONENTS);
        assert!(
            h.load_source(&mut w, &mut s, "systems/roll.luau", sys),
            "{:?}",
            h.errors()
        );
        let e = w.spawn();
        w.insert_by_name(e, "Health").unwrap();
        s.step(&mut w, 50).unwrap();
        w.get_field(e, "Health.cur").unwrap()
    };
    assert_eq!(run(3), run(3));
    assert_ne!(run(3), run(4));
}

#[test]
fn handles_held_across_frames_fail_clearly() {
    let (mut h, mut w, mut s) = host(1);
    h.load_source(&mut w, &mut s, "components/all.luau", COMPONENTS);
    let sys = r#"
local stash = nil
return system "sim.hoarder" {
  stage = "sim", writes = {"Health"},
  run = function(q)
    if stash == nil then
      for e, hp in q:each("Health") do stash = hp end
    else
      stash.cur = 99   -- previous frame's handle
    end
  end,
}"#;
    assert!(
        h.load_source(&mut w, &mut s, "systems/hoarder.luau", sys),
        "{:?}",
        h.errors()
    );
    let e = w.spawn();
    w.insert_by_name(e, "Health").unwrap();
    s.step(&mut w, 2).unwrap();
    let faults = fault_messages(&w);
    assert_eq!(faults.len(), 1);
    assert!(
        faults[0].1.contains("outside its system's run"),
        "{}",
        faults[0].1
    );
    assert!(faults[0].1.contains(&codes::STALE_HANDLE.to_string()));
    assert_eq!(
        faults[0].2.as_deref(),
        Some("systems/hoarder.luau:9"),
        "ref is the script line"
    );
    assert_eq!(w.get_field(e, "Health.cur").unwrap(), Value::Int(10));
}

#[test]
fn runtime_errors_point_at_the_script_line_and_disable_the_system() {
    let (mut h, mut w, mut s) = host(1);
    let sys = "return system \"sim.oops\" {\n  stage = \"sim\",\n  run = function(q)\n    local t = nil\n    return t.field\n  end,\n}";
    assert!(
        h.load_source(&mut w, &mut s, "systems/oops.luau", sys),
        "{:?}",
        h.errors()
    );
    s.step(&mut w, 3).unwrap();
    let faults = fault_messages(&w);
    assert_eq!(faults.len(), 1);
    assert!(
        faults[0].1.contains("attempt to index nil"),
        "{}",
        faults[0].1
    );
    assert_eq!(faults[0].2.as_deref(), Some("systems/oops.luau:5"));
    assert!(!s.is_enabled("sim.oops").unwrap());
}

#[test]
fn rust_and_script_systems_share_one_schedule_and_one_contract() {
    let (mut h, mut w, mut s) = host(1);
    h.load_source(&mut w, &mut s, "components/all.luau", COMPONENTS);
    let health = w.component_id("Health").unwrap();
    s.add_system(
        SystemDesc::new("sim.damage", Stage::Sim).writes(health),
        Box::new(|ctx| {
            let ids = ctx
                .query()
                .has(ctx.world().component_id("Health").unwrap())
                .entities();
            for e in ids {
                ctx.set_field(e, "Health.cur", 1i64)?;
            }
            Ok(())
        }),
    )
    .unwrap();
    let sys = r#"
return system "sim.after" {
  stage = "sim", after = {"sim.damage"}, writes = {"Health"},
  run = function(q) for e, hp in q:each("Health") do hp.cur += 100 end end,
}"#;
    assert!(
        h.load_source(&mut w, &mut s, "systems/after.luau", sys),
        "{:?}",
        h.errors()
    );
    let e = w.spawn();
    w.insert_by_name(e, "Health").unwrap();
    s.step(&mut w, 1).unwrap();
    assert_eq!(w.get_field(e, "Health.cur").unwrap(), Value::Int(101));
    assert_eq!(
        s.who_writes(health).unwrap(),
        vec!["sim.damage", "sim.after"]
    );
}
