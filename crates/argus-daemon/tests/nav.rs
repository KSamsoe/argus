//! Navigation through the protocol and from scripts (phase 3 item 2, §12):
//! nav.grids, nav.grid, nav.path, nav.field, nav.bake; q:path, q:walkable,
//! q:distance, q:downhill; a NavAgent walking to an entity; an edit that
//! closes the way.

mod common;

use common::*;
use serde_json::json;

/// A maze scene: a 7 x 5 grid, a wall down column 3 with a gap at the top
/// row, the hero at the bottom left with a flow field, a goal at the bottom
/// right, a walker with an agent, and a script that asks the path queries.
fn maze(root: &std::path::Path) {
    write(
        root,
        "scenes/maze/scene.toml",
        "clock = \"continuous@60\"\nseed = 1\n",
    );
    write(
        root,
        "scenes/maze/grid.toml",
        "[NavGrid]\norigin = [-0.5, -0.5]\nwidth = 7\nheight = 5\n",
    );
    for r in 0..4 {
        write(
            root,
            &format!("scenes/maze/wall{r}.toml"),
            &format!("[Transform]\npos = [3.0, {r}.0]\n\n[NavCost]\n"),
        );
    }
    write(
        root,
        "scenes/maze/hero.toml",
        "[Transform]\npos = [0.0, 0.0]\n\n[NavField]\ngrid = \"@grid\"\n",
    );
    write(
        root,
        "scenes/maze/goal.toml",
        "[Transform]\npos = [6.0, 0.0]\n",
    );
    write(
        root,
        "scenes/maze/walker.toml",
        "[Transform]\npos = [0.0, 4.0]\n\n[NavAgent]\ntarget = \"@goal\"\nspeed = 6.0\n",
    );
    write(
        root,
        "systems/probe.luau",
        r#"return system "sim.probe" {
  stage = "sim",
  reads = { "Transform", "NavGrid", "NavField" },
  run = function(q)
    local hero, goal = q:entity("@hero"), q:entity("@goal")
    local pts, cost = q:path(hero, goal)
    local down = q:downhill(hero, vector.create(6, 0, 0))
    q:emit("probe", {
      points = pts and #pts or -1,
      cost = cost or -1,
      last_x = pts and pts[#pts].x or -1,
      wall = q:walkable(vector.create(3, 1, 0)),
      gap = q:walkable(vector.create(3, 4, 0)),
      far = q:distance(hero, vector.create(6, 0, 0)) or -1,
      down_x = down and down.x or -1,
      down_y = down and down.y or -1,
    })
  end,
}
"#,
    );
}

fn last_event(d: &mut argus_daemon::Daemon, name: &str) -> serde_json::Value {
    let ev = ok(d, "obs.events", json!({ "name": name }));
    let rows = ev["rows"].as_array().unwrap();
    rows.last().cloned().unwrap_or_default()
}

