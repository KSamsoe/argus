//! Entity files and prefab resolution with provenance (§3, §4).

mod common;

use argus_ecs::{codes as ecs, Value};
use argus_scene::{codes, EntityDoc, Provenance, Scene};
use common::*;

#[test]
fn the_spec_entity_file_parses_into_overrides_only() {
    let text = r#"prefab = "door/locked"
tags   = ["door"]

[Transform]
pos = [46.0, 8.0]

[Door]
leads_to = "@cavern-04/door.west"   # entity ref by string id, room-qualified
key      = "item:key.brass"         # def ref, not a GUID
locked   = true
"#;
    let d = EntityDoc::parse("scenes/hollow/rooms/cavern-03/door.east.toml", text).unwrap();
    assert_eq!(d.prefab.as_deref(), Some("door/locked"));
    assert_eq!(d.tags, ["door"]);
    let names: Vec<&str> = d.components.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Transform", "Door"]);
    let door = d.component("Door").unwrap();
    assert_eq!(door.fields[0].name, "leads_to");
    assert_eq!(door.fields[0].raw.as_str(), Some("@cavern-04/door.west"));
    assert_eq!(
        door.fields[0].at,
        "scenes/hollow/rooms/cavern-03/door.east.toml:8"
    );
    assert_eq!(door.at, "scenes/hollow/rooms/cavern-03/door.east.toml:7");
}

