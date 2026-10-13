//! Shared code for game scripts (GitHub issue 24): modules in `lib/`,
//! required by systems, frozen, reloaded with the files that use them, and
//! a mod's own beside the game's.

use argus_ecs::{Event, Layers, Value, World};
use argus_luau::{ScriptConfig, ScriptHost};
use argus_sim::{Scheduler, SchedulerConfig};
use std::path::{Path, PathBuf};

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
    let root = std::env::temp_dir().join(format!("argus-luau-mod-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    Temp(root)
}

fn put(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn faults(w: &World) -> Vec<String> {
    w.events()
        .iter()
        .filter_map(|r| match &r.event {
            Event::SystemFault {
                system, message, ..
            } => Some(format!("{system}: {message}")),
            _ => None,
        })
        .collect()
}

const HP: &str = "return component \"Hp\" { cur = field.i32 { default = 0 } }";

/// Two systems use the same module: a constant and a function.
const RULES: &str = r#"-- The rules both systems use.
local BASE = 3
return {
  BASE = BASE,
  bonus = { fire = 2 },
  hit = function(hp, n) hp.cur += n + BASE end,
}"#;

const ONE: &str = r#"local rules = require("rules")
return system "sim.one" {
  stage = "sim", writes = { "Hp" },
  run = function(q)
    for _, hp in q:each("Hp") do rules.hit(hp, 1) end
  end,
}"#;

const TWO: &str = r#"local rules = require("rules")
local checks = require("rules/checks")
return system "sim.two" {
  stage = "sim", after = { "sim.one" }, writes = { "Hp" },
  run = function(q)
    for _, hp in q:each("Hp") do hp.cur += checks.twice(rules.BASE) end
    -- The module's value is frozen: one system cannot change another's.
    local ok = pcall(function() rules.BASE = 100 end)
    local ok2 = pcall(function() rules.bonus.fire = 100 end)
    assert(not ok and not ok2, "the module is frozen")
  end,
}"#;

#[test]
fn systems_share_a_module_and_a_change_to_it_reloads_them() {
    let root = project("share");
    put(&root, "components/hp.luau", HP);
    put(&root, "lib/rules.luau", RULES);
    put(
        &root,
        "lib/rules/checks.luau",
        "return { twice = function(n) return 2 * n end }",
    );
    put(&root, "systems/one.luau", ONE);
    put(&root, "systems/two.luau", TWO);
    put(
        &root,
        "systems/alone.luau",
        "return system \"sim.alone\" { stage = \"sim\", run = function(q) end }",
    );
    let mut h = ScriptHost::new(ScriptConfig::default()).unwrap();
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    assert!(
        h.load_project(&mut w, &mut s, &root).unwrap(),
        "{:?}",
        h.errors()
    );
    let e = w.spawn_authored("p").unwrap();
    w.insert_by_name(e, "Hp").unwrap();
    s.step(&mut w, 1).unwrap();
    assert!(faults(&w).is_empty(), "{:?}", faults(&w));
    // one: 1 + 3; two: 2 * 3.
    assert_eq!(w.get_field(e, "Hp.cur").unwrap(), Value::Int(10));
    let mods: Vec<(String, String, Vec<String>, String)> = h
        .modules()
        .into_iter()
        .map(|m| (m.file, m.name, m.used_by, m.doc))
        .collect();
    let v = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    assert_eq!(
        mods,
        vec![
            (
                "lib/rules.luau".to_string(),
                "rules".to_string(),
                v(&["systems/one.luau", "systems/two.luau"]),
                "The rules both systems use.".to_string()
            ),
            (
                "lib/rules/checks.luau".to_string(),
                "rules/checks".to_string(),
                v(&["systems/two.luau"]),
                String::new()
            ),
        ]
    );

    // Nothing changed: nothing reloads.
    let r = h.reload(&mut w, &mut s).unwrap();
    assert!(r.reloaded.is_empty() && r.modules.is_empty(), "{r:?}");

    // The rules change: both systems reload with them, the other does not.
    put(
        &root,
        "lib/rules.luau",
        &RULES.replace("local BASE = 3", "local BASE = 5"),
    );
    let r = h.reload(&mut w, &mut s).unwrap();
    assert_eq!(r.modules, vec!["lib/rules.luau"]);
    assert_eq!(
        r.reloaded,
        vec!["systems/one.luau", "systems/two.luau"],
        "{r:?}"
    );
    s.step(&mut w, 1).unwrap();
    assert_eq!(w.get_field(e, "Hp.cur").unwrap(), Value::Int(10 + 6 + 10));

    // A module used through another: checks changes, and two (which uses
    // it) reloads.
    put(
        &root,
        "lib/rules/checks.luau",
        "return { twice = function(n) return 3 * n end }",
    );
    let r = h.reload(&mut w, &mut s).unwrap();
    assert_eq!(r.reloaded, vec!["systems/two.luau"], "{r:?}");

    // A module that no longer compiles keeps its users as they are.
    put(&root, "lib/rules.luau", "return { hit = ");
    let r = h.reload(&mut w, &mut s).unwrap();
    assert!(r.reloaded.is_empty(), "{r:?}");
    assert_eq!(r.errors, 1);
    assert!(
        h.errors().iter().any(|e| e.file == "lib/rules.luau"),
        "{:?}",
        h.errors()
    );
    let before = w.get_field(e, "Hp.cur").unwrap();
    s.step(&mut w, 1).unwrap();
    assert_ne!(
        w.get_field(e, "Hp.cur").unwrap(),
        before,
        "the systems still run"
    );
    assert!(faults(&w).is_empty(), "{:?}", faults(&w));
    // Fixed: its users reload, and the error goes.
    put(&root, "lib/rules.luau", RULES);
    let r = h.reload(&mut w, &mut s).unwrap();
    assert_eq!(
        r.reloaded,
        vec!["systems/one.luau", "systems/two.luau"],
        "{r:?}"
    );
    assert_eq!(r.errors, 0, "{:?}", h.errors());
}

#[test]
fn a_missing_module_and_a_cycle_say_what_is_there() {
    let root = project("missing");
    put(&root, "lib/rules.luau", "return {}");
    put(&root, "lib/a.luau", "return { b = require(\"b\") }");
    put(&root, "lib/b.luau", "return { a = require(\"a\") }");
    put(&root, "systems/gone.luau", "local x = require(\"rulez\")\nreturn system \"sim.gone\" { stage = \"sim\", run = function(q) end }");
    put(&root, "systems/loop.luau", "local a = require(\"a\")\nreturn system \"sim.loop\" { stage = \"sim\", run = function(q) end }");
    let mut h = ScriptHost::new(ScriptConfig::default()).unwrap();
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    assert!(!h.load_project(&mut w, &mut s, &root).unwrap());
    let said: Vec<String> = h
        .errors()
        .iter()
        .map(|e| format!("{}: {}", e.file, e.message))
        .collect();
    let gone = said
        .iter()
        .find(|e| e.starts_with("systems/gone.luau"))
        .unwrap();
    assert!(
        gone.contains("there is no lib/rulez.luau") && gone.contains("a, b, rules"),
        "{gone}"
    );
    let lp = said
        .iter()
        .find(|e| e.starts_with("systems/loop.luau"))
        .unwrap();
    assert!(
        lp.contains(
            "in a circle: lib/a.luau, which requires lib/b.luau, which requires lib/a.luau"
        ),
        "{lp}"
    );
    // Said once, where the circle closes, not again for each module above.
    assert_eq!(
        said.iter().filter(|e| e.starts_with("lib/")).count(),
        1,
        "{said:?}"
    );
    assert!(
        h.errors().iter().all(|e| e.line == Some(1)),
        "{:?}",
        h.errors()
    );
}

#[test]
fn a_mods_files_use_its_own_module_and_the_games_use_the_games() {
    let root = project("mods");
    put(&root, "components/hp.luau", HP);
    put(&root, "lib/rules.luau", "return { n = 1 }");
    put(&root, "lib/shared.luau", "return { n = 100 }");
    put(
        &root,
        "systems/game.luau",
        "local rules = require(\"rules\")\nreturn system \"sim.game\" { stage = \"sim\", writes = {\"Hp\"}, run = function(q) for _, hp in q:each(\"Hp\") do hp.cur += rules.n end end }",
    );
    put(&root, "mods/m/mod.toml", "id = \"m\"\nname = \"M\"\n");
    put(&root, "mods/m/lib/rules.luau", "return { n = 10 }");
    put(
        &root,
        "mods/m/systems/more.luau",
        "local rules = require(\"rules\")\nlocal shared = require(\"shared\")\nreturn system \"m.more\" { stage = \"sim\", after = {\"sim.game\"}, writes = {\"Hp\"}, run = function(q) for _, hp in q:each(\"Hp\") do hp.cur += rules.n + shared.n end end }",
    );
    let mut h = ScriptHost::new(ScriptConfig::default()).unwrap();
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    let layers = Layers::with_mods(&root, &["m".to_string()]);
    assert!(
        h.load_layers(&mut w, &mut s, &layers).unwrap(),
        "{:?}",
        h.errors()
    );
    let e = w.spawn_authored("p").unwrap();
    w.insert_by_name(e, "Hp").unwrap();
    s.step(&mut w, 1).unwrap();
    assert!(faults(&w).is_empty(), "{:?}", faults(&w));
    // The game's file gets the game's rules (1); the mod's its own (10) and
    // the game's shared (100).
    assert_eq!(w.get_field(e, "Hp.cur").unwrap(), Value::Int(111));
}