#[test]
fn a_maze_is_searched_through_the_protocol_and_from_scripts() {
    let t = temp("nav");
    project(t.path());
    maze(t.path());
    let mut d = open(t.path());
    let r = ok(&mut d, "scene.load", json!({ "scene": "maze" }));
    assert!(r.get("issues").is_none(), "{r}");

    // The grid: 35 cells, the wall's 4 blocked.
    let g = ok(&mut d, "nav.grids", json!({}));
    assert_eq!(g["total"], 1);
    assert_eq!(g["rows"][0][0], "@grid");
    assert_eq!(g["rows"][0][3], json!([7, 5]));
    assert_eq!(
        (g["rows"][0][4].clone(), g["rows"][0][5].clone()),
        (json!(31), json!(4))
    );
    let drawn = ok(&mut d, "nav.grid", json!({}));
    assert_eq!(
        drawn["rows"],
        json!([".......", "...#...", "...#...", "...#...", "...#..."])
    );

    // A path: up to the gap and down again, 14 steps.
    let p = ok(
        &mut d,
        "nav.path",
        json!({ "from": "@hero", "to": "@goal" }),
    );
    assert_eq!(p["found"], true, "{p}");
    assert_eq!(
        (p["steps"].clone(), p["cost"].clone()),
        (json!(14), json!(14.0))
    );
    assert_eq!(
        p["points"].as_array().unwrap().last(),
        Some(&json!([6.0, 0.0]))
    );
    assert_eq!(p["from_cell"], json!([0, 0]));
    // Asked again: from the cache.
    let again = ok(&mut d, "nav.path", json!({ "from": [0, 0], "to": [6, 0] }));
    assert_eq!(again["cached"], true);
    // Into the wall: no; off every grid: no grid.
    let wall = ok(&mut d, "nav.path", json!({ "from": "@hero", "to": [3, 1] }));
    assert_eq!(wall["found"], false);
    assert_eq!(wall["why"], "the goal's cell is blocked");
    let off = ok(
        &mut d,
        "nav.path",
        json!({ "from": [50, 50], "to": [0, 0] }),
    );
    assert_eq!(
        (off["found"].clone(), off["grid"].clone()),
        (json!(false), json!(null))
    );
    let e = err(
        &mut d,
        "nav.path",
        json!({ "from": "@hero", "to": [50, 50] }),
    );
    assert_eq!(e.code, argus_nav::codes::OFF_GRID);

    // The hero's flow field: the goal is 14 steps away.
    let f = ok(&mut d, "nav.field", json!({ "of": "@hero" }));
    assert_eq!(f["reached"], 31);
    assert_eq!(f["target"], json!([0, 0]));
    assert_eq!(f["rows"][4], json!("012#cde"), "{f}");

    // Scripts ask the same, and the walker walks to the goal.
    ok(&mut d, "sim.step", json!({ "n": 1 }));
    let probe = last_event(&mut d, "probe");
    let fields = &probe["fields"];
    assert_eq!(fields["points"], 14, "{probe}");
    assert_eq!(fields["cost"].as_f64(), Some(14.0));
    assert_eq!(fields["last_x"].as_f64(), Some(6.0));
    assert_eq!(
        (fields["wall"].clone(), fields["gap"].clone()),
        (json!(false), json!(true))
    );
    assert_eq!(fields["far"].as_f64(), Some(14.0));
    // West and north are both 13 steps away; ties go east, west, north,
    // south.
    let down = (fields["down_x"].as_f64(), fields["down_y"].as_f64());
    assert_eq!(down, (Some(5.0), Some(0.0)));
    let a = ok(&mut d, "world.get", json!({ "entity": "@walker" }));
    assert_eq!(a["components"]["NavAgent"]["state"], "moving");
    ok(&mut d, "sim.step", json!({ "n": 120 }));
    let a = ok(&mut d, "world.get", json!({ "entity": "@walker" }));
    assert_eq!(a["components"]["NavAgent"]["state"], "arrived", "{a}");
    assert_eq!(a["components"]["Transform"]["pos"], json!([6.0, 0.0]));
    let arrived = ok(&mut d, "obs.events", json!({ "name": "nav.arrived" }));
    assert_eq!(arrived["total"], 1);
    // obs.why can name the system that moved it.
    let who = ok(
        &mut d,
        "world.who_writes",
        json!({ "component": "NavAgent" }),
    );
    assert!(who.to_string().contains("nav.agents"), "{who}");

    // An edit closes the gap: the way is gone at once, between ticks.
    commit(
        &mut d,
        "agent",
        "close the gap",
        &[(
            "edit.spawn",
            json!({ "id": "plug", "components": { "Transform": { "pos": [3.0, 4.0] }, "NavCost": {} } }),
        )],
    );
    let shut = ok(
        &mut d,
        "nav.path",
        json!({ "from": "@hero", "to": "@goal" }),
    );
    assert_eq!(shut["found"], false, "{shut}");
    assert_eq!(shut["why"], "the goal cannot be reached from the start");

    // A bake: an index and a chunk, which the scene loader leaves alone.
    let b = ok(&mut d, "nav.bake", json!({ "grid": "@grid" }));
    assert_eq!(b["index"], "scenes/maze/grid.navgrid.toml");
    assert_eq!(
        (b["bytes"].clone(), b["blocked"].clone()),
        (json!(35), json!(5))
    );
    assert!(read(t.path(), "scenes/maze/grid.navgrid.toml").contains("width = 7"));
    let r = ok(&mut d, "scene.load", json!({ "scene": "maze" }));
    assert!(r.get("issues").is_none(), "{r}");
}

#[test]
fn a_system_that_asks_for_a_path_must_say_it_reads_the_grid() {
    let t = temp("nav-rude");
    project(t.path());
    maze(t.path());
    write(
        t.path(),
        "systems/rude.luau",
        r#"return system "sim.rude" {
  stage = "sim",
  reads = { "Transform" },
  run = function(q)
    q:path(q:entity("@hero"), q:entity("@goal"))
  end,
}
"#,
    );
    let mut d = open(t.path());
    ok(&mut d, "scene.load", json!({ "scene": "maze" }));
    ok(&mut d, "sim.step", json!({ "n": 1 }));
    let errs = ok(&mut d, "script.errors", json!({}));
    let text = errs.to_string();
    assert!(
        text.contains("sim.rude") && text.contains("NavGrid"),
        "{text}"
    );
}
