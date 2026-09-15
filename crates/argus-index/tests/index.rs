//! The index: sync from a project, reads with the protocol shapes,
//! references, search, staleness, incremental updates (§4 "The index").

mod common;

use argus_index::{codes, Index};
use common::*;
use serde_json::json;

fn synced(t: &Temp) -> (Index, argus_ecs::World, argus_scene::Project) {
    project(t.path());
    let p = open(t.path());
    let w = world();
    let mut idx = Index::open(t.path()).unwrap();
    let r = idx
        .sync_project(&p, w.registry(), &systems(&w), None, false)
        .unwrap();
    assert_eq!(r.scenes, ["arena", "hollow"]);
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    (idx, w, p)
}

#[test]
fn sync_indexes_every_scene_prefab_def_schema_and_system() {
    let t = temp("sync");
    let (idx, _, _) = synced(&t);
    assert!(Index::path(t.path()).exists());
    let st = idx.stats().unwrap();
    assert_eq!(st["scenes"], 2);
    assert_eq!(st["rooms"], 3, "two rooms plus the flat scene's one");
    assert_eq!(
        st["entities"],
        3 + 1 + 2 + 2,
        "arena: player, bat, gate + light; hollow: left + light, right + light"
    );
    assert_eq!(st["prefabs"], 3);
    assert_eq!(st["defs"], 2);
    assert!(st["schemas"].as_u64().unwrap() >= 7);
    assert_eq!(st["systems"], 2);
    assert_eq!(idx.scenes().unwrap(), ["arena", "hollow"]);

    // A second sync with nothing changed re-indexes nothing.
    let p = open(t.path());
    let w = world();
    let mut idx = idx;
    let r = idx
        .sync_project(&p, w.registry(), &systems(&w), None, false)
        .unwrap();
    assert!(r.scenes.is_empty(), "{:?}", r.scenes);
    assert!(!idx.is_stale("arena").unwrap());
}

#[test]
fn queries_have_the_protocol_shape() {
    let t = temp("query");
    let (idx, _, _) = synced(&t);
    let r = idx
        .query("arena", &json!({ "select": ["id", "Transform.pos", "Health.cur"], "where": { "has": ["Health"] }, "order": "dist(@player)" }))
        .unwrap();
    assert_eq!(r["frame"], 0);
    assert_eq!(r["total"], 2);
    assert_eq!(
        r["rows"],
        json!([["@player", [0.0, 0.0], 10], ["@bat", [3.0, 0.0], 3]])
    );
    let r = idx
        .query(
            "arena",
            &json!({ "select": ["id", "Door"], "where": { "Door.leads_to": "@player" } }),
        )
        .unwrap();
    assert_eq!(r["rows"][0][0], "@gate");
    assert_eq!(r["rows"][0][1]["key"], "item:key.brass");
    assert_eq!(r["rows"][0][1]["locked"], true);
    let r = idx
        .query("arena", &json!({ "where": { "has": ["Light"] } }))
        .unwrap();
    assert_eq!(r["rows"], json!([["@gate/light"]]));
    let r = idx
        .query(
            "arena",
            &json!({ "select": ["id", "Parent.entity"], "where": { "has": ["Parent"] } }),
        )
        .unwrap();
    assert_eq!(r["rows"], json!([["@gate/light", "@gate"]]));
    // Paging, ordering, and errors behave as the live path does.
    let r = idx
        .query("arena", &json!({ "limit": 2, "order": "-Health.cur" }))
        .unwrap();
    assert_eq!(r["total"], 4);
    assert_eq!(r["truncated"], true);
    assert_eq!(r["rows"], json!([["@player"], ["@bat"]]));
    assert_eq!(r["next"], "c:2");
    let r = idx
        .query("arena", &json!({ "order": "-Health.cur", "cursor": "c:2" }))
        .unwrap();
    assert_eq!(r["rows"], json!([["@gate"], ["@gate/light"]]));
    let e = idx
        .query("arena", &json!({ "select": ["Nope.x"] }))
        .unwrap_err();
    assert_eq!(e.code, argus_ecs::codes::UNKNOWN_COMPONENT);
    let e = idx.query("nowhere", &json!({})).unwrap_err();
    assert_eq!(e.code, codes::NOT_INDEXED);
    assert!(e.hint.contains("arena, hollow"));

    // Room-qualified references resolve to full ids, as the live world reports them.
    let r = idx
        .query(
            "hollow",
            &json!({ "select": ["id", "Door.leads_to"], "where": { "has": ["Door"] } }),
        )
        .unwrap();
    assert_eq!(
        r["rows"],
        json!([["@a/left", "@b/right"], ["@b/right", "@a/left"]])
    );
}

