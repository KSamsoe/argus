//! 2D staples item 0 through a script: q:raycast, q:raycast_all,
//! q:shape_cast, and q:overlap against the scene's colliders, and the
//! reads a query declares.

mod common;

use common::*;
use serde_json::{json, Value as Json};
use std::path::Path;

fn project(root: &Path) {
    write(
        root,
        "argus.toml",
        "name = \"casts\"\nclock = \"continuous@60\"\ndefault_scene = \"main\"\nplugins = []\n",
    );
    write(
        root,
        "components/seen.luau",
        r#"component "Seen" {
  doc = "What the eye's queries found",
  ray = field.str {},
  point = field.vec2 {},
  normal = field.vec2 {},
  all = field.i32 {},
  stop = field.vec2 {},
  near = field.list { of = "str" },
}
"#,
    );
    write(
        root,
        "systems/look.luau",
        r#"system "sim.look" {
  stage = "sim",
  reads = { "Collider", "Tilemap", "Transform" },
  writes = { "Seen" },
  doc = "The eye looks right, sweeps a ball right, and feels around itself.",
  run = function(q)
    local eye = q:entity("@eye")
    local s = eye:get("Seen")
    local h = q:raycast(vector.create(0, 0, 0), vector.create(20, 0, 0), { ignore = eye })
    if h then
      s.ray = h.entity:name() or ""
      s.point = h.point
      s.normal = h.normal
    end
    s.all = #q:raycast_all(eye, vector.create(20, 0, 0), { ignore = eye })
    local stop = q:shape_cast({ radius = 0.5 }, eye, vector.create(20, 0, 0), { ignore = { eye } })
    if stop then s.stop = stop.position end
    local near = {}
    for _, o in ipairs(q:overlap({ radius = 3 }, eye, { ignore = eye })) do
      table.insert(near, o.entity:name() or "")
    end
    s.near = near
  end,
}
"#,
    );
    write(
        root,
        "systems/careless.luau",
        r#"system "sim.careless" {
  stage = "sim",
  doc = "Asks a query without saying it reads colliders.",
  run = function(q)
    if q.tick == 3 then q:raycast(vector.create(0, 0, 0), vector.create(1, 0, 0)) end
  end,
}
"#,
    );
    write(
        root,
        "scenes/main/scene.toml",
        "clock = \"continuous@60\"\nseed = 1\n",
    );
    write(
        root,
        "scenes/main/eye.toml",
        "[Transform]\npos = [0.0, 0.0]\n\n[Collider]\nsize = [1.0, 1.0]\n\n[Seen]\n",
    );
    write(
        root,
        "scenes/main/post.toml",
        "[Transform]\npos = [2.0, 1.2]\n\n[Collider]\nradius = 1.0\n",
    );
    write(
        root,
        "scenes/main/wall.toml",
        "[Transform]\npos = [8.0, 0.0]\n\n[Collider]\nsize = [2.0, 6.0]\n",
    );
    write(
        root,
        "scenes/main/far.toml",
        "[Transform]\npos = [14.0, 0.0]\n\n[Collider]\nsize = [1.0, 1.0]\n",
    );
}

fn seen(d: &mut argus_daemon::Daemon, field: &str) -> Json {
    let q = ok(
        d,
        "world.query",
        json!({ "select": [format!("Seen.{field}")], "where": { "has": ["Seen"] } }),
    );
    q["rows"][0][0].clone()
}

#[test]
fn a_script_asks_what_a_line_and_a_shape_meet() {
    let t = temp("casts");
    project(t.path());
    let mut d = open(t.path());
    ok(&mut d, "sim.step", json!({ "n": 2 }));
    // The post at (2, 1.2) radius 1 reaches down to y 0.2: the line at y 0
    // passes under it and meets the wall's face at x 7.
    assert_eq!(seen(&mut d, "ray"), "wall");
    assert_eq!(seen(&mut d, "point"), json!([7.0, 0.0]));
    assert_eq!(seen(&mut d, "normal"), json!([-1.0, 0.0]));
    assert_eq!(seen(&mut d, "all"), 2, "the wall and the far box");
    // A ball of radius 0.5 rolling right clips the post: it stops where
    // their centres are 1.5 apart, sqrt(1.5^2 - 1.2^2) = 0.9 short of x 2.
    let stop = seen(&mut d, "stop");
    let x = stop[0].as_f64().unwrap();
    assert!((x - 1.1).abs() < 1e-4, "{stop}");
    assert_eq!(
        seen(&mut d, "near"),
        json!(["post"]),
        "only the post is within 3"
    );

    // A system that asks without declaring the reads is switched off, saying so.
    ok(&mut d, "sim.step", json!({ "n": 2 }));
    let errs = ok(&mut d, "script.errors", json!({})).to_string();
    assert!(
        errs.contains("sim.careless") && errs.contains("reads Collider"),
        "{errs}"
    );
}
