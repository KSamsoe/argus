//! The index serves the same reads with the same shapes as the live world
//! (§4), and the daemon keeps it current on edits, reloads, and moves.

mod common;

use argus_daemon::Daemon;
use common::*;
use serde_json::{json, Value as Json};

fn strip(mut v: Json, keys: &[&str]) -> Json {
    if let Some(o) = v.as_object_mut() {
        for k in keys {
            o.remove(*k);
        }
    }
    v
}

#[test]
fn live_and_index_reads_have_identical_shapes() {
    let t = temp("shapes");
    project(t.path());
    let mut d = open(t.path());
    let st = ok(&mut d, "session.status", json!({}));
    assert!(st["index"].as_str().unwrap().ends_with("index.db"), "{st}");
    assert_eq!(st["index_error"], Json::Null);

    for params in [
        json!({ "select": ["id", "Transform.pos", "Health.cur"], "where": { "has": ["Health"] }, "order": "dist(@player)" }),
        json!({ "select": ["id", "Door"], "where": { "Door.leads_to": "@player" } }),
        json!({ "where": { "Health.cur": { "gte": 3 } } }),
        json!({ "limit": 1, "order": "-Health.cur" }),
        json!({ "where": { "within": { "of": "@player", "r": 5.0 }, "has": ["Enemy"] } }),
    ] {
        let live = ok(&mut d, "world.query", params.clone());
        let mut p = params.clone();
        p["source"] = json!("index");
        let idx = ok(&mut d, "world.query", p);
        assert_eq!(live, idx, "query {params}");
    }
    for entity in ["@bat", "@player", "@door"] {
        for resolved in [false, true] {
            let live = ok(
                &mut d,
                "world.get",
                json!({ "entity": entity, "resolved": resolved }),
            );
            let idx = ok(
                &mut d,
                "world.get",
                json!({ "entity": entity, "resolved": resolved, "source": "index" }),
            );
            assert_eq!(live["source"], "live");
            assert_eq!(idx["source"], "index");
            // The index has no live handle.
            assert_eq!(
                strip(live, &["entity", "source"]),
                strip(idx, &["entity", "source"]),
                "get {entity} resolved={resolved}"
            );
        }
    }
    let live = ok(&mut d, "world.describe", json!({}));
    let idx = ok(&mut d, "world.describe", json!({ "source": "index" }));
    assert_eq!(live, idx);
    let live = ok(&mut d, "world.who_writes", json!({ "component": "Health" }));
    let idx = ok(
        &mut d,
        "world.who_writes",
        json!({ "component": "Health", "source": "index" }),
    );
    assert_eq!(live, idx);
    let live = ok(
        &mut d,
        "world.defs",
        json!({ "table": "items", "where": { "name": "Bread" } }),
    );
    let idx = ok(
        &mut d,
        "world.defs",
        json!({ "table": "items", "where": { "name": "Bread" }, "source": "index" }),
    );
    assert_eq!(live, idx);
    let live = ok(&mut d, "world.summary", json!({}));
    let idx = ok(&mut d, "world.summary", json!({ "source": "index" }));
    assert_eq!(live["source"], "live");
    assert_eq!(idx["source"], "index");
    let volatile = [
        "frame",
        "tick",
        "time",
        "playing",
        "paused",
        "pending_events",
        "source",
        "rooms",
    ];
    assert_eq!(strip(live, &volatile), strip(idx, &volatile));

    // Another scene is answered from the index while arena stays live.
    let s = ok(&mut d, "world.summary", json!({ "scene": "delve" }));
    assert_eq!(s["scene"], "delve");
    assert_eq!(s["source"], "index");
    assert_eq!(s["entities"], 1);
    assert_eq!(s["clock"], "discrete");
    let q = ok(
        &mut d,
        "world.query",
        json!({ "scene": "hollow", "select": ["id", "Door.leads_to"] }),
    );
    assert_eq!(
        q["rows"],
        json!([["@a/left", "@b/right"], ["@b/right", "@a/left"]])
    );
    let rooms = ok(&mut d, "scene.rooms", json!({ "scene": "hollow" }));
    assert_eq!(rooms["source"], "index");
    assert_eq!(rooms["rooms"][1]["name"], "b");
    assert_eq!(
        ok(&mut d, "session.status", json!({}))["scene"],
        "arena",
        "still live on arena"
    );
}