#[test]
fn removals_children_and_bad_keys() {
    let d = EntityDoc::parse(
        "x.toml",
        "-Door = true\n-tags = [\"decoy\"]\n-children = [\"light\"]\n\n[children.lamp.Light]\nintensity = 2.0\n",
    )
    .unwrap();
    assert_eq!(d.removed, ["Door"]);
    assert_eq!(d.removed_tags, ["decoy"]);
    assert_eq!(d.removed_children, ["light"]);
    assert_eq!(d.children.len(), 1);
    assert_eq!(d.children[0].0, "lamp");
    assert!(d.child_path("lamp").unwrap().component("Light").is_some());

    let e = EntityDoc::parse("x.toml", "name = \"door\"\n\n[Door]\n").unwrap_err();
    assert_eq!(e.code, codes::BAD_SCENE);
    assert_eq!(e.r#ref.as_deref(), Some("x.toml:1"));
    assert!(e.message.contains("unexpected key"), "{e}");

    let e = EntityDoc::parse("x.toml", "tags = []\n\n-Door = false\n\n[Door]\n").unwrap_err();
    assert_eq!(e.code, codes::BAD_SCENE);
    assert_eq!(e.r#ref.as_deref(), Some("x.toml:3"));

    let e = EntityDoc::parse("x.toml", "[Door]\nlocked = tru\n").unwrap_err();
    assert_eq!(e.code, codes::BAD_TOML);
    assert_eq!(e.r#ref.as_deref(), Some("x.toml:2"));
}

#[test]
fn canonical_text_round_trips() {
    let text = "prefab = \"door/base\"\ntags = [\"door\"]\n-Light = true\n\n[Transform]\npos = [1.0, 2.0]\n\n[Door]\n\n[children.light.Light]\nintensity = 2.0\n";
    let d = EntityDoc::parse("a.toml", text).unwrap();
    let out = d.to_toml_string().unwrap();
    let d2 = EntityDoc::parse("a.toml", &out).unwrap();
    assert_eq!(d2.prefab, d.prefab);
    assert_eq!(d2.tags, d.tags);
    assert_eq!(d2.removed, d.removed);
    assert_eq!(d2.components.len(), 2);
    assert_eq!(d2.component("Door").unwrap().fields.len(), 0);
    assert_eq!(
        d2.child("light")
            .unwrap()
            .component("Light")
            .unwrap()
            .fields[0]
            .raw
            .as_float(),
        Some(2.0)
    );
    assert!(
        out.contains("[Door]"),
        "empty component table survives: {out}"
    );
}

#[test]
fn prefab_chain_gives_every_field_a_provenance() {
    let t = temp("prov");
    project(t.path());
    let p = open(t.path());
    let w = world();
    let s = Scene::open(&p, "hollow", w.registry()).unwrap();
    assert!(s.issues().is_empty(), "{:?}", s.issues());

    let east = s.resolved("cavern-03/door.east").unwrap();
    assert_eq!(east.chain, ["door/locked", "door/base"]);
    assert_eq!(east.field("Door.locked").unwrap().from, Provenance::Own);
    assert_eq!(
        east.field("Door.key").unwrap().value,
        Value::Ref("item:key.brass".into())
    );
    assert_eq!(east.field("Transform.pos").unwrap().from, Provenance::Own);
    // Transform.rot was set nowhere: schema default.
    assert_eq!(
        east.field("Transform.rot").unwrap().from,
        Provenance::Default
    );
    // The tag came from the base first; the own file repeating it changes nothing.
    assert_eq!(
        east.tags[0],
        ("door".to_string(), Provenance::Prefab("door/base".into()))
    );
    // The child from door/base, with its own id.
    assert_eq!(east.children.len(), 1);
    let light = &east.children[0];
    assert_eq!(light.id, "cavern-03/door.east/light");
    assert_eq!(
        light.field("Light.intensity").unwrap().from,
        Provenance::Prefab("door/base".into())
    );
    assert_eq!(light.field("Light.on").unwrap().from, Provenance::Default);

    // door.west: base only, child field overridden in its own file.
    let west = s.resolved("cavern-04/door.west").unwrap();
    assert_eq!(west.field("Door.locked").unwrap().value, Value::Bool(false));
    assert_eq!(
        west.field("Door.locked").unwrap().from,
        Provenance::Prefab("door/base".into())
    );
    let wl = s.resolved("cavern-04/door.west/light").unwrap();
    assert_eq!(
        wl.field("Light.intensity").unwrap().value,
        Value::Float(3.0)
    );
    assert_eq!(wl.field("Light.intensity").unwrap().from, Provenance::Own);
    assert_eq!(format!("{}", Provenance::Own), "own");
    assert_eq!(
        format!("{}", Provenance::Prefab("door/base".into())),
        "door/base"
    );

    // resolved: false is just the file.
    let ov = s.overrides("cavern-04/door.west/light").unwrap();
    assert_eq!(ov.components.len(), 1);
    assert_eq!(ov.components[0].fields.len(), 1);
}

#[test]
fn inheritance_is_per_field_and_removal_is_explicit() {
    let t = temp("perfield");
    project(t.path());
    write(
        t.path(),
        "scenes/arena/lockbox.toml",
        "prefab = \"door/locked\"\n-tags = [\"door\"]\n-children = [\"light\"]\n\n[Door]\nlocked = false\n",
    );
    write(
        t.path(),
        "scenes/arena/frame.toml",
        "prefab = \"door/locked\"\n-Door = true\n\n[Health]\n",
    );
    let p = open(t.path());
    let w = world();
    let s = Scene::open(&p, "arena", w.registry()).unwrap();
    assert!(s.issues().is_empty(), "{:?}", s.issues());

    let lb = s.resolved("lockbox").unwrap();
    // Overriding Door.locked did not detach Door.key from door/locked.
    assert_eq!(lb.field("Door.locked").unwrap().value, Value::Bool(false));
    assert_eq!(lb.field("Door.locked").unwrap().from, Provenance::Own);
    assert_eq!(
        lb.field("Door.key").unwrap().from,
        Provenance::Prefab("door/locked".into())
    );
    assert!(lb.tags.is_empty());
    assert!(lb.children.is_empty());

    let fr = s.resolved("frame").unwrap();
    assert!(fr.component("Door").is_none());
    let h = fr.component("Health").unwrap();
    assert_eq!(h.from, Provenance::Own);
    assert!(h.fields.iter().all(|f| f.from == Provenance::Default));
    assert_eq!(fr.children.len(), 1, "children still inherited");
}

#[test]
fn cycles_and_missing_prefabs_are_reported_with_the_path() {
    let t = temp("cycle");
    project(t.path());
    write(t.path(), "prefabs/a.toml", "prefab = \"b\"\n");
    write(t.path(), "prefabs/b.toml", "prefab = \"a\"\n");
    write(t.path(), "scenes/arena/x.toml", "prefab = \"a\"\n");
    write(
        t.path(),
        "scenes/arena/y.toml",
        "prefab = \"door/vanished\"\n",
    );
    let p = open(t.path());
    let w = world();
    let s = Scene::open(&p, "arena", w.registry()).unwrap();
    let cyc = s
        .issues()
        .iter()
        .find(|e| e.code == codes::PREFAB_CYCLE)
        .unwrap();
    assert!(cyc.message.contains("a -> b -> a"), "{cyc}");
    let miss = s
        .issues()
        .iter()
        .find(|e| e.code == codes::MISSING_REFERENCE)
        .unwrap();
    assert!(miss.message.contains("\"door/vanished\""), "{miss}");
    assert_eq!(miss.r#ref.as_deref(), Some("scenes/arena/y.toml:1"));
    assert!(miss.hint.contains("asset.move"), "{}", miss.hint);

    let err = s.instances_of("a");
    assert!(err.is_empty());
}

#[test]
fn every_problem_in_an_entity_file_is_an_issue_not_only_the_first() {
    // Scene issues showed the first problem a file had; fix it, reload,
    // and the next came up.
    let t = temp("allproblems");
    project(t.path());
    write(
        t.path(),
        "scenes/arena/a.toml",
        "[Light]\nbrightness = 1\nintensity = 50.0\n\n[Nope]\nx = 1\n",
    );
    let p = open(t.path());
    let w = world();
    let s = Scene::open(&p, "arena", w.registry()).unwrap();
    let mut at: Vec<(u32, String)> = s
        .issues()
        .iter()
        .filter(|e| {
            e.r#ref.as_deref().is_some_and(|r| r.contains("a.toml")) || e.message.contains("a.toml")
        })
        .map(|e| (e.code, e.r#ref.clone().unwrap_or_default()))
        .collect();
    at.sort();
    let codes_seen: Vec<u32> = at.iter().map(|x| x.0).collect();
    assert!(codes_seen.contains(&ecs::UNKNOWN_FIELD), "{at:?}");
    assert!(codes_seen.contains(&ecs::OUT_OF_RANGE), "{at:?}");
    assert!(codes_seen.contains(&ecs::UNKNOWN_COMPONENT), "{at:?}");
    assert!(
        at.iter().any(|x| x.1 == "scenes/arena/a.toml:5"),
        "the unknown component at its own line: {at:?}"
    );
}

#[test]
fn schema_errors_carry_both_locations() {
    let t = temp("schemaerr");
    project(t.path());
    write(t.path(), "scenes/arena/a.toml", "[Nope]\nx = 1\n");
    write(t.path(), "scenes/arena/b.toml", "[Light]\nbrightness = 1\n");
    write(
        t.path(),
        "scenes/arena/c.toml",
        "[Light]\nintensity = 50.0\n",
    );
    write(t.path(), "scenes/arena/d.toml", "[Health]\ncur = 1.5\n");
    let p = open(t.path());
    let w = world();
    let s = Scene::open(&p, "arena", w.registry()).unwrap();
    let by = |code: u32| s.issues().iter().find(|e| e.code == code).cloned().unwrap();
    let a = by(ecs::UNKNOWN_COMPONENT);
    assert_eq!(a.r#ref.as_deref(), Some("scenes/arena/a.toml:1"));
    let b = by(ecs::UNKNOWN_FIELD);
    assert_eq!(b.r#ref.as_deref(), Some("scenes/arena/b.toml:2"));
    assert!(b.hint.contains("intensity, on"), "{}", b.hint);
    let c = by(ecs::OUT_OF_RANGE);
    assert!(
        c.message
            .contains("Light.intensity must be in range [0, 10]; got 50"),
        "{c}"
    );
    assert!(c.message.contains("(in scenes/arena/c.toml:2)"), "{c}");
    assert!(
        c.r#ref.as_deref().unwrap().contains("common"),
        "schema ref kept: {:?}",
        c.r#ref
    );
    let d = by(ecs::TYPE_MISMATCH);
    assert_eq!(d.r#ref.as_deref(), Some("scenes/arena/d.toml:2"));

    let e = s.issues().len();
    assert_eq!(e, 4);
    let mut w2 = world();
    let mut s2 = s;
    let err = s2.spawn_all(&mut w2).unwrap_err();
    assert_eq!(err.code, codes::SCENE_NOT_CLEAN);
    assert!(err.message.contains("4 issue(s)"), "{err}");
}
