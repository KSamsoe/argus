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

/// An engine component (declared in Rust) with a range.
#[derive(argus_ecs::Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(doc = "A gauge")]
struct Gauge {
    #[argus(doc = "Its level", range(0, 1))]
    level: f32,
}

impl Default for Gauge {
    fn default() -> Self {
        Gauge { level: 0.0 }
    }
}

#[test]
fn range_violations_on_an_engine_component_point_at_the_script_line() {
    // The schema's source is Rust nobody edits; the script's line is the
    // one to read (found building Outpost: JobKind.work, phase 3 item 3).
    let (mut h, mut w, mut s) = host(1);
    w.register::<Gauge>().unwrap();
    let sys = r#"
return system "sim.over" {
  stage = "sim", writes = {"Gauge"},
  run = function(q)
    for e, g in q:each("Gauge") do g.level = 2 end
  end,
}"#;
    assert!(
        h.load_source(&mut w, &mut s, "systems/over.luau", sys),
        "{:?}",
        h.errors()
    );
    let e = w.spawn();
    w.insert(e, Gauge::default()).unwrap();
    s.step(&mut w, 1).unwrap();
    let faults = fault_messages(&w);
    assert_eq!(faults.len(), 1);
    assert!(
        faults[0].1.contains("Gauge.level must be in range"),
        "{}",
        faults[0].1
    );
    assert_eq!(faults[0].2.as_deref(), Some("systems/over.luau:5"));
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

/// What the scripting chapter's sandbox section says a script has (issue
/// 9: it named the libraries and not the base functions, which work), and
/// what it lacks: `require` says what to do instead.
#[test]
fn the_sandbox_has_what_the_manual_lists() {
    let (mut h, mut w, mut s) = host(7);
    let sys = r#"
return system "sim.sandbox" {
  stage = "sim",
  run = function(q)
    for _, n in ipairs({ "assert", "error", "pcall", "xpcall", "select", "type", "typeof",
        "tostring", "tonumber", "ipairs", "pairs", "next", "unpack", "rawget", "rawset",
        "rawequal", "rawlen", "setmetatable", "getmetatable", "print" }) do
      assert(type(_G[n]) == "function", n)
    end
    for _, n in ipairs({ "math", "string", "table", "vector", "coroutine", "bit32", "utf8", "buffer" }) do
      assert(type(_G[n]) == "table", n)
    end
    for _, n in ipairs({ "os", "io", "load", "loadstring", "dofile" }) do
      assert(_G[n] == nil, n)
    end
    local ok, err = pcall(require, "./helper")
    assert(not ok and string.find(tostring(err), "read no other files"), tostring(err))
  end,
}"#;
    assert!(
        h.load_source(&mut w, &mut s, "systems/sandbox.luau", sys),
        "{:?}",
        h.errors()
    );
    s.step(&mut w, 1).unwrap();
    assert!(fault_messages(&w).is_empty(), "{:?}", fault_messages(&w));
}

/// Issue 9: `q:spawn("id")` gives the id, a taken id is an error that
/// names it, and `q:find` asks without one.
#[test]
fn find_returns_nil_where_entity_errs_and_spawn_by_id_names_a_taken_one() {
    let (mut h, mut w, mut s) = host(7);
    h.load_source(&mut w, &mut s, "components/all.luau", COMPONENTS);
    let sys = r#"
return system "sim.finder" {
  stage = "sim", writes = {"Enemy"},
  run = function(q)
    assert(q:find("@boss") == nil, "none yet")
    local boss = q:spawn("boss")
    assert(q:find("@boss") == boss and q:entity("@boss") == boss)
    local ok, err = pcall(q.spawn, q, "boss")
    assert(not ok and string.find(tostring(err), "already used by"), tostring(err))
    local gone = q:spawn()
    local ref = tostring(gone)
    gone:despawn()
    assert(q:find(ref) == nil, "a dead ref is nil")
    local bad = pcall(q.find, q, "e:nope")
    assert(not bad, "a ref that is not one is still an error")
  end,
}"#;
    assert!(
        h.load_source(&mut w, &mut s, "systems/finder.luau", sys),
        "{:?}",
        h.errors()
    );
    s.step(&mut w, 1).unwrap();
    assert!(fault_messages(&w).is_empty(), "{:?}", fault_messages(&w));
    assert!(w.find("@boss").unwrap().is_some());
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

// ----- prof.alloc sees script allocations ---------------------------------

#[global_allocator]
static ALLOC: argus_sim::alloc::Counting = argus_sim::alloc::Counting;

#[test]
fn a_script_system_that_builds_a_table_per_entity_is_visible_in_prof_alloc() {
    let (mut h, mut w, mut s) = host(1);
    assert!(h.load_source(&mut w, &mut s, "components/all.luau", COMPONENTS));
    let lean = r#"
return system "sim.lean" {
  stage = "sim", reads = {"Health"},
  run = function(q)
    local total = 0
    for e, hp in q:each("Health") do total += hp.cur end
  end,
}"#;
    let greedy = r#"
return system "sim.greedy" {
  stage = "sim", reads = {"Health"}, after = {"sim.lean"},
  run = function(q)
    local keep = {}
    for e, hp in q:each("Health") do
      keep[#keep + 1] = { cur = hp.cur, max = hp.max, who = e, label = "hp " .. hp.cur }
    end
  end,
}"#;
    assert!(
        h.load_source(&mut w, &mut s, "systems/lean.luau", lean),
        "{:?}",
        h.errors()
    );
    assert!(
        h.load_source(&mut w, &mut s, "systems/greedy.luau", greedy),
        "{:?}",
        h.errors()
    );
    for _ in 0..1000 {
        let e = w.spawn();
        w.insert_by_name(e, "Health").unwrap();
    }
    s.step(&mut w, 10).unwrap();
    assert!(fault_messages(&w).is_empty(), "{:?}", fault_messages(&w));

    let rows = s.prof_alloc(600).unwrap();
    let lean = rows.iter().find(|r| r.name == "sim.lean").unwrap();
    let greedy = rows.iter().find(|r| r.name == "sim.greedy").unwrap();
    assert_eq!(greedy.samples, 10);
    // A table with four fields per entity, a thousand entities: tens of
    // kilobytes a tick, whatever Luau's page allocator batches it into.
    assert!(greedy.mean_bytes >= 32_000.0, "greedy {greedy:?}");
    assert!(
        greedy.mean_bytes > lean.mean_bytes * 2.0,
        "the table builder stands out: greedy {greedy:?} lean {lean:?}"
    );
}

// ----- phase 1 item 0: vectors, batch spawn, the broadphase ------------------

#[test]
fn vec2_fields_are_native_vectors_and_spawn_many_and_within_reach_the_broadphase() {
    let (mut h, mut w, mut s) = host(1);
    w.register::<argus_ecs::Transform>().unwrap();
    assert!(h.load_source(&mut w, &mut s, "components/all.luau", COMPONENTS));
    let sys = r#"
component "Pos" { hits = field.u32 { default = 0 } }
return system "sim.vec" {
  stage = "sim", reads = {"Pos", "Health", "Transform"}, writes = {"Pos", "Health", "Transform"},
  run = function(q)
    if q.tick == 0 then
      -- One call spawns them all, straight into their archetype.
      local n = q:spawn_many(100, { "Pos", "Health", "Transform" }, function(e, i)
        e:get("Transform").pos = vector.create(i, 0, 0)     -- a vector write
        e:get("Health").cur = i
      end)
      assert(n == 100)
      local one = q:spawn({ "Pos", "Transform" })
      one:get("Transform").pos = { x = 1000, y = 0 }         -- a table write still works
      return
    end
    -- Reads are vectors: fields, arithmetic, magnitude, no tables.
    for e, tr in q:each("Transform") do
      local p = tr.pos
      assert(type(p) == "vector", type(p))
      tr.pos = p + vector.create(0, 1, 0)
    end
    -- The broadphase was rebuilt at the end of tick 0: within and nearest see tick-0 positions.
    local count = 0
    for e, pos, hp, d in q:within(vector.create(10, 0, 0), 2.5, "Pos", "Health") do
      count += 1
      pos.hits += 1
      assert(d <= 2.5)
    end
    assert(count == 5, "within found " .. count)
    local near, d = q:nearest(vector.create(10.4, 0, 0), 5, "Pos", "Health")
    assert(near ~= nil and near:get("Health").cur == 10, tostring(near))
    assert(math.abs(d - 0.4) < 1e-3, tostring(d))
    local none = q:nearest({ x = 5000, y = 0 }, 1, "Pos")
    assert(none == nil)
  end,
}"#;
    assert!(
        h.load_source(&mut w, &mut s, "systems/vec.luau", sys),
        "{:?}",
        h.errors()
    );
    s.step(&mut w, 2).unwrap();
    assert!(fault_messages(&w).is_empty(), "{:?}", fault_messages(&w));
    let pos = w.component_id("Pos").unwrap();
    assert_eq!(w.query().has(pos).count(), 101);
    let hit: usize = w
        .query()
        .has(pos)
        .entities()
        .iter()
        .filter(|e| w.get_field(**e, "Pos.hits").unwrap() == Value::UInt(1))
        .count();
    assert_eq!(hit, 5, "entities at x 8..12 were within 2.5 of x 10");
    // The vector read and write cost no table: what remains per entity is
    // the binding's call path (mlua allocates its argument and result
    // lists per call), about 17 allocations for the two calls above. The
    // table form was two tables more per entity on top of that.
    let rows = s.prof_alloc(10).unwrap();
    let vec = rows.iter().find(|r| r.name == "sim.vec").unwrap();
    assert!(
        vec.max_allocs < 2500,
        "tick 1 over 101 entities allocated {}",
        vec.max_allocs
    );
}

/// `q:events(name)`: the game events emitted so far this tick, as tables
/// of their fields plus `name`; a later tick sees only its own (section 8
/// L2, how a script reacts to what the physics step reported).
#[test]
fn a_script_reads_the_events_emitted_earlier_in_its_tick() {
    let (mut h, mut w, mut s) = host(1);
    assert!(h.load_source(&mut w, &mut s, "components/all.luau", COMPONENTS));
    let sys = r#"
component "Heard" { pings = field.u32 { default = 0 }, last = field.i32 { default = 0 } }
system "sim.ping" {
  stage = "sim", writes = { "Heard" },
  run = function(q)
    if q.tick == 0 then
      q:spawn({ "Heard" })
      q:emit("ping", { n = 7 })
      q:emit("other", {})
    end
  end,
}
return system "late.hear" {
  stage = "late", reads = { "Heard" }, writes = { "Heard" },
  run = function(q)
    local pings = q:events("ping")
    local all = q:events()
    for e, h in q:each("Heard") do
      h.pings += #pings
      if #pings > 0 then
        assert(pings[1].name == "ping" and pings[1].n == 7, tostring(pings[1].n))
        assert(#all >= 2, "every game event without a name: " .. #all)
        h.last = pings[1].n
      end
    end
  end,
}"#;
    assert!(
        h.load_source(&mut w, &mut s, "systems/hear.luau", sys),
        "{:?}",
        h.errors()
    );
    s.step(&mut w, 3).unwrap();
    assert!(fault_messages(&w).is_empty(), "{:?}", fault_messages(&w));
    let heard = w.component_id("Heard").unwrap();
    let e = w.query().has(heard).entities()[0];
    assert_eq!(
        w.get_field(e, "Heard.pings").unwrap(),
        Value::UInt(1),
        "seen once, on its own tick"
    );
    assert_eq!(w.get_field(e, "Heard.last").unwrap(), Value::Int(7));
}

#[test]
fn an_unfrozen_script_system_runs_through_a_hit_stop() {
    let (mut h, mut w, mut s) = host(1);
    // A hit-stop on ticks 1 to 3 (phase 2 item 6).
    s.add_system(
        SystemDesc::new("input.stop", Stage::Input).first(),
        Box::new(|ctx| {
            if (1..4).contains(&ctx.tick) {
                ctx.freeze();
            }
            Ok(())
        }),
    )
    .unwrap();
    let src = r#"
component "Game" { n = field.u32 {} }
component "Effect" { n = field.u32 {} }
system "sim.game" {
  stage = "sim", writes = { "Game" },
  run = function(q) for e, g in q:each("Game") do g.n += 1 end end,
}
return system "sim.effect" {
  stage = "sim", writes = { "Effect" }, unfrozen = true,
  run = function(q) for e, f in q:each("Effect") do f.n += 1 end end,
}"#;
    assert!(
        h.load_source(&mut w, &mut s, "systems/stop.luau", src),
        "{:?}",
        h.errors()
    );
    let e = w.spawn();
    w.insert_by_name(e, "Game").unwrap();
    w.insert_by_name(e, "Effect").unwrap();
    s.step(&mut w, 5).unwrap();
    assert!(fault_messages(&w).is_empty(), "{:?}", fault_messages(&w));
    assert_eq!(
        w.get_field(e, "Game.n").unwrap(),
        Value::UInt(2),
        "ticks 0 and 4"
    );
    assert_eq!(
        w.get_field(e, "Effect.n").unwrap(),
        Value::UInt(5),
        "all five"
    );
    let info = s.systems(w.registry()).unwrap();
    assert!(info.iter().any(|i| i.name == "sim.effect" && i.unfrozen));
}

/// Player-facing strings from a script (phase 3 item 4): a key in the
/// world's language, its {name} holes from the table, "[key]" where there
/// is none, and the languages there are.
struct Words;

impl argus_luau::Texts for Words {
    fn text(&self, _: &World, key: &str) -> Option<String> {
        match key {
            "hud.day" => Some("Day {day}, {n} left".to_string()),
            "menu.go" => Some("Go".to_string()),
            _ => None,
        }
    }
    fn languages(&self) -> Vec<String> {
        vec!["en".into(), "fr".into()]
    }
}

#[test]
fn a_script_asks_for_its_words_by_key() {
    let (mut h, mut w, mut s) = host(1);
    h.set_texts(std::rc::Rc::new(Words));
    let sys = r#"
return system "sim.words" {
  stage = "sim",
  run = function(q)
    local langs = q:languages()
    q:emit("words", {
      day = q:text("hud.day", { day = 3, n = 2.5 }),
      go = q:text("menu.go"),
      none = q:text("menu.nope"),
      langs = table.concat(langs, ","),
    })
  end,
}"#;
    assert!(
        h.load_source(&mut w, &mut s, "systems/words.luau", sys),
        "{:?}",
        h.errors()
    );
    s.step(&mut w, 1).unwrap();
    let ev: Vec<(String, Value)> = w
        .events()
        .iter()
        .find_map(|r| match &r.event {
            Event::Game { name, fields } if name == "words" => Some(fields.clone()),
            _ => None,
        })
        .expect("emitted");
    let get = |k: &str| {
        ev.iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.clone())
            .unwrap()
    };
    assert_eq!(get("day"), Value::Str("Day 3, 2.5 left".into()));
    assert_eq!(get("go"), Value::Str("Go".into()));
    assert_eq!(get("none"), Value::Str("[menu.nope]".into()));
    assert_eq!(get("langs"), Value::Str("en,fr".into()));
}

/// Issue 13: a script's strings are bytes, and text cut through a character
/// (`string.sub` at a byte count, through an "é") switched the system off
/// when it was written. Text is kept now, the cut character as U+FFFD, in a
/// field, an event, and what the game sends its host.
#[test]
fn text_cut_through_a_character_is_kept_with_a_replacement() {
    let (mut h, mut w, mut s) = host(7);
    h.load_source(
        &mut w,
        &mut s,
        "components/note.luau",
        "component \"Note\" { text = field.str {} }",
    );
    let sys = r#"
return system "sim.cut" {
  stage = "sim", writes = { "Note" },
  run = function(q)
    local cut = string.sub("caf" .. "\u{e9}", 1, 4)
    q:entity("@n"):get("Note").text = cut
    q:emit("cut", { text = cut })
    q:send("gm.request", cut)
  end,
}"#;
    assert!(
        h.load_source(&mut w, &mut s, "systems/cut.luau", sys),
        "{:?}",
        h.errors()
    );
    let n = w.spawn_authored("n").unwrap();
    w.insert_by_name(n, "Note").unwrap();
    s.step(&mut w, 1).unwrap();
    assert!(fault_messages(&w).is_empty(), "{:?}", fault_messages(&w));
    let kept = "caf\u{fffd}".to_string();
    assert_eq!(
        w.get_field(n, "Note.text").unwrap(),
        Value::Str(kept.clone())
    );
    let text_of = |name: &str| {
        w.events().iter().find_map(|r| match &r.event {
            Event::Game { name: n, fields } if n == name => {
                fields.iter().find(|f| f.0 == "text").map(|f| f.1.clone())
            }
            _ => None,
        })
    };
    assert_eq!(text_of("cut"), Some(Value::Str(kept.clone())));
    assert_eq!(text_of("host.send"), Some(Value::Str(kept)));
}

/// Issue 20: a generator from given numbers draws the same values for the
/// same numbers, whatever was drawn first, and other values for others.
#[test]
fn rng_from_gives_the_same_draws_for_the_same_numbers() {
    let (mut h, mut w, mut s) = host(7);
    let sys = r#"
return system "sim.rooms" {
  stage = "sim",
  run = function(q)
    local function room(seed, floor, n)
      local r = q:rng_from(seed, floor, n)
      return r:range(1, 1000000) .. "," .. r:below(1000) .. "," .. tostring(r:chance(0.5))
    end
    local a = room(4242, 1, 3)
    for i = 1, 5 do room(4242, 1, i) end
    assert(room(4242, 1, 3) == a, "the same numbers, the same room")
    assert(room(4242, 1, 4) ~= a and room(4243, 1, 3) ~= a, "others differ")
    local ok = pcall(q.rng_from, q, 1.5)
    assert(not ok, "a fraction is refused")
    q:emit("room", { text = a })
  end,
}"#;
    assert!(
        h.load_source(&mut w, &mut s, "systems/rooms.luau", sys),
        "{:?}",
        h.errors()
    );
    s.step(&mut w, 1).unwrap();
    assert!(fault_messages(&w).is_empty(), "{:?}", fault_messages(&w));
    let drawn = w.events().iter().find_map(|r| match &r.event {
        Event::Game { name, fields } if name == "room" => Some(fields[0].1.clone()),
        _ => None,
    });
    // Pinned: these are the engine's numbers on every machine.
    assert_eq!(drawn, Some(Value::Str("459794,727,true".to_string())));
}
