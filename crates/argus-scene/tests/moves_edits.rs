//! `asset.move` with the moves log, and file edits: overrides written back
//! without disturbing the file, prefab changes reapplied to instances (§3, §4, §5).

mod common;

use argus_ecs::Value;
use argus_scene::{codes, EntityDoc, MoveKind, Scene};
use common::*;

#[test]
fn moving_a_prefab_rewrites_every_reference_and_logs_it() {
    let t = temp("move-prefab");
    project(t.path());
    let mut p = open(t.path());
    let r = p
        .move_asset("prefabs/door/locked.toml", "prefabs/door/bolted.toml")
        .unwrap();
    assert_eq!(r.record.kind, MoveKind::Prefab);
    assert_eq!(r.record.from, "door/locked");
    assert_eq!(r.record.to, "door/bolted");
    assert_eq!(
        r.rewritten,
        ["scenes/hollow/rooms/cavern-03/door.east.toml"]
    );
    assert!(t.path().join("prefabs/door/bolted.toml").exists());
    assert!(!t.path().join("prefabs/door/locked.toml").exists());
    assert!(p.prefabs.get("door/bolted").is_some());
    assert!(p.prefabs.get("door/locked").is_none());

    let east = read(t.path(), "scenes/hollow/rooms/cavern-03/door.east.toml");
    assert!(east.starts_with("prefab = \"door/bolted\"\n"), "{east}");
    assert!(
        east.contains("# entity ref by string id, room-qualified"),
        "comments kept: {east}"
    );

    let log = read(t.path(), ".argus/moves.log");
    let line = log.lines().next().unwrap();
    assert!(
        line.contains("\tprefab\tdoor/locked\tdoor/bolted\t1"),
        "{line}"
    );
    assert_eq!(p.moves.len(), 1);

    let w = world();
    let s = Scene::open(&p, "hollow", w.registry()).unwrap();
    assert!(s.issues().is_empty(), "{:?}", s.issues());
    assert_eq!(
        s.resolved("cavern-03/door.east").unwrap().chain,
        ["door/bolted", "door/base"]
    );

    // Moving onto an existing file is refused; nothing changes.
    let e = p
        .move_asset("prefabs/door/base.toml", "prefabs/door/bolted.toml")
        .unwrap_err();
    assert_eq!(e.code, codes::BAD_MOVE);
    assert!(t.path().join("prefabs/door/base.toml").exists());
}

#[test]
fn an_outside_rename_leaves_a_missing_reference_naming_the_old_path() {
    let t = temp("outside-rename");
    project(t.path());
    std::fs::rename(
        t.path().join("prefabs/door/locked.toml"),
        t.path().join("prefabs/door/bolted.toml"),
    )
    .unwrap();
    let p = open(t.path());
    let w = world();
    let s = Scene::open(&p, "hollow", w.registry()).unwrap();
    let e = &s.issues()[0];
    assert_eq!(e.code, codes::MISSING_REFERENCE);
    assert_eq!(
        e.message,
        "missing reference: prefab \"door/locked\" does not exist"
    );
    assert_eq!(
        e.r#ref.as_deref(),
        Some("scenes/hollow/rooms/cavern-03/door.east.toml:1")
    );
    assert!(e.hint.contains("asset.move"), "{}", e.hint);

    // An out-of-date client that missed a logged move is told where it went.
    write(
        t.path(),
        ".argus/moves.log",
        "1700000000\tprefab\tdoor/locked\tdoor/bolted\t0\n",
    );
    let p = open(t.path());
    let s = Scene::open(&p, "hollow", w.registry()).unwrap();
    let e = &s.issues()[0];
    assert!(e.hint.contains("moved to \"door/bolted\""), "{}", e.hint);
    assert_eq!(p.moves[0].to, "door/bolted");
}