#[test]
fn get_summary_rooms_describe_and_defs() {
    let t = temp("get");
    let (idx, _, _) = synced(&t);
    let g = idx.get("arena", "@gate", false).unwrap();
    assert_eq!(g["id"], "gate");
    assert_eq!(g["authored"], true);
    assert_eq!(g["source"], "index");
    assert_eq!(g["components"]["Door"]["leads_to"], "@player");
    assert_eq!(g["components"]["Door"]["locked"], true);
    assert_eq!(g["overrides"]["prefab"], "door/locked");
    assert_eq!(g["overrides"]["components"]["Door"]["leads_to"], "@player");
    let g = idx.get("arena", "@gate", true).unwrap();
    let r = &g["resolved"];
    assert_eq!(r["chain"], json!(["door/locked", "door/base"]));
    assert_eq!(
        r["components"]["Door"]["fields"]["locked"]["from"],
        "door/locked"
    );
    assert_eq!(r["components"]["Door"]["fields"]["leads_to"]["from"], "own");
    assert_eq!(
        r["components"]["Transform"]["fields"]["rot"]["from"],
        "default"
    );
    assert_eq!(r["tags"], json!([["door", "door/base"]]));
    assert_eq!(r["children"], json!(["gate/light"]));
    let e = idx.get("arena", "@ghost", false).unwrap_err();
    assert_eq!(e.code, argus_ecs::codes::UNKNOWN_ID);

    let s = idx.summary("arena").unwrap();
    assert_eq!(s["source"], "index");
    assert_eq!(s["scene"], "arena");
    assert_eq!(s["entities"], 4);
    assert_eq!(s["seed"], 7);
    assert_eq!(s["clock"], "continuous@60");
    assert_eq!(s["systems"], json!(["sim.damage", "present.hud"]));
    assert_eq!(
        s["named"][0],
        json!(["@bat", ["Transform", "Health", "Enemy"]])
    );
    let s = idx.summary("hollow").unwrap();
    assert_eq!(s["rooms"], json!(["a", "b"]));
    let rooms = idx.rooms("hollow").unwrap();
    assert_eq!(rooms["rooms"][0]["name"], "a");
    assert_eq!(rooms["rooms"][0]["neighbors"], json!(["b"]));
    assert_eq!(rooms["rooms"][0]["entities"], 1);
    assert_eq!(rooms["rooms"][0]["bounds"], json!([0.0, 0.0, 10.0, 10.0]));

    let d = idx.describe(&json!({ "component": "Health" })).unwrap();
    assert_eq!(d["rows"][0]["doc"], "Hit points");
    assert_eq!(d["rows"][0]["fields"][0]["range"], json!([0.0, 1000.0]));
    assert_eq!(d["rows"][0]["written_by"], json!(["sim.damage"]));
    assert_eq!(
        d["rows"][0]["read_by"],
        json!(["sim.damage", "present.hud"]),
        "a writer reads too"
    );
    assert_eq!(
        idx.who_writes("Health").unwrap()["systems"],
        json!(["sim.damage"])
    );
    let defs = idx.defs(&json!({})).unwrap();
    assert_eq!(defs["tables"][0]["name"], "items");
    assert_eq!(defs["tables"][0]["rows"], 2);
    let rows = idx
        .defs(&json!({ "table": "items", "where": { "name": "Bread" } }))
        .unwrap();
    assert_eq!(rows["columns"], json!(["id", "name", "weight"]));
    assert_eq!(rows["rows"], json!([["bread", "Bread", 1.0]]));
}

