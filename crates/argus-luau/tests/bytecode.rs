//! §15 "Luau precompiled to bytecode" (phase 2 item 7): a compiled script
//! loads and runs exactly as its source does, a syntax error is caught at
//! compile time with its line, and a run-time error in bytecode still names
//! the file and line.

use argus_ecs::{Value, World};
use argus_luau::{compile, ErrorPhase, ScriptConfig, ScriptHost};
use argus_sim::{Scheduler, SchedulerConfig};

const SRC: &str = r#"
component "Count" { n = field.i32 { default = 0 } }
return system "sim.count" {
  stage = "sim", writes = { "Count" },
  run = function(q)
    for e, c in q:each("Count") do
      c.n += 1
      if c.n == 4 then
        error("four is too many")
      end
    end
  end,
}
"#;

fn run(bytes: &[u8]) -> (World, ScriptHost, Vec<i64>) {
    let mut h = ScriptHost::new(ScriptConfig::default()).unwrap();
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0).seed(1));
    assert!(
        h.load_bytes(&mut w, &mut s, "systems/count.luau", bytes),
        "{:?}",
        h.errors()
    );
    let e = w.spawn();
    w.insert_by_name(e, "Count").unwrap();
    let mut seen = Vec::new();
    for _ in 0..5 {
        s.step(&mut w, 1).unwrap();
        match w.get_field(e, "Count.n").unwrap() {
            Value::Int(n) => seen.push(n),
            v => panic!("{v:?}"),
        }
    }
    (w, h, seen)
}

#[test]
fn bytecode_runs_as_its_source_does_and_keeps_its_lines() {
    let bytes = compile("systems/count.luau", SRC).unwrap();
    assert!(
        bytes[0] < b'\n',
        "Luau bytecode starts with its version byte"
    );
    let (_, _, from_source) = run(SRC.as_bytes());
    let (w, _, from_bytes) = run(&bytes);
    assert_eq!(from_bytes, from_source);
    // The error on the fourth tick stopped the system in both, at line 9.
    assert_eq!(from_bytes, [1, 2, 3, 4, 4]);
    let fault = w
        .events()
        .iter()
        .find_map(|r| match &r.event {
            argus_ecs::Event::SystemFault { message, r#ref, .. } => {
                Some(format!("{} {:?}", message, r#ref))
            }
            _ => None,
        })
        .expect("a fault");
    assert!(
        fault.contains("count.luau:9") && fault.contains("four is too many"),
        "{fault}"
    );
}

#[test]
fn a_syntax_error_is_caught_when_compiling_with_its_line() {
    let e = compile(
        "systems/bad.luau",
        "local a = 1\nlocal b = 2\nlocal c = = 3\n",
    )
    .unwrap_err();
    assert_eq!(e.file, "systems/bad.luau");
    assert_eq!(e.line, Some(3), "{e:?}");
    assert_eq!(e.phase, ErrorPhase::Syntax);
    assert!(e.message.to_lowercase().contains("expected"), "{e:?}");
}