#[test]
fn moving_an_entity_or_a_def_row_rewrites_refs_in_place() {
    let t = temp("move-entity");
    project(t.path());
    let mut p = open(t.path());
    let r = p
        .move_asset(
            "scenes/hollow/rooms/cavern-04/door.west.toml",
            "scenes/hollow/rooms/cavern-04/door.w.toml",
        )
        .unwrap();
    assert_eq!(r.record.kind, MoveKind::Entity);
    assert_eq!(
        r.rewritten,
        ["scenes/hollow/rooms/cavern-03/door.east.toml"]
    );
    let east = read(t.path(), "scenes/hollow/rooms/cavern-03/door.east.toml");
    assert!(
        east.contains("leads_to = \"@cavern-04/door.w\"   # entity ref"),
        "{east}"
    );

    let r = p
        .move_asset("defs/items.toml#key.brass", "#key.iron")
        .unwrap();
    assert_eq!(r.record.kind, MoveKind::Def);
    assert_eq!(r.record.from, "key.brass");
    let mut rewritten = r.rewritten.clone();
    rewritten.sort();
    assert_eq!(
        rewritten,
        [
            "prefabs/door/locked.toml",
            "scenes/hollow/rooms/cavern-03/door.east.toml"
        ]
    );
    let items = read(t.path(), "defs/items.toml");
    assert!(items.contains("[\"key.iron\"]"), "{items}");
    assert!(!items.contains("key.brass"), "{items}");
    assert!(p.defs.resolve("item:key.iron").is_some());
    let east = read(t.path(), "scenes/hollow/rooms/cavern-03/door.east.toml");
    assert!(
        east.contains("key      = \"item:key.iron\"         # def ref"),
        "{east}"
    );

    let w = world();
    let s = Scene::open(&p, "hollow", w.registry()).unwrap();
    assert!(s.issues().is_empty(), "{:?}", s.issues());
    assert_eq!(p.moves.len(), 2);

    // Entities cannot change scene.
    let e = p
        .move_asset(
            "scenes/hollow/rooms/cavern-03/save-point.toml",
            "scenes/arena/save-point.toml",
        )
        .unwrap_err();
    assert_eq!(e.code, codes::BAD_MOVE);
    let e = p.move_asset("README.md", "docs/README.md").unwrap_err();
    assert_eq!(e.code, codes::BAD_MOVE);
}

#[test]
fn overrides_are_written_back_without_disturbing_the_file() {
    let t = temp("edits");
    project(t.path());
    let p = open(t.path());
    let mut w = world();
    let mut s = Scene::open(&p, "arena", w.registry()).unwrap();
    s.spawn_all(&mut w).unwrap();
    let player = w.entity("@player").unwrap();

    // edit.set → commit writes the override.
    s.set_override(
        &p.ctx(w.registry()),
        "@player",
        "Health.cur",
        &Value::Int(5),
    )
    .unwrap();
    let text = read(t.path(), "scenes/arena/player.toml");
    assert!(text.starts_with("# the player\n"), "{text}");
    assert!(
        text.contains("cur = 5 # hp\n"),
        "value replaced, comment kept: {text}"
    );
    assert_eq!(
        s.resolved("player")
            .unwrap()
            .field("Health.cur")
            .unwrap()
            .value,
        Value::Int(5)
    );
    // The live world is synced on request, not implicitly.
    assert_eq!(w.get::<Health>(player).unwrap().cur, 10);
    assert!(s.sync_entity(&mut w, "player").unwrap());
    assert_eq!(w.get::<Health>(player).unwrap().cur, 5);

    // Range errors come from the schema, with its ref.
    let e = s
        .set_override(
            &p.ctx(w.registry()),
            "@player",
            "Health.cur",
            &Value::Int(5000),
        )
        .unwrap_err();
    assert_eq!(e.code, argus_ecs::codes::OUT_OF_RANGE);

    // Adding a component writes an empty table; clearing a field keeps the table.
    s.add_component(&p.ctx(w.registry()), "player", "Light")
        .unwrap();
    s.set_override(
        &p.ctx(w.registry()),
        "player",
        "Light.intensity",
        &Value::Float(2.5),
    )
    .unwrap();
    s.clear_override(&p.ctx(w.registry()), "player", "Light.intensity")
        .unwrap();
    let text = read(t.path(), "scenes/arena/player.toml");
    assert!(text.contains("[Light]\n"), "{text}");
    assert!(!text.contains("intensity"), "{text}");
    s.sync_entity(&mut w, "player").unwrap();
    assert!(w.has_type::<Light>(player));
    assert_eq!(w.get::<Light>(player).unwrap().intensity, 0.0);
    s.remove_component(&p.ctx(w.registry()), "player", "Light")
        .unwrap();
    assert!(!read(t.path(), "scenes/arena/player.toml").contains("Light"));
    s.sync_entity(&mut w, "player").unwrap();
    assert!(!w.has_type::<Light>(player));

    // Entity refs are written as @id.
    s.set_override(
        &p.ctx(w.registry()),
        "spawner.north",
        "Door.leads_to",
        &Value::Str("@player".into()),
    )
    .unwrap();
    let e = s
        .set_override(
            &p.ctx(w.registry()),
            "spawner.north",
            "Door.leads_to",
            &Value::Entity(player),
        )
        .unwrap_err();
    assert_eq!(e.code, codes::NOT_AUTHORED);

    // New and deleted entities.
    let doc = EntityDoc {
        prefab: Some("enemy/bat".into()),
        ..Default::default()
    };
    let id = s
        .create_entity(&p.ctx(w.registry()), "bat.1", None, &doc)
        .unwrap();
    assert_eq!(id, "bat.1");
    assert_eq!(
        read(t.path(), "scenes/arena/bat.1.toml"),
        "prefab = \"enemy/bat\"\n"
    );
    assert_eq!(s.resolved("bat.1").unwrap().chain, ["enemy/bat"]);
    let e = s
        .create_entity(&p.ctx(w.registry()), "bat.1", None, &doc)
        .unwrap_err();
    assert_eq!(e.code, argus_ecs::codes::DUPLICATE_ID);
    s.delete_entity(&mut w, "bat.1").unwrap();
    assert!(!t.path().join("scenes/arena/bat.1.toml").exists());
    assert!(s.resolved("bat.1").is_none());
    assert!(s.issues().is_empty(), "{:?}", s.issues());
}

