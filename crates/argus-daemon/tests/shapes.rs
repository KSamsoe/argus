//! 2D staples item 2 through a project: a scene gives colliders a polygon
//! (`points`, a list of vec2 on a Rust component) and a capsule; the
//! protocol shows them; a script's ray meets them by their shapes; and
//! the overlap tier reports a touch only where the shape is.

mod common;

use common::*;
use serde_json::{json, Value as Json};
use std::path::Path;

fn project(root: &Path) {
    write(
        root,
        "argus.toml",
        "name = \"shapes\"\nclock = \"continuous@60\"\ndefault_scene = \"main\"\nplugins = []\n",
    );
    write(
        root,
        "components/seen.luau",
        r#"component "Seen" {
  doc = "Where the eye's rays met something",
  wedge = field.vec2 {},
  pill = field.vec2 {},
}
"#,
    );
    write(
        root,
        "systems/look.luau",
        r#"system "sim.look" {
  stage = "sim",
  reads = { "Collider", "Transform" },
  writes = { "Seen" },
  doc = "Looks down at the wedge and across at the pill.",
  run = function(q)
    local s = q:entity("@eye"):get("Seen")
    local down = q:raycast(vector.create(-1, 10, 0), vector.create(-1, -10, 0), { tiles = false })
    if down then s.wedge = down.point end
    local across = q:raycast(vector.create(10, 3, 0), vector.create(30, 3, 0), { tiles = false })
    if across then s.pill = across.point end
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
        "[Transform]\npos = [0.0, 50.0]\n\n[Seen]\n",
    );
    // A wedge: its left face runs from (-2, 0) up to (0, 2).
    write(
        root,
        "scenes/main/wedge.toml",
        "[Transform]\npos = [0.0, 0.0]\n\n[Collider]\npoints = [[-2.0, 0.0], [2.0, 0.0], [0.0, 2.0]]\n",
    );
    write(
        root,
        "scenes/main/pill.toml",
        "[Transform]\npos = [20.0, 3.0]\n\n[Collider]\nradius = 0.5\nheight = 3.0\n",
    );
    // A probe inside the wedge's box, outside the wedge.
    write(
        root,
        "scenes/main/probe.toml",
        "[Transform]\npos = [1.6, 1.6]\n\n[Collider]\nradius = 0.1\n",
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

fn near(v: &Json, x: f64, y: f64) -> bool {
    (v[0].as_f64().unwrap() - x).abs() < 1e-3 && (v[1].as_f64().unwrap() - y).abs() < 1e-3
}

#[test]
fn a_scene_gives_colliders_polygons_and_capsules() {
    let t = temp("shapes");
    project(t.path());
    let mut d = open(t.path());

    let wedge = ok(&mut d, "world.get", json!({ "entity": "@wedge" }));
    assert_eq!(
        wedge["components"]["Collider"]["points"],
        json!([[-2.0, 0.0], [2.0, 0.0], [0.0, 2.0]]),
        "{wedge}"
    );

    ok(&mut d, "sim.step", json!({ "n": 1 }));
    // Down at x -1 onto the left face: y 1 there.
    let w = seen(&mut d, "wedge");
    assert!(near(&w, -1.0, 1.0), "{w}");
    // Across at the pill's side, half a unit from its centre.
    let p = seen(&mut d, "pill");
    assert!(near(&p, 19.5, 3.0), "{p}");

    // The probe sits in the wedge's box but not in the wedge: no touch.
    let ev = ok(&mut d, "obs.events", json!({ "name": "overlap.begin" })).to_string();
    assert!(!ev.contains("@probe"), "{ev}");
    // Moved inside, it touches.
    commit(
        &mut d,
        "agent:claude",
        "into the wedge",
        &[(
            "edit.set",
            json!({ "entity": "@probe", "path": "Transform.pos", "value": [0.5, 0.5] }),
        )],
    );
    ok(&mut d, "sim.step", json!({ "n": 1 }));
    let ev = ok(&mut d, "obs.events", json!({ "name": "overlap.begin" })).to_string();
    assert!(ev.contains("@probe") && ev.contains("@wedge"), "{ev}");
}