#[test]
fn the_index_follows_edits_reloads_and_moves() {
    let t = temp("follow");
    project(t.path());
    let mut d = open(t.path());
    commit(
        &mut d,
        "agent:claude",
        "hurt",
        &[
            (
                "edit.set",
                json!({ "entity": "@player", "path": "Health.cur", "value": 1 }),
            ),
            (
                "edit.spawn",
                json!({ "id": "bat.2", "prefab": "enemy/bat" }),
            ),
            ("edit.despawn", json!({ "entity": "@door" })),
        ],
    );
    let g = ok(
        &mut d,
        "world.get",
        json!({ "entity": "@player", "source": "index" }),
    );
    assert_eq!(g["components"]["Health"]["cur"], 1);
    let q = ok(
        &mut d,
        "world.query",
        json!({ "source": "index", "where": { "has": ["Enemy"] } }),
    );
    assert_eq!(q["rows"], json!([["@bat"], ["@bat.2"]]));
    let e = err(
        &mut d,
        "world.get",
        json!({ "entity": "@door", "source": "index" }),
    );
    assert_eq!(e.code, argus_ecs::codes::UNKNOWN_ID);
    ok(&mut d, "edit.undo", json!({ "author": "agent:claude" }));
    let g = ok(
        &mut d,
        "world.get",
        json!({ "entity": "@door", "source": "index" }),
    );
    assert_eq!(g["components"]["Door"]["leads_to"], "@player");

    // References and instances across the whole project.
    let r = ok(&mut d, "world.refs", json!({ "target": "@player" }));
    assert_eq!(r["rows"][0][2], "door");
    assert_eq!(r["rows"][0][3], "Door.leads_to");
    let r = ok(&mut d, "world.instances", json!({ "prefab": "enemy/bat" }));
    assert_eq!(r["rows"], json!([["arena", "@bat"]]));
    let s = ok(&mut d, "world.search", json!({ "text": "hit point" }));
    let names: Vec<&str> = s["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x[1].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"Health") && names.contains(&"sim.damage"),
        "{names:?}"
    );

    // A move through the protocol rewrites references and the index.
    let m = ok(
        &mut d,
        "asset.move",
        json!({ "from": "prefabs/enemy/bat.toml", "to": "prefabs/enemy/vampire.toml" }),
    );
    assert_eq!(m["kind"], "prefab");
    assert_eq!(m["rewritten"], json!(["scenes/arena/bat.toml"]));
    assert!(read(t.path(), "scenes/arena/bat.toml").starts_with("prefab = \"enemy/vampire\""));
    assert_eq!(
        ok(
            &mut d,
            "world.instances",
            json!({ "prefab": "enemy/vampire" })
        )["total"],
        1
    );
    assert_eq!(
        ok(&mut d, "world.instances", json!({ "prefab": "enemy/bat" }))["total"],
        0
    );
    assert_eq!(
        ok(
            &mut d,
            "world.get",
            json!({ "entity": "@bat", "resolved": true })
        )["resolved"]["chain"],
        json!(["enemy/vampire"])
    );

    // A hand edit outside the daemon is picked up by scene.index (and by the idle refresh).
    write(t.path(), "scenes/delve/hero.toml", "[Health]\ncur = 1\n");
    let r = ok(&mut d, "scene.index", json!({}));
    assert_eq!(r["scenes"], json!(["delve"]), "stale: {}", r["stale"]);
    assert_eq!(
        ok(
            &mut d,
            "world.get",
            json!({ "entity": "@hero", "scene": "delve" })
        )["components"]["Health"]["cur"],
        1
    );
    let r = ok(&mut d, "scene.index", json!({ "rebuild": true }));
    assert_eq!(r["scenes"], json!(["arena", "delve", "hollow"]));
    // arena: player, bat, door; delve: hero; hollow: left, right.
    assert_eq!(r["stats"]["entities"], 6);

    // A scene that fails to load is still readable, from the index.
    write(t.path(), "scenes/arena/broken.toml", "[Nope]\nx = 1\n");
    let mut d2 = Daemon::new();
    let r = ok(
        &mut d2,
        "session.open",
        json!({ "project": t.path().to_string_lossy() }),
    );
    assert!(r.get("issues").is_some());
    assert_eq!(ok(&mut d2, "session.status", json!({}))["loaded"], false);
    let s = ok(&mut d2, "world.summary", json!({}));
    assert_eq!(s["source"], "index");
    assert!(s["entities"].as_u64().unwrap() >= 3);
    let g = ok(&mut d2, "world.get", json!({ "entity": "@player" }));
    assert_eq!(
        g["components"]["Health"]["cur"], 3,
        "the undo above restored the file"
    );
}
