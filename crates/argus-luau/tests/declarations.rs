//! §3 "from Luau via a schema file", §4 def schemas, §7 "compile error as a
//! structured script.errors response with file:line, never as a crash",
//! "script globals frozen after module load".

use argus_ecs::{ComponentKind, FieldType, Value, World};
use argus_luau::{codes, ErrorPhase, ScriptConfig, ScriptHost};
use argus_sim::{Scheduler, SchedulerConfig};
use std::path::{Path, PathBuf};

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

/// An empty `argus-<name>-<pid>` in the temp directory.
fn temp(name: &str) -> Temp {
    let dir = std::env::temp_dir().join(format!("argus-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Temp(dir)
}

fn host() -> (ScriptHost, World, Scheduler) {
    (
        ScriptHost::new(ScriptConfig::default()).unwrap(),
        World::new(),
        Scheduler::new(SchedulerConfig::default()),
    )
}

const HITSTOP: &str = r#"
return component "HitStop" {
  doc    = "Freezes an entity's simulation for N frames after a hit. Read by sim.hitstop.",
  version = 2,
  frames = field.u8 { default = 0, range = {0, 30}, doc = "Remaining frozen frames" },
  scale  = field.f32{ default = 0.0, range = {0, 1}, tune = true,
                      doc = "Time scale while frozen. 0 = full stop" },
  migrate = function(old, from) return old end,
}
"#;

#[test]
fn component_declaration_becomes_an_introspectable_schema() {
    let (mut h, mut w, mut s) = host();
    assert!(h.load_source(&mut w, &mut s, "components/hitstop.luau", HITSTOP));
    assert!(h.errors().is_empty(), "{:?}", h.errors());
    let sc = w.describe("HitStop").unwrap();
    assert!(sc.doc.starts_with("Freezes"));
    assert_eq!(sc.version, 2);
    assert_eq!(sc.source.as_deref(), Some("components/hitstop.luau:2"));
    // Declaration order is preserved even though Lua tables are unordered.
    let names: Vec<&str> = sc.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["frames", "scale"]);
    let frames = sc.field("frames").unwrap();
    assert_eq!(frames.ty, FieldType::U8);
    assert_eq!(frames.default, Value::UInt(0));
    assert_eq!(frames.range, Some((0.0, 30.0)));
    assert_eq!(frames.doc, "Remaining frozen frames");
    assert!(!frames.tune);
    let scale = sc.field("scale").unwrap();
    assert_eq!(scale.ty, FieldType::F32);
    assert!(scale.tune);
    assert_eq!(sc.size, 8);
    let info = &h.components()[0];
    assert_eq!(info.name, "HitStop");
    assert_eq!(info.file, "components/hitstop.luau");
    assert!(info.has_migrate);
    assert!(h.has_migration("HitStop"));
}

#[test]
fn field_types_defaults_and_tags() {
    let (mut h, mut w, mut s) = host();
    let src = r#"
component "Door" {
  leads_to = field.entity { doc = "Entity ref by string id" },
  key      = field.ref { default = "item:key.brass", doc = "Def ref, not a GUID" },
  label    = field.str { default = "a door" },
  pos      = field.vec2 { default = {46, 8} },
  locked   = field.bool { default = true },
  hp       = field.i32 { default = -5 },
}
component "Enemy" {}
"#;
    assert!(
        h.load_source(&mut w, &mut s, "components/door.luau", src),
        "{:?}",
        h.errors()
    );
    let d = w.describe("Door").unwrap();
    assert_eq!(d.field("leads_to").unwrap().ty, FieldType::Entity);
    assert_eq!(
        d.field("key").unwrap().default,
        Value::Ref("item:key.brass".into())
    );
    assert_eq!(
        d.field("label").unwrap().default,
        Value::Str("a door".into())
    );
    assert_eq!(
        d.field("pos").unwrap().default,
        Value::Vec2(argus_ecs::Vec2::new(46.0, 8.0))
    );
    assert_eq!(d.field("locked").unwrap().default, Value::Bool(true));
    assert_eq!(d.field("hp").unwrap().default, Value::Int(-5));
    assert!(d.has_entity_refs());
    assert_eq!(w.describe("Enemy").unwrap().kind(), ComponentKind::Tag);
    assert_eq!(h.components().len(), 2);
}

#[test]
fn def_schema_declaration() {
    let (mut h, mut w, mut s) = host();
    let src = r#"
return def "recipes" {
  doc    = "Crafting recipes",
  key    = "id",
  id     = field.str {},
  output = field.ref { doc = "item produced" },
  time_s = field.f32 { default = 1.0, range = {0.1, 600}, tune = true },
}
"#;
    assert!(
        h.load_source(&mut w, &mut s, "defs/recipes.luau", src),
        "{:?}",
        h.errors()
    );
    let d = &h.defs()[0];
    assert_eq!(d.schema.name, "recipes");
    assert_eq!(d.key, "id");
    assert_eq!(d.schema.fields.len(), 3);
    assert!(d.schema.field("time_s").unwrap().tune);
    assert!(
        w.component_id("recipes").is_none(),
        "defs are not components"
    );

    let bad = r#"return def "items" { key = "code", name = field.str {} }"#;
    assert!(!h.load_source(&mut w, &mut s, "defs/items.luau", bad));
    assert!(h.errors()[0].message.contains("key field"));
}

#[test]
fn errors_are_file_line_data_never_a_crash() {
    let (mut h, mut w, mut s) = host();

    // Syntax.
    assert!(!h.load_source(
        &mut w,
        &mut s,
        "systems/broken.luau",
        "return system \"x\" {\n  stage = \n}"
    ));
    let e = h.errors();
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].phase, ErrorPhase::Syntax);
    assert_eq!(e[0].file, "systems/broken.luau");
    assert_eq!(e[0].line, Some(3));
    assert!(e[0].message.contains("Expected"), "{}", e[0].message);

    // Runtime error while loading.
    assert!(!h.load_source(
        &mut w,
        &mut s,
        "systems/boom.luau",
        "local t = nil\nreturn t.x"
    ));
    let e = h.errors();
    let boom = e.iter().find(|e| e.file == "systems/boom.luau").unwrap();
    assert_eq!(boom.phase, ErrorPhase::Load);
    assert_eq!(boom.line, Some(2));
    assert!(boom.message.contains("attempt to index nil"));

    // Malformed declarations.
    assert!(!h.load_source(
        &mut w,
        &mut s,
        "components/bad.luau",
        "return component \"Bad\" {\n  n = 5\n}"
    ));
    let bad = h
        .errors()
        .into_iter()
        .find(|e| e.file == "components/bad.luau")
        .unwrap()
        .clone();
    assert_eq!(bad.phase, ErrorPhase::Declaration);
    assert_eq!(bad.line, Some(1), "points at the component line");
    assert!(
        bad.message.contains("Bad.n") && bad.message.contains("field.<type>"),
        "{}",
        bad.message
    );

    assert!(!h.load_source(
        &mut w,
        &mut s,
        "systems/nostage.luau",
        "return system \"sim.x\" {\n run = function(q) end }"
    ));
    let ns = h
        .errors()
        .into_iter()
        .find(|e| e.file == "systems/nostage.luau")
        .unwrap()
        .clone();
    assert!(ns.message.contains("stage is required"));

    assert!(!h.load_source(
        &mut w,
        &mut s,
        "systems/badstage.luau",
        "return system \"sim.y\" {\n stage = \"weekly\", run = function(q) end }"
    ));
    let bs = h
        .errors()
        .into_iter()
        .find(|e| e.file == "systems/badstage.luau")
        .unwrap()
        .clone();
    assert!(bs.message.contains("unknown stage") && bs.message.contains("input, sim"));

    // Undeclared component in reads: the static half of the access rule.
    assert!(!h.load_source(
        &mut w,
        &mut s,
        "systems/ghost.luau",
        "return system \"sim.z\" {\n stage = \"sim\", reads = {\"Ghost\"}, run = function(q) end }"
    ));
    let g = h
        .errors()
        .into_iter()
        .find(|e| e.file == "systems/ghost.luau")
        .unwrap()
        .clone();
    assert!(g.message.contains("Ghost"), "{}", g.message);
    assert_eq!(g.line, Some(1));

    // Loading a good file for the same path clears its errors.
    assert!(h.load_source(&mut w, &mut s, "systems/ghost.luau", "return 1"));
    assert!(h.errors().iter().all(|e| e.file != "systems/ghost.luau"));

    // Bad range on a non-numeric type is caught by the schema builder with the file ref.
    assert!(!h.load_source(
        &mut w,
        &mut s,
        "components/range.luau",
        "return component \"R\" { s = field.str { range = {0, 1} } }"
    ));
    let r = h
        .errors()
        .into_iter()
        .find(|e| e.file == "components/range.luau")
        .unwrap()
        .clone();
    assert!(r.message.contains("range on non-numeric"), "{}", r.message);
    assert_eq!(h.errors().len(), 6);
}