#[test]
fn references_instances_and_full_text_search() {
    let t = temp("refs");
    let (idx, _, _) = synced(&t);
    let r = idx.references_to("item:key.brass", &json!({})).unwrap();
    let rows = r["rows"].as_array().unwrap();
    assert!(
        rows.iter()
            .any(|x| x[1] == "prefab" && x[2] == "door/locked"),
        "{rows:?}"
    );
    assert!(
        rows.iter()
            .any(|x| x[0] == "arena" && x[2] == "gate" && x[3] == "Door.key"),
        "{rows:?}"
    );
    let r = idx.references_to("@player", &json!({})).unwrap();
    assert_eq!(
        r["rows"][0],
        json!([
            "arena",
            "entity",
            "gate",
            "Door.leads_to",
            "entity",
            "scenes/arena/gate.toml:7"
        ])
    );
    let r = idx.references_to("door/base", &json!({})).unwrap();
    let from: Vec<&str> = r["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x[2].as_str().unwrap())
        .collect();
    assert!(
        from.contains(&"door/locked") && from.contains(&"gate") && from.contains(&"a/left"),
        "{from:?}"
    );
    assert_eq!(
        idx.instances_of("door/base").unwrap(),
        [
            ("arena".to_string(), "gate".to_string()),
            ("hollow".to_string(), "a/left".to_string()),
            ("hollow".to_string(), "b/right".to_string())
        ]
    );

    let s = idx.search("hit points", &json!({})).unwrap();
    let names: Vec<&str> = s["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x[1].as_str().unwrap())
        .collect();
    assert!(names.contains(&"Health"), "{names:?}");
    assert!(names.contains(&"sim.damage"), "{names:?}");
    let s = idx.search("brightness", &json!({})).unwrap();
    assert_eq!(
        s["rows"][0],
        json!(["field", "Light.intensity", "Brightness of the lamp"])
    );
    let s = idx.search("does-not-appear", &json!({})).unwrap();
    assert_eq!(s["total"], 0);
}

#[test]
fn file_changes_are_picked_up_and_entities_update_incrementally() {
    let t = temp("stale");
    let (mut idx, w, p) = synced(&t);
    assert!(!idx.is_stale("arena").unwrap());
    // A hand edit changes size, so the scene is stale and the next sync re-indexes only it.
    write(
        t.path(),
        "scenes/arena/player.toml",
        "tags = [\"player\"]\n\n[Transform]\npos = [1.0, 1.0]\n\n[Health]\ncur = 4\nmax = 10\n",
    );
    assert!(idx.is_stale("arena").unwrap());
    assert!(!idx.is_stale("hollow").unwrap());
    let r = idx
        .sync_project(&p, w.registry(), &systems(&w), None, false)
        .unwrap();
    assert_eq!(r.scenes, ["arena"]);
    let g = idx.get("arena", "@player", false).unwrap();
    assert_eq!(g["components"]["Health"]["cur"], 4);
    // A new file appears; a file disappears.
    write(
        t.path(),
        "scenes/hollow/rooms/b/lamp.toml",
        "[Light]\nintensity = 2.0\n",
    );
    std::fs::remove_file(t.path().join("scenes/arena/bat.toml")).unwrap();
    assert!(idx.is_stale("arena").unwrap());
    assert!(idx.is_stale("hollow").unwrap());
    let r = idx
        .sync_project(&p, w.registry(), &systems(&w), None, false)
        .unwrap();
    assert_eq!(r.scenes, ["arena", "hollow"]);
    assert_eq!(idx.summary("arena").unwrap()["entities"], 3);
    assert_eq!(
        idx.query("hollow", &json!({ "where": { "has": ["Light"] } }))
            .unwrap()["total"],
        3
    );
    // A prefab edit makes every scene stale.
    write(
        t.path(),
        "prefabs/enemy/bat.toml",
        "tags = [\"Enemy\"]\n\n[Health]\ncur = 9\nmax = 9\n",
    );
    let p = open(t.path());
    let r = idx
        .sync_project(&p, w.registry(), &systems(&w), None, false)
        .unwrap();
    assert_eq!(r.scenes, ["arena", "hollow"]);

    // Incremental: the daemon path after an edit, from a scene in memory.
    let mut s = scene(&p, &w, "arena");
    let ctx = p.ctx(w.registry());
    s.set_override(&ctx, "player", "Health.cur", &argus_ecs::Value::Int(7))
        .unwrap();
    idx.sync_entities(&s, &["player"]).unwrap();
    assert_eq!(
        idx.get("arena", "@player", false).unwrap()["components"]["Health"]["cur"],
        7
    );
    idx.remove_entity("arena", "@gate", None).unwrap();
    assert_eq!(
        idx.summary("arena").unwrap()["entities"],
        1,
        "gate and its light are gone"
    );
    let e = idx.get("arena", "@gate/light", false).unwrap_err();
    assert_eq!(e.code, argus_ecs::codes::UNKNOWN_ID);

    // The index is a cache: a schema bump starts over cleanly.
    drop(idx);
    let conn = rusqlite_open(&Index::path(t.path()));
    conn.execute(
        "UPDATE meta SET value = '0' WHERE key = 'schema_version'",
        [],
    )
    .unwrap();
    drop(conn);
    let idx = Index::open(t.path()).unwrap();
    assert_eq!(idx.stats().unwrap()["entities"], 0);
}

fn rusqlite_open(p: &std::path::Path) -> rusqlite::Connection {
    rusqlite::Connection::open(p).unwrap()
}
