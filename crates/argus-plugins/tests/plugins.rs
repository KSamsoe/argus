//! Plugins end to end (phase 5 item 6): real plugin crates, built by cargo,
//! loaded, checked, and run in a scheduler. The crates are written under
//! cargo's test scratch directory and kept there between runs, so only the
//! first run compiles the SDK.
//!
//! §7: hot-path systems in "a project plugin crate ... compiled to a
//! dynamic library the daemon loads"; "the daemon snapshots, unloads the
//! old dylib, loads the new one, restores, and continues"; a compile error
//! is "file:line and the compiler's message, never a crash"; the two rules
//! (declared components only, no references across frames). §2: a panic
//! "is caught at the system boundary, the system is disabled, and the
//! fault is reported through the protocol as an event". §17: "Rust plugin
//! reload < 2 s after compile".

use argus_ecs::{Event, FieldType, SchemaBuilder, Value, Vec2, World};
use argus_plugins::{build, load_all, reload, Plugins, Source};
use argus_sim::{Scheduler, SchedulerConfig};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The test project: `plugins/<name>/` crates under cargo's scratch dir.
fn project() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("plugin-tests");
        std::fs::create_dir_all(d.join("plugins")).unwrap();
        d
    })
}

/// Write the crate `name` with `src` as its lib.rs (only when it differs,
/// so an unchanged crate stays built).
fn plugin(name: &str, src: &str) {
    let dir = project().join("plugins").join(name);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    // The SDK where a project's plugin finds it: the engine writes its own
    // into .argus/plugins/sdk before it builds.
    let toml = format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\nargus-plugin = {{ path = \"../../.argus/plugins/sdk\" }}\n\n[workspace]\n"
    );
    put(&dir.join("Cargo.toml"), &toml);
    put(&dir.join("src").join("lib.rs"), src);
}

fn put(p: &Path, text: &str) {
    if std::fs::read_to_string(p).ok().as_deref() != Some(text) {
        std::fs::write(p, text).unwrap();
    }
}

/// A world with the components the test plugins read, as a game's Luau
/// would declare them.
fn world() -> World {
    let mut w = World::new();
    for s in [
        SchemaBuilder::new("Pos").field("p", FieldType::Vec2),
        SchemaBuilder::new("Vel").field("v", FieldType::Vec2),
        SchemaBuilder::new("Hp").field("cur", FieldType::I32),
        SchemaBuilder::new("Name").field("s", FieldType::Str),
    ] {
        w.register_schema(s.build().unwrap()).unwrap();
    }
    for i in 0..10 {
        let e = w.spawn();
        w.insert_default(e, w.component_id("Pos").unwrap()).unwrap();
        w.insert_default(e, w.component_id("Vel").unwrap()).unwrap();
        w.set_field(e, "Vel.v", Vec2::new(i as f32, 1.0)).unwrap();
        w.insert_default(e, w.component_id("Hp").unwrap()).unwrap();
        w.set_field(e, "Hp.cur", 2 + i).unwrap();
    }
    w.drain_events();
    w
}

fn faults(w: &World) -> Vec<(String, String, Option<String>)> {
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

fn line_of(src: &str, needle: &str) -> u32 {
    src.lines().position(|l| l.contains(needle)).unwrap() as u32 + 1
}

const BASIC: &str = r#"use argus_plugin::prelude::*;

component! { pub struct Pos { pub p: Vec2 } }
component! { pub struct Vel { pub v: Vec2 } }
component! { pub struct Hp { pub cur: i32 } }

fn step(ctx: &Ctx) -> Result {
    let dt = ctx.dt() as f32;
    ctx.each::<(&Vel, &mut Pos)>(|_, (v, p)| p.p += v.v * dt)
}

fn hurt(ctx: &Ctx) -> Result {
    let mut dead = Vec::new();
    ctx.each::<(&mut Hp,)>(|e, (h,)| {
        h.cur -= 1;
        if h.cur <= 0 {
            dead.push(e);
        }
    })?;
    for e in dead {
        ctx.emit("died", &[("entity", e.into())])?;
        ctx.despawn(e)?;
    }
    if ctx.tick() == 2 {
        ctx.spawn(&["Hp", "Name"], &[("Hp.cur", 3.into()), ("Name.s", "fresh".into())])?;
    }
    Ok(())
}

fn boom(ctx: &Ctx) -> Result {
    if ctx.tick() == 3 {
        panic!("boom at tick 3");
    }
    Ok(())
}

fn sneaky(ctx: &Ctx) -> Result {
    ctx.each::<(&mut Hp,)>(|_, _| {})
}

plugin! {
    components: [Pos, Vel, Hp],
    systems: [
        System::new("sim.step", "sim").reads(&["Vel"]).writes(&["Pos"]).run(step),
        System::new("sim.hurt", "sim").after("sim.step").writes(&["Hp"]).spawns(&["Hp", "Name"]).run(hurt),
        System::new("late.boom", "late").run(boom), // BOOM
        System::new("late.sneaky", "late").reads(&["Hp"]).run(sneaky), // SNEAKY
    ],
}
"#;

#[test]
fn a_plugin_s_systems_run_and_a_panic_stops_only_its_own() {
    plugin("basic", BASIC);
    let mut w = world();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    let p = load_all(
        project(),
        &["basic".to_string()],
        &Source::default(),
        &mut w,
        &mut s,
    );
    assert!(p.errors.is_empty(), "{:?}", p.errors);
    assert_eq!(p.loaded.len(), 1);
    assert_eq!(p.of_system("sim.hurt"), Some("basic"));
    let infos = s.systems(w.registry()).unwrap();
    let hurt = infos.iter().find(|i| i.name == "sim.hurt").unwrap();
    assert_eq!(
        hurt.writes,
        vec!["Hp", "Name"],
        "a serial system's spawns are writes"
    );
    assert_eq!(
        hurt.source
            .as_deref()
            .map(|s| s.starts_with("plugins/basic/src/lib.rs:")),
        Some(true),
        "{:?}",
        hurt.source
    );
    s.play(&w);
    s.step(&mut w, 5).unwrap();

    // sim.step moved everything by its velocity, five ticks of 1/60.
    let v2 = |e, path| match w.get_field(e, path).unwrap() {
        Value::Vec2(v) => v,
        v => panic!("{v:?}"),
    };
    let movers = w.query().has(w.component_id("Vel").unwrap()).entities();
    assert_eq!(movers.len(), 6);
    for e in movers {
        let (p, v) = (v2(e, "Pos.p"), v2(e, "Vel.v"));
        assert!((p.x - v.x * 5.0 / 60.0).abs() < 1e-5 && (p.y - 5.0 / 60.0).abs() < 1e-5);
    }
    // sim.hurt: entity 0 (2 hp) died at tick 1, entity 1 at tick 2, ...
    let died = w
        .events()
        .iter()
        .filter(|r| matches!(&r.event, Event::Game { name, .. } if name == "died"))
        .count();
    assert_eq!(died, 4, "hp 2, 3, 4, 5 are gone after five ticks");
    assert!(w.events().iter().any(
        |r| matches!(&r.event, Event::Game { name, .. } if name == "died")
            && r.by.as_deref() == Some("sim.hurt")
    ));
    let names = w.query().has(w.component_id("Name").unwrap()).entities();
    assert_eq!(names.len(), 1, "the spawn at tick 2");
    assert_eq!(
        w.get_field(names[0], "Name.s").unwrap(),
        Value::Str("fresh".into())
    );

    // late.boom panicked at tick 3: caught at the boundary, switched off,
    // reported at its declaration; late.sneaky wrote what it only reads.
    let f = faults(&w);
    let boom = f.iter().find(|x| x.0 == "late.boom").expect("boom's fault");
    assert!(boom.1.contains("panic: boom at tick 3"), "{boom:?}");
    assert_eq!(
        boom.2.as_deref(),
        Some(format!("plugins/basic/src/lib.rs:{}", line_of(BASIC, "// BOOM")).as_str())
    );
    let sneaky = f
        .iter()
        .find(|x| x.0 == "late.sneaky")
        .expect("sneaky's fault");
    assert!(
        sneaky.1.contains("writes Hp without declaring it"),
        "{sneaky:?}"
    );
    assert!(!s.is_enabled("late.boom").unwrap());
    assert!(!s.is_enabled("late.sneaky").unwrap());
    assert!(s.is_enabled("sim.hurt").unwrap() && s.is_enabled("sim.step").unwrap());
}

#[test]
fn a_compile_error_is_its_file_and_line_and_the_compilers_words() {
    let src = "use argus_plugin::prelude::*;\n\nfn f(_: &Ctx) -> Result {\n    let n: u32 = \"three\";\n    let _ = n;\n    Ok(())\n}\n\nplugin! { components: [], systems: [System::new(\"sim.f\", \"sim\").run(f)] }\n";
    plugin("broken", src);
    let b = build(project(), "broken", true);
    assert!(b.dll.is_none());
    let e = &b.errors("broken")[0];
    assert_eq!(e.file, "plugins/broken/src/lib.rs");
    assert_eq!(e.line, Some(4));
    assert_eq!(e.phase, "compile");
    assert!(e.message.contains("mismatched types"), "{e:?}");
    assert!(
        b.diagnostics[0].rendered.contains("expected `u32`"),
        "{:?}",
        b.diagnostics[0]
    );

    // And a world that lists it runs without it.
    let mut w = world();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    let p = load_all(
        project(),
        &["broken".into(), "missing".into()],
        &Source::default(),
        &mut w,
        &mut s,
    );
    assert!(p.loaded.is_empty());
    assert_eq!(p.errors[0].at(), "plugins/broken/src/lib.rs:4");
    assert!(
        p.errors
            .iter()
            .any(|e| e.file == "plugins/missing/Cargo.toml"),
        "{:?}",
        p.errors
    );
    s.step(&mut w, 1).unwrap();
}

#[test]
fn a_layout_the_game_does_not_have_is_refused_at_load() {
    let src = "use argus_plugin::prelude::*;\n\ncomponent! { pub struct Hp { pub cur: f32 } }\n\nfn f(ctx: &Ctx) -> Result { ctx.each::<(&mut Hp,)>(|_, _| {}) }\n\nplugin! { components: [Hp], systems: [System::new(\"sim.skewed\", \"sim\").writes(&[\"Hp\", \"Nope\"]).run(f)] }\n";
    plugin("skewed", src);
    let mut w = world();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    let p = load_all(
        project(),
        &["skewed".into()],
        &Source::default(),
        &mut w,
        &mut s,
    );
    let layout = p
        .errors
        .iter()
        .find(|e| e.code == argus_plugins::codes::LAYOUT)
        .expect("layout");
    assert_eq!(layout.at(), "plugins/skewed/src/lib.rs:3");
    assert!(
        layout.message.contains("cur i32 at 0") && layout.message.contains("cur f32 at 0"),
        "{layout:?}"
    );
    assert!(p.errors.iter().any(|e| e
        .message
        .contains("writes Nope, which the game does not declare")));
    assert!(s.source_of("sim.skewed").is_none(), "nothing installed");
}

fn live_src(add: i32) -> String {
    format!(
        "use argus_plugin::prelude::*;\n\ncomponent! {{ pub struct Hp {{ pub cur: i32 }} }}\n\nfn grow(ctx: &Ctx) -> Result {{ ctx.each::<(&mut Hp,)>(|_, (h,)| h.cur += {add}) }}\n\nplugin! {{ components: [Hp], systems: [System::new(\"sim.grow\", \"sim\").writes(&[\"Hp\"]).run(grow)] }}\n"
    )
}

#[test]
fn reload_swaps_the_library_and_the_run_continues() {
    plugin("live", &live_src(1));
    let names = vec!["live".to_string()];
    let mut w = world();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    let mut p: Plugins = load_all(project(), &names, &Source::default(), &mut w, &mut s);
    assert!(p.errors.is_empty(), "{:?}", p.errors);
    s.play(&w);
    s.step(&mut w, 3).unwrap();
    let e = w.query().has(w.component_id("Hp").unwrap()).entities()[0];
    assert_eq!(w.get_field(e, "Hp.cur").unwrap(), Value::Int(5));
    let first = p.loaded[0].loaded_from().to_path_buf();

    // Nothing changed: nothing swapped.
    let r = reload(project(), &names, &mut p, &mut w, &mut s);
    assert!(!r[0].swapped);

    plugin("live", &live_src(10));
    let digest = w.digest();
    let tick = s.tick();
    let r = reload(project(), &names, &mut p, &mut w, &mut s);
    assert!(r[0].swapped, "{r:?} {:?}", p.errors);
    assert!(p.errors.is_empty(), "{:?}", p.errors);
    assert!(r[0].reload_ms < 2000.0, "{r:?}");
    eprintln!(
        "plugin rebuilt in {} ms, reloaded in {:.1} ms",
        r[0].build_ms, r[0].reload_ms
    );
    assert_eq!(w.digest(), digest, "the world is the one before the swap");
    assert_eq!(s.tick(), tick, "and the clock");
    assert!(!first.exists(), "the old library's copy went with it");
    s.step(&mut w, 1).unwrap();
    assert_eq!(
        w.get_field(e, "Hp.cur").unwrap(),
        Value::Int(15),
        "the new code runs"
    );

    // A build that fails keeps the old library running, and says why.
    std::fs::write(
        project().join("plugins/live/src/lib.rs"),
        live_src(10).replace("h.cur += 10", "h.cur += \"ten\""),
    )
    .unwrap();
    let r = reload(project(), &names, &mut p, &mut w, &mut s);
    assert!(!r[0].swapped);
    assert_eq!(p.errors[0].phase, "compile");
    s.step(&mut w, 1).unwrap();
    assert_eq!(w.get_field(e, "Hp.cur").unwrap(), Value::Int(25));
    plugin("live", &live_src(1));
}

const PAR: &str = r#"use argus_plugin::prelude::*;

component! { pub struct Pos { pub p: Vec2 } }
component! { pub struct Vel { pub v: Vec2 } }
component! { pub struct Hp { pub cur: i32 } }

fn fold(x: f32) -> f32 {
    let mut h = x;
    for _ in 0..200 { h = (h * 1.000_1).sin() + 0.5; }
    h
}

fn moves(ctx: &Ctx) -> Result {
    ctx.each::<(&Vel, &mut Pos)>(|_, (v, p)| p.p.x = fold(p.p.x + v.v.x))
}

fn heals(ctx: &Ctx) -> Result {
    let roll = ctx.rng().below(3)? as i32;
    ctx.each::<(&mut Hp,)>(|_, (h,)| h.cur = (fold(h.cur as f32) * 100.0) as i32 + roll)?;
    if roll == 0 { ctx.emit("rolled.zero", &[])?; }
    Ok(())
}

plugin! {
    components: [Pos, Vel, Hp],
    systems: [
        System::new("sim.moves", "sim").reads(&["Vel"]).writes(&["Pos"]).parallel().run(moves),
        System::new("sim.heals", "sim").writes(&["Hp"]).rng("heals").parallel().run(heals),
    ],
}
"#;

#[test]
fn parallel_plugin_systems_share_a_batch_and_reach_the_same_world_on_any_threads() {
    plugin("par", PAR);
    let run = |threads: usize| {
        let mut w = world();
        let mut s = Scheduler::new(SchedulerConfig {
            threads,
            ..SchedulerConfig::continuous(60.0)
        });
        s.set_parallel_floor(std::time::Duration::ZERO);
        let p = load_all(
            project(),
            &["par".into()],
            &Source::default(),
            &mut w,
            &mut s,
        );
        assert!(p.errors.is_empty(), "{:?}", p.errors);
        s.play(&w);
        let mut digests = Vec::new();
        for _ in 0..20 {
            s.step(&mut w, 1).unwrap();
            digests.push(w.digest());
        }
        let b = s.batches().unwrap();
        assert_eq!(b[0].systems, vec!["sim.moves", "sim.heals"]);
        let events: Vec<String> = w
            .events()
            .iter()
            .map(|r| format!("{:?}", r.event))
            .collect();
        (digests, events, b[0].threaded)
    };
    let one = run(1);
    let two = run(2);
    assert!(!one.2 && two.2);
    assert_eq!(one.0, two.0);
    assert_eq!(one.1, two.1);
}