#[test]
fn conflicting_redeclaration_is_reported_with_a_hint() {
    let (mut h, mut w, mut s) = host();
    assert!(h.load_source(
        &mut w,
        &mut s,
        "components/a.luau",
        "return component \"A\" { x = field.f32 {} }"
    ));
    assert!(!h.load_source(
        &mut w,
        &mut s,
        "components/a2.luau",
        "return component \"A\" { x = field.f64 {} }"
    ));
    let e = h.errors()[0].clone();
    assert_eq!(e.phase, ErrorPhase::Declaration);
    assert!(
        e.message.contains("different schema") && e.message.contains("version"),
        "{}",
        e.message
    );
    // Same shape again is fine (idempotent registration).
    assert!(h.load_source(
        &mut w,
        &mut s,
        "components/a3.luau",
        "return component \"A\" { x = field.f32 {} }"
    ));
}

/// A game's own component by an engine component's name says the name is
/// taken, not that a migration would help (the platformer template's
/// Parallax met the engine's).
#[test]
fn a_component_by_an_engine_name_is_told_the_name_is_taken() {
    let (mut h, mut w, mut s) = host();
    w.register::<argus_ecs::Transform>().unwrap();
    assert!(!h.load_source(
        &mut w,
        &mut s,
        "components/looks.luau",
        "return component \"Transform\" { follow = field.f32 {} }"
    ));
    let e = h.errors()[0].clone();
    assert!(e.message.contains("engine's own"), "{}", e.message);
    assert!(
        !e.message.contains("migration") && e.message.contains("Rename yours"),
        "{}",
        e.message
    );
}

