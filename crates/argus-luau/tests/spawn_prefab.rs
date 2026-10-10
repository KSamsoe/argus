//! Scripts spawning from a prefab (GitHub issue 1): `q:spawn_prefab` places
//! the prefab's entity and its children with the prefab's fields, then the
//! caller's; `spawns` grants the prefab's components; a prefab not named
//! there, or a field of a component the prefab lacks, is an error.

use argus_ecs::{Event, GlobalTransform, Parent, Transform, Value, Vec2, World};
use argus_luau::{Prefabs, ScriptConfig, ScriptHost, SpawnPlan};
use argus_sim::{Scheduler, SchedulerConfig};
use std::cell::Cell;
use std::rc::Rc;

/// One prefab, `enemy/bat`, as a host would resolve it: a tag, fields, a
/// reference to an authored entity, and a child.
struct Bats {
    asked: Cell<usize>,
}

impl Prefabs for Bats {
    fn plan(&self, _world: &World, name: &str) -> argus_ecs::Result<Rc<SpawnPlan>> {
        self.asked.set(self.asked.get() + 1);
        if name != "enemy/bat" {
            return Err(argus_ecs::ArgusError::new(
                argus_ecs::codes::UNKNOWN_ID,
                format!("no prefab named {name:?}"),
            ));
        }
        Ok(Rc::new(SpawnPlan {
            name: name.into(),
            tags: vec!["enemy".into()],
            components: vec![
                (
                    "Transform".into(),
                    vec![("pos".into(), Value::Vec2(Vec2::new(2.0, 0.0)))],
                ),
                ("Health".into(), vec![("cur".into(), Value::Int(3))]),
                (
                    "Target".into(),
                    vec![("who".into(), Value::Str("@hero".into()))],
                ),
            ],
            children: vec![SpawnPlan {
                name: "eye".into(),
                components: vec![
                    (
                        "Transform".into(),
                        vec![("pos".into(), Value::Vec2(Vec2::new(0.5, 0.5)))],
                    ),
                    ("Glow".into(), vec![]),
                ],
                file: "prefabs/enemy/bat.toml".into(),
                ..Default::default()
            }],
            file: "prefabs/enemy/bat.toml".into(),
        }))
    }
}

const COMPONENTS: &str = r#"
component "Health" { cur = field.i32 { default = 10 }, max = field.i32 { default = 10 } }
component "Target" { who = field.entity {} }
component "Glow" {}
component "Spawner" { done = field.bool { default = false } }
"#;

fn setup(system: &str) -> (ScriptHost, World, Scheduler, Rc<Bats>) {
    let mut w = World::new();
    w.register::<Transform>().unwrap();
    w.register::<GlobalTransform>().unwrap();
    w.register::<Parent>().unwrap();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0).seed(1));
    let mut h = ScriptHost::new(ScriptConfig::default()).unwrap();
    let bats = Rc::new(Bats {
        asked: Cell::new(0),
    });
    h.set_prefabs(bats.clone());
    assert!(
        h.load_source(&mut w, &mut s, "components/all.luau", COMPONENTS),
        "{:?}",
        h.errors()
    );
    w.spawn_authored("hero").unwrap();
    assert!(
        h.load_source(&mut w, &mut s, "systems/spawn.luau", system),
        "{:?}",
        h.errors()
    );
    (h, w, s, bats)
}

fn faults(w: &World) -> Vec<String> {
    w.events()
        .iter()
        .filter_map(|r| match &r.event {
            Event::SystemFault { message, .. } => Some(message.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_prefab_spawns_with_its_fields_children_and_the_callers_fields() {
    // Nothing in writes: naming the prefab in spawns grants its components.
    let sys = r#"
return system "sim.spawn" {
  stage = "sim", reads = { "Health" }, spawns = { "enemy/bat" },
  run = function(q)
    if q.tick == 0 then
      local a = q:spawn_prefab("enemy/bat")
      local b = q:spawn_prefab("prefabs/enemy/bat.toml", { ["Transform.pos"] = vector.create(9, 4, 0), ["Health.cur"] = 7 })
      assert(a:get("Health").cur == 3, "the prefab's value")
      assert(b:get("Health").cur == 7, "the caller's value")
      assert(b:get("Health").max == 10, "the schema's default")
    end
  end,
}"#;
    let (_h, mut w, mut s, bats) = setup(sys);
    s.step(&mut w, 2).unwrap();
    assert!(faults(&w).is_empty(), "{:?}", faults(&w));
    assert_eq!(
        bats.asked.get(),
        2,
        "the host is asked for the plan each spawn"
    );

    let enemy = w.component_id("enemy").expect("the tag is registered");
    let bats_now: Vec<_> = w.query().has(enemy).entities();
    assert_eq!(bats_now.len(), 2);
    let hero = w.entity("@hero").unwrap();
    for &b in &bats_now {
        assert_eq!(w.get_field(b, "Target.who").unwrap(), Value::Entity(hero));
    }
    let pos = |e| w.get::<Transform>(e).unwrap().pos;
    let second = bats_now
        .iter()
        .copied()
        .find(|&e| pos(e) == Vec2::new(9.0, 4.0))
        .expect("the caller's position");
    // Each bat has its eye, parented, with a GlobalTransform to be placed by.
    let glow = w.component_id("Glow").unwrap();
    let eyes: Vec<_> = w.query().has(glow).entities();
    assert_eq!(eyes.len(), 2);
    let eye = eyes
        .iter()
        .copied()
        .find(|&e| w.get::<Parent>(e).unwrap().entity == second)
        .expect("the second bat's eye");
    assert_eq!(w.get::<Transform>(eye).unwrap().pos, Vec2::new(0.5, 0.5));
    assert!(w.get::<GlobalTransform>(eye).is_ok());
    assert!(
        w.string_id(eye).is_none(),
        "a spawned child is a runtime entity"
    );
}

#[test]
fn a_prefab_not_in_spawns_is_an_error_that_says_how() {
    let sys = r#"
return system "sim.spawn" {
  stage = "sim", writes = { "Health" },
  run = function(q)
    if q.tick == 0 then q:spawn_prefab("enemy/bat") end
  end,
}"#;
    let (_h, mut w, mut s, _) = setup(sys);
    s.step(&mut w, 1).unwrap();
    let f = faults(&w).join("\n");
    assert!(
        f.contains("spawns the prefab \"enemy/bat\" without declaring it"),
        "{f}"
    );
}

#[test]
fn a_field_of_a_component_the_prefab_lacks_is_an_error() {
    let sys = r#"
return system "sim.spawn" {
  stage = "sim", spawns = { "enemy/bat" },
  run = function(q)
    if q.tick == 0 then q:spawn_prefab("enemy/bat", { ["Spawner.done"] = true }) end
  end,
}"#;
    let (_h, mut w, mut s, _) = setup(sys);
    s.step(&mut w, 1).unwrap();
    let f = faults(&w).join("\n");
    assert!(f.contains("has no Spawner for Spawner.done"), "{f}");
}

#[test]
fn a_prefab_the_host_does_not_have_says_so() {
    let sys = r#"
return system "sim.spawn" {
  stage = "sim", spawns = { "enemy/rat" },
  run = function(q)
    if q.tick == 0 then q:spawn_prefab("enemy/rat") end
  end,
}"#;
    let (_h, mut w, mut s, _) = setup(sys);
    s.step(&mut w, 1).unwrap();
    let f = faults(&w).join("\n");
    assert!(f.contains("no prefab named \"enemy/rat\""), "{f}");
}
