//! The job system through the protocol and from scripts (phase 3 item 3,
//! §12, §8, §4, §6): jobs posted by a script from a data table, claimed
//! and done; why a worker is idle (jobs.why, obs.why on job.idle); the
//! queue as an aggregate; and a game asking for a speed.

mod common;

use common::*;
use serde_json::json;

/// A yard: a 6 x 3 grid, a baker (skill bake) and a hauler (no skills), an
/// oven, and a script that posts one bake job from defs/jobs.toml and asks
/// for 3x once.
fn yard(root: &std::path::Path) {
    write(
        root,
        "scenes/yard/scene.toml",
        "clock = \"continuous@60\"\nseed = 1\n",
    );
    write(
        root,
        "scenes/yard/grid.toml",
        "[NavGrid]\norigin = [-0.5, -0.5]\nwidth = 6\nheight = 3\n",
    );
    write(
        root,
        "scenes/yard/baker.toml",
        "[Transform]\npos = [0.0, 0.0]\n\n[Worker]\nskills = \"bake\"\n\n[NavAgent]\ngoal = [0.0, 0.0]\nspeed = 6.0\n",
    );
    write(
        root,
        "scenes/yard/hauler.toml",
        "[Transform]\npos = [0.0, 2.0]\n\n[Worker]\n\n[NavAgent]\ngoal = [0.0, 2.0]\nspeed = 6.0\n",
    );
    write(
        root,
        "scenes/yard/oven.toml",
        "[Transform]\npos = [5.0, 1.0]\n",
    );
    write(
        root,
        "defs/schemas.luau",
        r#"return def "jobs" {
  prefix = "job", key = "id",
  id = field.str {},
  priority = field.f32 { default = 0 },
  skill = field.str {},
  work = field.f32 { default = 1 },
}
"#,
    );
    write(
        root,
        "defs/jobs.toml",
        "[\"bake\"]\npriority = 4\nskill = \"bake\"\nwork = 0.5\n",
    );
    write(
        root,
        "systems/bakery.luau",
        r#"return system "sim.bakery" {
  stage = "sim",
  writes = { "Job" },
  run = function(q)
    if q.tick ~= 1 then return end
    local row = q:def("job:bake")
    local j = q:spawn({ "Job" }):get("Job")
    j.kind = row.key
    j.priority = row.priority
    j.requires = row.skill
    j.work = row.work
    j.site = q:entity("@oven")
    q:speed(3)
  end,
}
"#,
    );
}

#[test]
fn jobs_are_posted_from_a_table_claimed_done_and_explained() {
    let t = temp("jobs");
    project(t.path());
    yard(t.path());
    let mut d = open(t.path());
    let r = ok(&mut d, "scene.load", json!({ "scene": "yard" }));
    assert!(r.get("issues").is_none(), "{r}");
    assert_eq!(ok(&mut d, "script.errors", json!({}))["total"], 0);

    ok(&mut d, "sim.step", json!({ "n": 3 }));
    // The game asked for 3x: the clock's speed from the step after.
    assert_eq!(ok(&mut d, "sim.status", json!({}))["speed"], 3.0);
    // The job from the table: the baker has it; the hauler says why not.
    let q = ok(
        &mut d,
        "world.query",
        json!({ "group_by": ["Job.kind", "Job.stage"], "select": ["count()", "sum(Job.priority)"] }),
    );
    assert_eq!(
        q["columns"],
        json!(["Job.kind", "Job.stage", "count()", "sum(Job.priority)"])
    );
    assert_eq!(q["rows"], json!([["bake", "going", 1, 4.0]]));
    let why = ok(&mut d, "jobs.why", json!({ "worker": "@hauler" }));
    assert_eq!(why["job"], serde_json::Value::Null);
    assert_eq!(why["why"], "the one job is taken");
    assert_eq!(why["rows"][0][5], "@baker has it");
    let mine = ok(&mut d, "jobs.why", json!({ "worker": "@baker" }));
    assert!(mine["job"].as_str().unwrap().starts_with("e:"), "{mine}");
    assert_eq!(mine["rows"][0][5], "has it");
    assert!(mine.get("why").is_none(), "{mine}");

    // obs.why on the hauler's job.idle: the job system emitted it, and the
    // answer carries the worker's jobs judged now.
    let ow = ok(&mut d, "obs.why", json!({ "name": "job.idle" }));
    assert_eq!(ow["event"]["fields"]["worker"], "@hauler");
    let emitter = ow["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r[3] == "emitted it")
        .unwrap();
    assert_eq!(emitter[0], "jobs.update");
    assert_eq!(ow["jobs"]["why"], "the one job is taken");

    // Walked there and baked: done, and gone.
    ok(&mut d, "sim.step", json!({ "n": 120 }));
    let done = ok(&mut d, "obs.events", json!({ "name": "job.done" }));
    assert_eq!(done["total"], 1);
    assert_eq!(done["rows"][0]["fields"]["kind"], "bake");
    assert_eq!(done["rows"][0]["fields"]["worker"], "@baker");
    assert_eq!(done["rows"][0]["fields"]["site"], "@oven");
    let q = ok(
        &mut d,
        "world.query",
        json!({ "select": ["count()"], "where": { "has": ["Job"] } }),
    );
    assert_eq!(q["rows"], json!([[0]]));
    let why = ok(&mut d, "jobs.why", json!({ "worker": "@baker" }));
    assert_eq!(why["why"], "no jobs are posted");
    let e = err(&mut d, "jobs.why", json!({ "worker": "@oven" }));
    assert_eq!(e.code, argus_jobs::codes::NOT_A_WORKER);
}