#[test]
fn globals_are_frozen_and_the_sandbox_holds() {
    let (mut h, mut w, mut s) = host();
    assert!(!h.load_source(&mut w, &mut s, "systems/leak.luau", "_G.leak = 1"));
    assert!(
        h.errors()[0].message.contains("readonly"),
        "{}",
        h.errors()[0].message
    );
    // Globals are read-only even at module load, so nothing can live there:
    // module state must be a local. This is stricter than "frozen after load".
    let src = "counter = 0\nreturn system \"sim.g\" { stage = \"sim\", run = function(q) counter = counter + 1 end }";
    assert!(!h.load_source(&mut w, &mut s, "systems/g.luau", src));
    assert!(h
        .errors()
        .iter()
        .any(|e| e.file == "systems/g.luau" && e.message.contains("readonly")));
    // And a run function cannot create one later either.
    let src = "return system \"sim.g2\" { stage = \"sim\", run = function(q) counter = 1 end }";
    assert!(
        h.load_source(&mut w, &mut s, "systems/g2.luau", src),
        "{:?}",
        h.errors()
    );
    s.step(&mut w, 1).unwrap();
    let fault = w.events().iter().find_map(|r| match &r.event {
        argus_ecs::Event::SystemFault { message, .. } => Some(message.clone()),
        _ => None,
    });
    assert!(fault.unwrap().contains("readonly"));
    assert!(!h.load_source(&mut w, &mut s, "systems/io.luau", "return io.open('x')"));
    assert!(!h.load_source(
        &mut w,
        &mut s,
        "systems/os.luau",
        "return os.execute('dir')"
    ));
    // Module-level locals are fine; they are the file's own.
    assert!(h.load_source(&mut w, &mut s, "systems/ok.luau", "local x = 1\nreturn x"));
    let _ = codes::BAD_DECLARATION;
}

#[test]
fn a_project_directory_loads_in_order() {
    let root = temp("luau-proj");
    std::fs::create_dir_all(root.join("components")).unwrap();
    std::fs::create_dir_all(root.join("systems/sub")).unwrap();
    std::fs::create_dir_all(root.join("defs")).unwrap();
    std::fs::write(
        root.join("components/hp.luau"),
        "return component \"Hp\" { cur = field.i32 { default = 3 } }",
    )
    .unwrap();
    std::fs::write(
        root.join("defs/items.luau"),
        "return def \"items\" { id = field.str {} }",
    )
    .unwrap();
    std::fs::write(
        root.join("systems/sub/heal.luau"),
        "return system \"sim.heal\" { stage = \"sim\", writes = {\"Hp\"}, run = function(q) for e, hp in q:each(\"Hp\") do hp.cur += 1 end end }",
    )
    .unwrap();
    let (mut h, mut w, mut s) = host();
    assert!(
        h.load_project(&mut w, &mut s, &root).unwrap(),
        "{:?}",
        h.errors()
    );
    assert_eq!(
        h.files(),
        vec![
            "components/hp.luau",
            "defs/items.luau",
            "systems/sub/heal.luau"
        ]
    );
    assert_eq!(
        h.systems_in("systems/sub/heal.luau"),
        &["sim.heal".to_string()]
    );
    let e = w.spawn();
    w.insert_by_name(e, "Hp").unwrap();
    s.step(&mut w, 2).unwrap();
    assert_eq!(w.get_field(e, "Hp.cur").unwrap(), Value::Int(5));
}
