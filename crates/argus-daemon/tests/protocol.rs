//! The phase-0 protocol end to end, in process (§5). The socket path is
//! covered in `transport.rs`.

mod common;

use argus_daemon::{codes, Daemon};
use common::*;
use serde_json::json;

#[global_allocator]
static ALLOC: argus_sim::alloc::Counting = argus_sim::alloc::Counting;

#[test]
fn session_open_gives_the_one_screen_summary() {
    let t = temp("open");
    project(t.path());
    let mut d = open(t.path());
    let s = ok(&mut d, "world.summary", json!({}));
    assert_eq!(s["scene"], "arena");
    assert_eq!(s["entities"], 3);
    assert_eq!(s["authored"], 3);
    assert_eq!(s["seed"], 7);
    assert_eq!(s["clock"], "continuous@60");
    assert_eq!(s["playing"], false);
    let named: Vec<&str> = s["named"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r[0].as_str().unwrap())
        .collect();
    assert_eq!(named, ["@bat", "@door", "@player"]);
    let systems: Vec<&str> = s["systems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    // The physics, navigation, jobs, camera, and post systems are in every
    // scene and cost nothing without bodies, agents, workers, or cameras;
    // the hit-stop check runs first in input, physics control first in sim,
    // move and overlap last with the nav agents after them, the timelines
    // first in late, and the camera, the post effects, the particles, and
    // the UI's layout last. Rooms stream first, after the hit-stop check,
    // before the physics moves anything.
    assert_eq!(
        systems,
        [
            "post.hitstop",
            "rooms.stream",
            "input.actions",
            "ui.input",
            "physics.control",
            "jobs.update",
            "dialogue.update",
            "input.rebind",
            "sim.damage",
            "sim.move",
            "physics.move",
            "physics.overlap",
            "nav.agents",
            "timeline.update",
            "anim.update",
            "skeletal.pose",
            "tween.update",
            "camera.update",
            "post.update",
            "particles.update",
            "ui.layout",
            "present.hud"
        ]
    );
    assert!(
        s.to_string().len() < 2048,
        "summary must fit in ~2 KB: {}",
        s.to_string().len()
    );

    let st = ok(&mut d, "session.status", json!({}));
    assert_eq!(st["project"], "fixture");
    assert_eq!(st["scene"], "arena");
    assert_eq!(st["driver"], "continuous");

    let caps = ok(&mut d, "session.capabilities", json!({}));
    let names: Vec<&str> = caps["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    for m in [
        "session.open",
        "world.summary",
        "world.query",
        "world.get",
        "world.describe",
        "world.who_writes",
        "world.defs",
        "edit.begin",
        "edit.set",
        "edit.spawn",
        "edit.despawn",
        "edit.commit",
        "edit.abort",
        "edit.lock",
        "edit.undo",
        "edit.redo",
        "sim.play",
        "sim.pause",
        "sim.step",
        "sim.advance",
        "sim.speed",
        "sim.threads",
        "sim.run_until",
        "script.reload",
        "script.systems",
        "script.errors",
        "obs.events",
        "prof.systems",
        "prof.alloc",
        "prof.capture",
    ] {
        assert!(names.contains(&m), "missing {m}");
    }
    let docs = ok(&mut d, "session.docs", json!({ "method": "world.query" }));
    assert!(docs["text"].as_str().unwrap().starts_with("world.query\n"));

    // Errors are documentation: no project, unknown method, bad params.
    let mut empty = Daemon::new();
    let e = err(&mut empty, "world.get", json!({ "entity": "@player" }));
    assert_eq!(e.code, codes::NO_PROJECT);
    assert!(e.hint.contains("session.open"));
    let e = err(&mut d, "obs.frobnicate", json!({}));
    assert_eq!(e.code, codes::UNKNOWN_METHOD);
    let e = err(&mut d, "world.get", json!({}));
    assert_eq!(e.code, codes::BAD_PARAMS);
    assert!(e.hint.contains("session.docs"));
    let resp = d
        .handle_line(0, "{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"nope.x\"}")
        .unwrap();
    assert_eq!(resp["error"]["code"], -32601);
    assert_eq!(resp["id"], 9);
    let resp = d.handle_line(0, "not json").unwrap();
    assert_eq!(resp["error"]["code"], -32700);
    assert!(
        d.handle_line(0, "{\"jsonrpc\":\"2.0\",\"method\":\"sim.status\"}")
            .is_none(),
        "notifications get no reply"
    );
}

#[test]
fn world_query_obeys_the_read_contract() {
    let t = temp("query");
    project(t.path());
    let mut d = open(t.path());
    let r = ok(
        &mut d,
        "world.query",
        json!({ "select": ["id", "Transform.pos", "Health.cur"], "where": { "has": ["Health"] }, "order": "dist(@player)" }),
    );
    assert_eq!(r["frame"], 0);
    assert_eq!(r["total"], 2);
    assert_eq!(r["truncated"], false);
    assert_eq!(
        r["rows"],
        json!([["@player", [0.0, 0.0], 3], ["@bat", [3.0, 0.0], 2]])
    );

    let r = ok(
        &mut d,
        "world.query",
        json!({ "where": { "has": ["Enemy"], "within": { "of": "@player", "r": 5.0 } } }),
    );
    assert_eq!(r["rows"], json!([["@bat"]]));
    let r = ok(
        &mut d,
        "world.query",
        json!({ "where": { "within": { "of": "@player", "r": 1.0 } } }),
    );
    assert_eq!(r["total"], 1);
    let r = ok(
        &mut d,
        "world.query",
        json!({ "select": ["id", "Door"], "where": { "Door.leads_to": "@player" } }),
    );
    assert_eq!(r["rows"][0][0], "@door");
    assert_eq!(r["rows"][0][1]["key"], "item:key.brass");
    let r = ok(
        &mut d,
        "world.query",
        json!({ "where": { "Health.cur": { "gte": 3 } } }),
    );
    assert_eq!(r["rows"], json!([["@player"]]));

    // Paging: limit 1, cursor continues, total is always present.
    let r = ok(
        &mut d,
        "world.query",
        json!({ "limit": 1, "order": "-Health.cur" }),
    );
    assert_eq!(r["total"], 3);
    assert_eq!(r["truncated"], true);
    assert_eq!(r["rows"], json!([["@player"]]));
    let next = r["next"].as_str().unwrap().to_string();
    let r2 = ok(
        &mut d,
        "world.query",
        json!({ "limit": 1, "order": "-Health.cur", "cursor": next }),
    );
    assert_eq!(r2["rows"], json!([["@bat"]]));
    let e = err(&mut d, "world.query", json!({ "cursor": "zzz" }));
    assert_eq!(e.code, codes::BAD_CURSOR);
    let e = err(&mut d, "world.query", json!({ "select": ["Nope.x"] }));
    assert_eq!(e.code, argus_ecs::codes::UNKNOWN_COMPONENT);
}

#[test]
fn world_get_describe_who_writes_and_defs() {
    let t = temp("get");
    project(t.path());
    let mut d = open(t.path());
    let g = ok(&mut d, "world.get", json!({ "entity": "@bat" }));
    assert_eq!(g["authored"], true);
    assert_eq!(g["id"], "bat");
    assert_eq!(g["components"]["Health"]["cur"], 2);
    assert_eq!(g["components"]["Enemy"], json!({}));
    assert_eq!(g["overrides"]["prefab"], "enemy/bat");
    assert_eq!(
        g["overrides"]["components"]["Transform"]["pos"],
        json!([3.0, 0.0])
    );
    assert!(
        g["overrides"]["components"].get("Health").is_none(),
        "overrides only"
    );
    let g = ok(
        &mut d,
        "world.get",
        json!({ "entity": "@bat", "resolved": true }),
    );
    let r = &g["resolved"];
    assert_eq!(r["chain"], json!(["enemy/bat"]));
    assert_eq!(
        r["components"]["Health"]["fields"]["cur"]["from"],
        "enemy/bat"
    );
    assert_eq!(r["components"]["Transform"]["fields"]["pos"]["from"], "own");
    assert_eq!(
        r["components"]["Transform"]["fields"]["rot"]["from"],
        "default"
    );
    assert_eq!(r["tags"], json!([["Enemy", "enemy/bat"]]));

    let desc = ok(&mut d, "world.describe", json!({ "component": "Health" }));
    let h = &desc["rows"][0];
    assert_eq!(h["doc"], "Hit points");
    assert_eq!(h["fields"][0]["range"], json!([0.0, 1000.0]));
    assert!(h["source"]
        .as_str()
        .unwrap()
        .starts_with("components/health.luau:"));
    assert_eq!(h["written_by"], json!(["sim.damage"]));
    assert_eq!(h["read_by"], json!(["sim.damage", "present.hud"]));
    let all = ok(&mut d, "world.describe", json!({}));
    assert!(all["total"].as_u64().unwrap() >= 7);
    let ww = ok(&mut d, "world.who_writes", json!({ "component": "Health" }));
    assert_eq!(ww["systems"], json!(["sim.damage"]));

    let tables = ok(&mut d, "world.defs", json!({}));
    // The engine's dialogue table is every project's (phase 3 item 4).
    assert_eq!(tables["tables"][0]["name"], "dialogue");
    assert_eq!(tables["tables"][0]["rows"], 0);
    assert_eq!(tables["tables"][1]["name"], "items");
    assert_eq!(tables["tables"][1]["rows"], 2);
    let rows = ok(
        &mut d,
        "world.defs",
        json!({ "table": "items", "where": { "name": "Bread" } }),
    );
    assert_eq!(rows["columns"], json!(["id", "name", "weight"]));
    assert_eq!(rows["rows"], json!([["bread", "Bread", 1.0]]));
    assert_eq!(rows["total"], 1);
}

#[test]
fn edits_are_atomic_authored_edits_write_files_and_the_feed_answers_why() {
    let t = temp("edits");
    project(t.path());
    let mut d = open(t.path());

    let c = commit(
        &mut d,
        "agent:claude",
        "weaken the player",
        &[
            (
                "edit.set",
                json!({ "entity": "@player", "path": "Health.cur", "value": 1 }),
            ),
            (
                "edit.add",
                json!({ "entity": "@player", "component": "Light" }),
            ),
            (
                "edit.set",
                json!({ "entity": "@player", "path": "Light.intensity", "value": 2.5 }),
            ),
        ],
    );
    assert_eq!(c["id"], 1);
    assert_eq!(c["touched"], json!(["@player"]));
    assert_eq!(c["files"], json!(["scenes/arena/player.toml"]));
    let text = read(t.path(), "scenes/arena/player.toml");
    assert!(text.contains("cur = 1 # hp"), "{text}");
    assert!(text.contains("[Light]\nintensity = 2.5"), "{text}");
    assert!(text.starts_with("# the player"), "{text}");
    let g = ok(&mut d, "world.get", json!({ "entity": "@player" }));
    assert_eq!(g["components"]["Health"]["cur"], 1);
    assert_eq!(g["components"]["Light"]["intensity"], 2.5);

    // Atomic: a bad op in the middle leaves nothing applied.
    let tx = ok(
        &mut d,
        "edit.begin",
        json!({ "author": "agent:claude", "note": "half bad" }),
    )["tx"]
        .clone();
    ok(
        &mut d,
        "edit.set",
        json!({ "tx": tx, "entity": "@player", "path": "Health.max", "value": 99 }),
    );
    ok(
        &mut d,
        "edit.set",
        json!({ "tx": tx, "entity": "@player", "path": "Health.cur", "value": 5000 }),
    );
    let e = err(&mut d, "edit.commit", json!({ "tx": tx }));
    assert_eq!(e.code, argus_ecs::codes::OUT_OF_RANGE);
    assert!(
        e.message
            .contains("Health.cur must be in range [0, 1000]; got 5000"),
        "{e}"
    );
    assert!(
        e.r#ref
            .as_deref()
            .unwrap()
            .starts_with("components/health.luau:"),
        "{e:?}"
    );
    let g = ok(&mut d, "world.get", json!({ "entity": "@player" }));
    assert_eq!(g["components"]["Health"]["max"], 3, "rolled back");
    assert!(!read(t.path(), "scenes/arena/player.toml").contains("99"));
    let e = err(&mut d, "edit.commit", json!({ "tx": tx }));
    assert_eq!(e.code, codes::NO_TRANSACTION);

    // Runtime spawn (never written), authored spawn (a file), despawn (file gone).
    let c = commit(
        &mut d,
        "agent:claude",
        "spawn things",
        &[
            (
                "edit.spawn",
                json!({ "tags": ["Enemy"], "components": { "Health": { "cur": 1 } } }),
            ),
            (
                "edit.spawn",
                json!({ "id": "bat.2", "prefab": "enemy/bat", "components": { "Transform": { "pos": [5, 5] } } }),
            ),
            ("edit.despawn", json!({ "entity": "@door" })),
        ],
    );
    let touched = c["touched"].as_array().unwrap();
    assert!(
        touched
            .iter()
            .any(|v| v.as_str().unwrap().starts_with("e:")),
        "{touched:?}"
    );
    assert!(touched.contains(&json!("@bat.2")));
    assert_eq!(
        c["files"],
        json!(["scenes/arena/bat.2.toml", "scenes/arena/door.toml"])
    );
    assert_eq!(
        read(t.path(), "scenes/arena/bat.2.toml"),
        "prefab = \"enemy/bat\"\n\n[Transform]\npos = [5.0, 5.0]\n"
    );
    assert!(!t.path().join("scenes/arena/door.toml").exists());
    let s = ok(&mut d, "world.summary", json!({}));
    assert_eq!(s["entities"], 4);
    let q = ok(
        &mut d,
        "world.query",
        json!({ "where": { "has": ["Enemy"] } }),
    );
    assert_eq!(q["total"], 3);

    // Undo per author brings the door file back; redo removes it again.
    let u = ok(&mut d, "edit.undo", json!({ "author": "agent:claude" }));
    assert_eq!(u["kind"], json!({ "undo": 2 }));
    assert!(t.path().join("scenes/arena/door.toml").exists());
    assert!(!t.path().join("scenes/arena/bat.2.toml").exists());
    let g = ok(&mut d, "world.get", json!({ "entity": "@door" }));
    assert_eq!(g["components"]["Door"]["leads_to"], "@player");
    let q = ok(
        &mut d,
        "world.query",
        json!({ "where": { "has": ["Enemy"] } }),
    );
    assert_eq!(q["total"], 1);
    ok(&mut d, "edit.redo", json!({ "author": "agent:claude" }));
    assert!(!t.path().join("scenes/arena/door.toml").exists());
    let e = err(&mut d, "edit.undo", json!({ "author": "human:kevin" }));
    assert_eq!(e.code, codes::NOTHING_TO_UNDO);

    // The feed answers who changed this and why.
    let f = ok(&mut d, "edit.feed", json!({}));
    assert_eq!(f["total"], 4);
    assert_eq!(f["rows"][0]["author"], "agent:claude");
    assert_eq!(f["rows"][0]["note"], "weaken the player");
    assert_eq!(
        f["rows"][0]["ops"][0],
        json!(["set", "@player", "Health.cur", 1])
    );
    assert_eq!(f["rows"][2]["kind"], json!({ "undo": 2 }));
    let f = ok(&mut d, "edit.feed", json!({ "since": 3 }));
    assert_eq!(f["total"], 1);
    assert_eq!(f["rows"][0]["kind"], json!({ "redo": 3 }));

    // Locks: the human fences Health on the player; the agent's write fails naming the lock.
    let l = ok(
        &mut d,
        "edit.lock",
        json!({ "author": "human:kevin", "note": "balancing hp by hand", "entities": ["@player"], "paths": ["Health"] }),
    );
    let lock = l["lock"].clone();
    let tx = ok(
        &mut d,
        "edit.begin",
        json!({ "author": "agent:claude", "note": "poke" }),
    )["tx"]
        .clone();
    ok(
        &mut d,
        "edit.set",
        json!({ "tx": tx, "entity": "@player", "path": "Health.cur", "value": 2 }),
    );
    let e = err(&mut d, "edit.commit", json!({ "tx": tx }));
    assert_eq!(e.code, codes::LOCKED);
    assert!(e.message.contains("locked by human:kevin"), "{e}");
    assert!(e.message.contains("balancing hp by hand"), "{e}");
    // The owner may still write, and other paths are free.
    commit(
        &mut d,
        "human:kevin",
        "tune",
        &[(
            "edit.set",
            json!({ "entity": "@player", "path": "Health.cur", "value": 2 }),
        )],
    );
    commit(
        &mut d,
        "agent:claude",
        "light",
        &[(
            "edit.set",
            json!({ "entity": "@player", "path": "Light.intensity", "value": 1.0 }),
        )],
    );
    let e = err(
        &mut d,
        "edit.unlock",
        json!({ "author": "agent:claude", "lock": lock }),
    );
    assert_eq!(e.code, codes::BAD_LOCK);
    ok(
        &mut d,
        "edit.unlock",
        json!({ "author": "human:kevin", "lock": lock }),
    );
    assert_eq!(ok(&mut d, "edit.locks", json!({}))["rows"], json!([]));
    commit(
        &mut d,
        "agent:claude",
        "free again",
        &[(
            "edit.set",
            json!({ "entity": "@player", "path": "Health.cur", "value": 3 }),
        )],
    );
}

#[test]
fn sim_and_events_answer_why_the_bat_died() {
    let t = temp("sim");
    project(t.path());
    let mut d = open(t.path());
    let st = ok(&mut d, "sim.play", json!({}));
    assert_eq!(st["playing"], true);
    let r = ok(
        &mut d,
        "sim.run_until",
        json!({ "event": "death", "max_ticks": 100 }),
    );
    assert_eq!(r["hit"], true);
    assert_eq!(r["ticks"], 2);
    assert_eq!(r["tick"], 2);
    let ev = ok(&mut d, "obs.events", json!({ "name": "death" }));
    assert_eq!(ev["total"], 1);
    assert_eq!(ev["rows"][0]["frame"], 1);
    assert_eq!(ev["rows"][0]["fields"]["entity"], "@bat");
    assert_eq!(ev["rows"][0]["fields"]["cause"], "attrition");
    let ev = ok(&mut d, "obs.events", json!({ "entity": "@bat" }));
    assert!(
        ev["total"].as_u64().unwrap() >= 2,
        "spawned and death: {ev}"
    );
    let g = ok(&mut d, "world.get", json!({ "entity": "@bat" }));
    assert_eq!(g["components"]["Health"]["cur"], 0);
    let ww = ok(&mut d, "world.who_writes", json!({ "component": "Health" }));
    assert_eq!(ww["systems"], json!(["sim.damage"]));

    // Paging over the event log with since.
    let all = ok(&mut d, "obs.events", json!({ "limit": 2 }));
    assert_eq!(all["truncated"], true);
    let last = all["last_seq"].as_u64().unwrap();
    let tail = ok(&mut d, "obs.events", json!({ "since": last - 1 }));
    assert_eq!(tail["total"], 1);

    // Stop restores the play snapshot; the sim state comes back too.
    let st = ok(&mut d, "sim.stop", json!({}));
    assert_eq!(st["playing"], false);
    assert_eq!(st["tick"], 0);
    let g = ok(&mut d, "world.get", json!({ "entity": "@bat" }));
    assert_eq!(g["components"]["Health"]["cur"], 2);

    // Step and speed without play; run_until by time and ticks.
    let st = ok(&mut d, "sim.step", json!({ "n": 3 }));
    assert_eq!(st["tick"], 3);
    assert_eq!(st["time"], 0.05);
    let st = ok(&mut d, "sim.speed", json!({ "x": 3 }));
    assert_eq!(st["speed"], 3.0);
    let r = ok(&mut d, "sim.run_until", json!({ "time": 1.0 }));
    assert_eq!(r["tick"], 60);
    let r = ok(&mut d, "sim.run_until", json!({ "ticks": 5 }));
    assert_eq!(r["tick"], 65);
    assert_eq!(r["hit"], true);
    let pf = ok(&mut d, "prof.frame", json!({}));
    assert_eq!(pf["tick"], 64, "the last tick that ran, numbered from 0");
    assert_eq!(pf["systems"][0][0], "post.hitstop", "stage order");
    let ps = ok(&mut d, "prof.systems", json!({ "last": 10 }));
    assert_eq!(ps["rows"][8][0], "sim.damage");
    assert!(
        ok(&mut d, "prof.worst", json!({ "n": 2 }))["rows"]
            .as_array()
            .unwrap()
            .len()
            == 2
    );
    assert!(
        ok(&mut d, "prof.archetypes", json!({}))["rows"]
            .as_array()
            .unwrap()
            .len()
            >= 2
    );
    assert_eq!(
        pf["systems"][0].as_array().unwrap().len(),
        4,
        "name, ms, allocs, bytes per system"
    );

    // prof.alloc: this binary installs the counting allocator as argusd does.
    let pa = ok(&mut d, "prof.alloc", json!({ "last": 10 }));
    assert_eq!(pa["counted"], true);
    assert_eq!(
        pa["rows"][8][0], "sim.damage",
        "stage order, like prof.systems"
    );
    let physics: Vec<&serde_json::Value> = pa["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| {
            let n = r[0].as_str().unwrap();
            [
                "physics.",
                "camera.",
                "anim.",
                "tween.",
                "particles.",
                "jobs.",
                "dialogue.",
            ]
            .iter()
            .any(|p| n.starts_with(p))
        })
        .collect();
    assert_eq!(physics.len(), 9);
    assert!(
        physics.iter().all(|r| r[2].as_f64() == Some(0.0)),
        "physics, cameras, emitters, jobs, and conversations with none in the scene allocate nothing: {physics:?}"
    );
    assert!(pa["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r[1] != "present")
        .all(|r| r[6] == 10));
    assert!(pa["tick_mean_allocs"].as_f64().unwrap() >= 0.0);

    // prof.capture: arm, tick past it, get the file and the event.
    assert_eq!(ok(&mut d, "prof.capture", json!({}))["armed"], false);
    let e = err(&mut d, "prof.capture", json!({ "ticks": 0 }));
    assert_eq!(e.code, codes::BAD_PARAMS);
    let armed = ok(&mut d, "prof.capture", json!({ "ticks": 3 }));
    assert_eq!(armed["armed"], true);
    assert_eq!(armed["from_tick"], 65);
    let path = std::path::PathBuf::from(armed["path"].as_str().unwrap());
    assert!(
        path.parent()
            .unwrap()
            .ends_with(std::path::Path::new(".argus").join("captures")),
        "{path:?}"
    );
    assert!(path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .ends_with("-tick65.json"));
    ok(&mut d, "sim.step", json!({ "n": 2 }));
    let st = ok(&mut d, "prof.capture", json!({}));
    assert_eq!(st["captured"], 2);
    assert_eq!(st["ticks"], 3);
    ok(&mut d, "sim.step", json!({ "n": 2 }));
    assert_eq!(
        ok(&mut d, "prof.capture", json!({}))["armed"],
        false,
        "written and cleared"
    );
    let ev = ok(&mut d, "obs.events", json!({ "name": "prof.capture" }));
    assert_eq!(ev["total"], 1);
    assert_eq!(ev["rows"][0]["fields"]["ticks"], 3);
    assert_eq!(ev["rows"][0]["fields"]["from_tick"], 65);
    assert_eq!(
        ev["rows"][0]["fields"]["path"].as_str().unwrap(),
        path.to_string_lossy()
    );
    let trace: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let events = trace["traceEvents"].as_array().unwrap();
    assert!(events
        .iter()
        .any(|e| e["name"] == "tick 65" && e["ph"] == "X" && e["cat"] == "tick"));
    assert!(events
        .iter()
        .any(|e| e["name"] == "sim.damage" && e["cat"] == "system" && e["args"]["tick"] == 67));
    assert!(
        !events.iter().any(|e| e["args"]["tick"] == 68),
        "three ticks, no more"
    );

    // A discrete scene advances on turns.
    let r = ok(&mut d, "scene.load", json!({ "scene": "delve" }));
    assert_eq!(r["clock"], "discrete");
    let st = ok(&mut d, "sim.advance", json!({ "by": "turn", "n": 2 }));
    assert_eq!(st["tick"], 2);
    assert_eq!(st["driver"], "discrete");
    let e = err(&mut d, "sim.advance", json!({ "by": "hour" }));
    assert_eq!(e.code, codes::BAD_PARAMS);
    let e = err(&mut d, "sim.run_until", json!({}));
    assert_eq!(e.code, codes::BAD_PARAMS);
    // More than a call's cap says it stopped short (phase 3 item 3: a
    // thirty-day run asked for in one call stopped at 100,000 ticks).
    let before = ok(&mut d, "sim.status", json!({}))["tick"]
        .as_u64()
        .unwrap();
    let r = ok(
        &mut d,
        "sim.run_until",
        json!({ "ticks": 5, "max_ticks": 2 }),
    );
    assert_eq!(r["tick"], before + 2);
    assert_eq!(r["hit"], false);
}

#[test]
fn scripts_reload_in_place_and_errors_are_data() {
    let t = temp("script");
    project(t.path());
    let mut d = open(t.path());
    let sys = ok(&mut d, "script.systems", json!({}));
    assert_eq!(sys["total"], 22);
    let dmg = &sys["rows"][8];
    assert_eq!(dmg["name"], "sim.damage");
    assert_eq!(dmg["stage"], "sim");
    assert_eq!(dmg["writes"], json!(["Health"]));
    assert_eq!(dmg["enabled"], true);
    // A script system runs on the sim thread, in no batch (phase 5 item 5).
    assert_eq!(dmg["parallel"], false);
    assert!(dmg["batch"].is_null());
    let threads = ok(&mut d, "sim.threads", json!({ "n": 1 }));
    assert_eq!(threads["threads"], 1);
    assert_eq!(threads["batches"], json!([]));
    let threads = ok(&mut d, "sim.threads", json!({ "n": 0 }));
    assert!(threads["threads"].as_u64().unwrap() >= 1, "one per core");
    assert_eq!(ok(&mut d, "script.errors", json!({}))["total"], 0);

    ok(&mut d, "sim.step", json!({ "n": 1 }));
    // Change the rule's event: the world state and tick survive the reload.
    let text = read(t.path(), "systems/damage.luau")
        .replace("cause = \"attrition\"", "cause = \"reloaded\"");
    write(t.path(), "systems/damage.luau", &text);
    let r = ok(&mut d, "script.reload", json!({}));
    assert_eq!(r["reloaded"], json!(["systems/damage.luau"]));
    assert_eq!(r["errors"], 0);
    assert_eq!(ok(&mut d, "sim.status", json!({}))["tick"], 1, "tick kept");
    let before = ok(&mut d, "world.get", json!({ "entity": "@player" }))["components"]["Health"]
        ["cur"]
        .clone();
    assert_eq!(before, 3);
    let bat = ok(&mut d, "world.get", json!({ "entity": "@bat" }));
    assert_eq!(
        bat["components"]["Health"]["cur"], 1,
        "one tick of the old rule"
    );
    ok(&mut d, "sim.step", json!({ "n": 1 }));
    let ev = ok(&mut d, "obs.events", json!({ "name": "death" }));
    assert_eq!(ev["rows"][0]["fields"]["cause"], "reloaded");

    // A syntax error is file:line data and the old system keeps running.
    write(
        t.path(),
        "systems/damage.luau",
        "system \"sim.damage\" { stage = \"sim\", run = function(q) if end }\n",
    );
    let r = ok(&mut d, "script.reload", json!({}));
    assert_eq!(r["errors"], 1);
    let errs = ok(&mut d, "script.errors", json!({}));
    assert_eq!(errs["rows"][0]["file"], "systems/damage.luau");
    assert_eq!(errs["rows"][0]["phase"], "syntax");
    assert!(errs["rows"][0]["ref"]
        .as_str()
        .unwrap()
        .starts_with("systems/damage.luau:"));
    assert_eq!(ok(&mut d, "session.status", json!({}))["script_errors"], 1);
}

/// Found building Hollow: a script that failed on its first tick was turned
/// off, and `script.errors` said nothing.
#[test]
fn a_system_a_fault_turned_off_is_listed_with_its_line() {
    let t = temp("fault");
    project(t.path());
    let mut d = open(t.path());
    write(
        t.path(),
        "systems/broken.luau",
        "system \"sim.broken\" {\n  stage = \"sim\",\n  run = function(q)\n    error(\"no such thing\")\n  end,\n}\n",
    );
    ok(&mut d, "script.reload", json!({}));
    assert_eq!(ok(&mut d, "script.errors", json!({}))["total"], 0);
    ok(&mut d, "sim.step", json!({ "n": 2 }));
    let errs = ok(&mut d, "script.errors", json!({}));
    let rows = errs["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{errs}");
    let row = &rows[0];
    assert_eq!(row["phase"], "run");
    assert_eq!(row["system"], "sim.broken");
    assert_eq!(row["file"], "systems/broken.luau");
    assert_eq!(row["line"], 4);
    assert!(row["message"].as_str().unwrap().contains("no such thing"));
    assert_eq!(ok(&mut d, "session.status", json!({}))["script_errors"], 1);

    // Fixed and reloaded, it runs again and the row is gone.
    write(
        t.path(),
        "systems/broken.luau",
        "system \"sim.broken\" { stage = \"sim\", run = function(q) end }\n",
    );
    ok(&mut d, "script.reload", json!({}));
    ok(&mut d, "sim.step", json!({ "n": 1 }));
    assert_eq!(ok(&mut d, "script.errors", json!({}))["total"], 0);
}

#[test]
fn rooms_stream_through_the_protocol() {
    let t = temp("rooms");
    project(t.path());
    let mut d = Daemon::new();
    let r = ok(
        &mut d,
        "session.open",
        json!({ "project": t.path().to_string_lossy(), "scene": "hollow" }),
    );
    assert_eq!(r["rooms"], json!(["a"]));
    assert_eq!(r["seed"], 3);
    let rooms = ok(&mut d, "scene.rooms", json!({}));
    assert_eq!(rooms["current"], "a");
    assert_eq!(rooms["rooms"][0]["loaded"], true);
    assert_eq!(rooms["rooms"][1]["loaded"], false);
    assert_eq!(rooms["rooms"][0]["neighbors"], json!(["b"]));
    let g = ok(&mut d, "world.get", json!({ "entity": "@a/left" }));
    assert_eq!(
        g["components"]["Door"]["leads_to"],
        serde_json::Value::Null,
        "b is not loaded"
    );
    let tr = ok(&mut d, "scene.transition", json!({ "room": "b" }));
    assert_eq!(tr["loaded"], json!(["b"]));
    assert_eq!(tr["unloaded"], json!(["a"]));
    let ev = ok(&mut d, "obs.events", json!({ "name": "room.transition" }));
    assert_eq!(ev["rows"][0]["fields"], json!({ "from": "a", "to": "b" }));
    let e = err(&mut d, "scene.transition", json!({ "room": "zz" }));
    assert_eq!(e.code, argus_scene::codes::UNKNOWN_ROOM);
    assert_eq!(
        ok(&mut d, "scene.list", json!({}))["scenes"],
        json!(["arena", "delve", "hollow"])
    );

    // Bulk data through scene.tiles.get.
    let mut b = argus_scene::Bulk::create(
        t.path(),
        "scenes/hollow/rooms/a/tiles.map.toml",
        "tiles",
        4,
        2,
        &["ground"],
        1,
    )
    .unwrap();
    b.set("ground", argus_scene::Rect::new(1, 0, 2, 1), &[7, 9])
        .unwrap();
    b.save().unwrap();
    let tiles = ok(
        &mut d,
        "scene.tiles.get",
        json!({ "file": "scenes/hollow/rooms/a/tiles.map.toml", "rect": { "x": 0, "y": 0, "w": 4, "h": 1 } }),
    );
    assert_eq!(tiles["cells"], json!([0, 7, 9, 0]));
    assert_eq!(tiles["layer"], "ground");

    // A scene with a broken file opens with issues and an empty world; scene.issues lists them.
    write(
        t.path(),
        "scenes/hollow/rooms/b/bad.toml",
        "[Nope]\nx = 1\n",
    );
    let r = ok(&mut d, "scene.load", json!({}));
    assert_eq!(r["entities"], 0);
    assert_eq!(r["issues"][0]["code"], argus_ecs::codes::UNKNOWN_COMPONENT);
    let iss = ok(&mut d, "scene.issues", json!({}));
    assert_eq!(iss["total"], 1);
    assert_eq!(
        iss["rows"][0]["data"]["ref"],
        "scenes/hollow/rooms/b/bad.toml:1"
    );
    // Fix the file and load again: clean.
    std::fs::remove_file(t.path().join("scenes/hollow/rooms/b/bad.toml")).unwrap();
    let r = ok(&mut d, "scene.load", json!({}));
    assert!(r.get("issues").is_none());
    assert_eq!(ok(&mut d, "scene.issues", json!({}))["total"], 0);
}

#[test]
fn a_scene_loaded_while_playing_plays_on_and_scenarios_start_stopped() {
    // Found by Kevin (2026-09-28): a playing window froze on `argus scene
    // load mods=[...]`, since a fresh scene's clock starts stopped.
    let t = temp("scene-play");
    project(t.path());
    let mut d = open(t.path());
    ok(&mut d, "sim.play", json!({}));
    ok(&mut d, "sim.speed", json!({ "x": 3 }));
    ok(&mut d, "scene.load", json!({}));
    let st = ok(&mut d, "sim.status", json!({}));
    assert_eq!(st["playing"], true, "{st}");
    assert_eq!(st["speed"], 3.0, "{st}");
    assert!(d.wants_frames());
    // Stopped, it loads stopped.
    ok(&mut d, "sim.pause", json!({}));
    ok(&mut d, "sim.stop", json!({}));
    ok(&mut d, "scene.load", json!({}));
    assert_eq!(ok(&mut d, "sim.status", json!({}))["playing"], false);

    // A scenario's load starts stopped though the session plays, and the
    // session plays on after the run.
    let scene = ok(&mut d, "session.status", json!({}))["scene"]
        .as_str()
        .unwrap()
        .to_string();
    let file = t.path().join("tests").join("playing.luau");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        &file,
        format!(
            "return scenario \"starts stopped\" {{\n  scene = \"{scene}\",\n  run = function(t)\n    if t:status().playing then error(\"playing\", 0) end\n    t:step(1)\n  end,\n}}\n"
        ),
    )
    .unwrap();
    ok(&mut d, "sim.play", json!({}));
    let r = ok(&mut d, "test.run", json!({}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(ok(&mut d, "sim.status", json!({}))["playing"], true);
}

/// Found by Kevin's hand check of the phase 4 gate: he kept a tuned value
/// while Outpost played (its file written), pressed Stop, and the game
/// went back to the value before it while the file kept his. Stop goes
/// back to where play started and makes again what was kept meanwhile:
/// commits, undos, and redos, in order, world only.
#[test]
fn what_was_kept_while_playing_stays_after_stop() {
    let t = temp("stop-keeps");
    project(t.path());
    let mut d = open(t.path());
    let commit = |d: &mut Daemon, note: &str, method: &str, params: serde_json::Value| {
        let tx = ok(d, "edit.begin", json!({ "author": "human", "note": note }))["tx"].clone();
        let mut p = params;
        p["tx"] = tx.clone();
        ok(d, method, p);
        ok(d, "edit.commit", json!({ "tx": tx }));
    };
    let field = |d: &mut Daemon, id: &str, c: &str, f: &str| {
        ok(d, "world.get", json!({ "entity": id }))["components"][c][f].clone()
    };
    // Before play: the player's hp set to 1, kept.
    commit(
        &mut d,
        "low",
        "edit.set",
        json!({ "entity": "@player", "path": "Health.cur", "value": 1 }),
    );
    ok(&mut d, "sim.play", json!({}));
    ok(&mut d, "sim.step", json!({ "n": 30 }));
    // While playing: the earlier change undone, redone, undone again; a
    // field kept, a light added, and an entity made with its file.
    ok(&mut d, "edit.undo", json!({ "author": "human" }));
    ok(&mut d, "edit.redo", json!({ "author": "human" }));
    ok(&mut d, "edit.undo", json!({ "author": "human" }));
    commit(
        &mut d,
        "tougher",
        "edit.set",
        json!({ "entity": "@player", "path": "Health.max", "value": 7 }),
    );
    commit(
        &mut d,
        "a light",
        "edit.add",
        json!({ "entity": "@player", "component": "Light" }),
    );
    commit(
        &mut d,
        "a crate",
        "edit.spawn",
        json!({ "id": "crate", "components": { "Transform": { "pos": [4.0, 0.0] }, "Health": { "cur": 9 } } }),
    );
    let st = ok(&mut d, "sim.stop", json!({}));
    assert_eq!(
        (st["playing"].clone(), st["tick"].clone()),
        (json!(false), json!(0))
    );
    // The world is the files: hp back to 3 (the undo), max 7, the light,
    // the crate.
    assert!(read(t.path(), "scenes/arena/player.toml").contains("max = 7"));
    assert_eq!(field(&mut d, "@player", "Health", "max"), 7, "kept in play");
    assert_eq!(
        field(&mut d, "@player", "Health", "cur"),
        3,
        "undone in play"
    );
    assert!(
        field(&mut d, "@player", "Light", "intensity").is_number(),
        "added in play"
    );
    assert_eq!(field(&mut d, "@crate", "Health", "cur"), 9, "made in play");
    // And the feed and undo go on as they were: undo takes the crate.
    ok(&mut d, "edit.undo", json!({ "author": "human" }));
    assert!(d
        .call(0, "world.get", &json!({ "entity": "@crate" }))
        .is_err());
    assert!(!t.path().join("scenes/arena/crate.toml").exists());
}