#[test]
fn a_prefab_change_touches_its_instances() {
    let t = temp("prefab-change");
    project(t.path());
    let mut p = open(t.path());
    let mut w = world();
    let mut s = Scene::open(&p, "hollow", w.registry()).unwrap();
    s.spawn_all(&mut w).unwrap();
    assert_eq!(
        s.instances_of("door/base"),
        ["cavern-03/door.east", "cavern-04/door.west"]
    );
    assert_eq!(s.instances_of("door/locked"), ["cavern-03/door.east"]);

    // Edit the base prefab's child light and its Door default.
    let text =
        read(t.path(), "prefabs/door/base.toml").replace("intensity = 1.0", "intensity = 7.0");
    write(
        t.path(),
        "prefabs/door/base.toml",
        &format!("{text}\n[Health]\ncur = 9\n"),
    );
    p.reload_prefabs();
    let touched = s.reapply_prefab(&p, &mut w, "door/base").unwrap();
    // Two instances; three entities touched, because the lights inherit too.
    assert_eq!(
        touched,
        [
            "cavern-03/door.east",
            "cavern-03/door.east/light",
            "cavern-04/door.west",
            "cavern-04/door.west/light"
        ]
    );

    let el = w.entity("@cavern-03/door.east/light").unwrap();
    assert_eq!(
        w.get::<Light>(el).unwrap().intensity,
        7.0,
        "inherited value re-inherited"
    );
    let wl = w.entity("@cavern-04/door.west/light").unwrap();
    assert_eq!(
        w.get::<Light>(wl).unwrap().intensity,
        3.0,
        "own override wins"
    );
    let east = w.entity("@cavern-03/door.east").unwrap();
    assert_eq!(
        w.get::<Health>(east).unwrap().cur,
        9,
        "component added by the prefab"
    );
    let west = w.entity("@cavern-04/door.west").unwrap();
    assert_eq!(
        w.get::<Door>(west).unwrap().leads_to,
        east,
        "refs survive a resync"
    );
    assert!(s.issues().is_empty(), "{:?}", s.issues());
    assert_eq!(
        p.prefabs.dependents("door/base"),
        ["door/base", "door/locked"]
    );
}
