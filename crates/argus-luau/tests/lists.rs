//! List fields in scripts (issue 22): what a declaration may say, and what
//! assigning one takes.

use argus_ecs::{Event, Value, World};
use argus_luau::{ScriptConfig, ScriptHost};
use argus_sim::{Scheduler, SchedulerConfig};

fn host(src: &str) -> (ScriptHost, World, Scheduler, bool) {
    let mut h = ScriptHost::new(ScriptConfig::default()).unwrap();
    let mut w = World::new();
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    let ok = h.load_bytes(&mut w, &mut s, "components/tile.luau", src.as_bytes());
    (h, w, s, ok)
}

fn said(h: &ScriptHost) -> String {
    h.errors()
        .iter()
        .map(|e| e.message.clone())
        .collect::<Vec<_>>()
        .join(" | ")
}

#[test]
fn a_list_declaration_says_what_it_holds_and_nothing_a_list_cannot_take() {
    for (src, want) in [
        ("component \"T\" { on = field.list {} }", "it needs of"),
        (
            "component \"T\" { on = field.list { of = \"entity\" } }",
            "not a type a list holds",
        ),
        (
            "component \"T\" { on = field.list { of = \"i32\", range = {0, 3} } }",
            "a list takes no range",
        ),
        (
            "component \"T\" { on = field.list { of = \"f32\", tune = true } }",
            "a list takes no tune",
        ),
        (
            "component \"T\" { on = field.list { of = \"u8\", default = { 1, 300 } } }",
            "element 2",
        ),
    ] {
        let (h, _, _, ok) = host(src);
        assert!(!ok, "{src}");
        assert!(said(&h).contains(want), "{src}: {}", said(&h));
    }
    let (h, w, _, ok) = host(
        "component \"T\" { on = field.list { of = \"str\", default = { \"dry\" }, doc = \"What is on it\" } }",
    );
    assert!(ok, "{}", said(&h));
    let f = w.registry().resolve_path("T.on").unwrap().1;
    assert_eq!(f.ty.name(), "list of str");
    assert_eq!(f.default, Value::List(vec![Value::Str("dry".into())]));
}

#[test]
fn a_list_is_assigned_whole_from_an_array() {
    let (h, mut w, mut s, ok) = host(
        r#"return {
  component "T" { on = field.list { of = "str" }, n = field.list { of = "i32" } },
  system "sim.t" {
    stage = "sim", writes = { "T" },
    run = function(q)
      for e, t in q:each("T") do
        t.on = { "oiled", "burning" }
        t.n = {}
        local bad = { pcall(function() t.on = { x = 1 } end) }
        local mixed = { pcall(function() t.n = { 1, "two" } end) }
        q:emit("tried", { keys = tostring(bad[2]), mixed = tostring(mixed[2]), kept = t.on })
      end
    end,
  },
}"#,
    );
    assert!(ok, "{}", said(&h));
    let e = w.spawn();
    w.insert_by_name(e, "T").unwrap();
    s.step(&mut w, 1).unwrap();
    let ev = w
        .events()
        .iter()
        .find_map(|r| match &r.event {
            Event::Game { name, fields } if name == "tried" => Some(fields.clone()),
            _ => None,
        })
        .expect("the system ran");
    let get = |k: &str| ev.iter().find(|(n, _)| n == k).unwrap().1.clone();
    let Value::Str(keys) = get("keys") else {
        panic!()
    };
    assert!(keys.contains("an array"), "{keys}");
    let Value::Str(mixed) = get("mixed") else {
        panic!()
    };
    assert!(mixed.contains("element 2"), "{mixed}");
    assert_eq!(
        get("kept"),
        Value::List(vec![
            Value::Str("oiled".into()),
            Value::Str("burning".into())
        ]),
        "a refused assignment leaves the list as it was"
    );
    assert_eq!(w.get_field(e, "T.n").unwrap(), Value::List(vec![]));
}
