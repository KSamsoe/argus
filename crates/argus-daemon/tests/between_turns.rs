//! 2D staples item 5: a turn-based game moves between turns. Between
//! turns the continuous clock's idle ticks run only what moves between
//! turns (a tween here, and a script's system that asks to); on a turn
//! that takes no time; and the idle ticks are recorded with the input, so
//! a seek back runs the same ones again before the turn.

mod common;

use common::*;
use serde_json::{json, Value as Json};
use std::path::Path;

fn project(root: &Path) {
    write(
        root,
        "argus.toml",
        "name = \"turns\"\nclock = \"discrete\"\ndefault_scene = \"main\"\nplugins = []\n",
    );
    write(
        root,
        "input.toml",
        "[actions.wait]\ntype = \"button\"\n[actions.wait.bindings]\nkeyboard = [\"space\"]\n\n[slots.1]\ndevices = [\"keyboard\"]\n",
    );
    write(
        root,
        "components/count.luau",
        r#"component "Count" {
  doc = "What ran",
  turns = field.i32 {},
  idle = field.i32 {},
}
"#,
    );
    write(
        root,
        "systems/count.luau",
        r#"system "sim.turns" {
  stage = "sim",
  writes = { "Count" },
  doc = "Counts the turns.",
  run = function(q)
    local c = q:entity("@counter"):get("Count")
    c.turns = c.turns + 1
  end,
}

system "late.sway" {
  stage = "late",
  between_turns = true,
  writes = { "Count" },
  doc = "Counts the ticks time passed in.",
  run = function(q)
    if q.dt > 0 then
      local c = q:entity("@counter"):get("Count")
      c.idle = c.idle + 1
    end
  end,
}
"#,
    );
    write(
        root,
        "scenes/main/scene.toml",
        "clock = \"discrete\"\nseed = 1\n",
    );
    write(root, "scenes/main/counter.toml", "[Count]\n");
    write(
        root,
        "scenes/main/mover.toml",
        "[Transform]\npos = [0.0, 0.0]\n",
    );
    // A one-second slide from x 0 to 10, as a turn would start one.
    write(
        root,
        "scenes/main/slide.toml",
        "[Tween]\ntarget = \"@mover\"\npath = \"Transform.pos.x\"\nfrom = [0.0, 0.0]\nto = [10.0, 0.0]\nduration = 1.0\neasing = \"linear\"\n",
    );
}

fn x(d: &mut argus_daemon::Daemon) -> f64 {
    let g = ok(d, "world.get", json!({ "entity": "@mover" }));
    g["components"]["Transform"]["pos"][0].as_f64().unwrap()
}

fn count(d: &mut argus_daemon::Daemon, f: &str) -> Json {
    let q = ok(
        d,
        "world.query",
        json!({ "select": [format!("Count.{f}")], "where": { "has": ["Count"] } }),
    );
    q["rows"][0][0].clone()
}

#[test]
fn a_turn_based_game_moves_between_turns_and_a_seek_runs_the_same() {
    let t = temp("between-turns");
    project(t.path());
    let mut d = open(t.path());

    // A turn takes no time: the slide has not moved.
    ok(&mut d, "sim.advance", json!({ "n": 1 }));
    assert_eq!(x(&mut d), 0.0);

    // Half a second between turns: halfway, and no turn passed.
    let r = ok(&mut d, "sim.idle", json!({ "seconds": 0.5 }));
    assert_eq!(r["idled"], 30);
    assert!((x(&mut d) - 5.0).abs() < 1e-3, "{}", x(&mut d));
    assert_eq!(count(&mut d, "turns"), 1);
    assert_eq!(count(&mut d, "idle"), 30);

    // The next turn: still halfway; then the rest of the slide.
    ok(&mut d, "sim.advance", json!({ "n": 1 }));
    let at_turn = x(&mut d);
    assert!((at_turn - 5.0).abs() < 1e-3);
    ok(&mut d, "sim.idle", json!({ "ticks": 30 }));
    assert!((x(&mut d) - 10.0).abs() < 1e-3);
    assert_eq!(count(&mut d, "turns"), 2);
    assert_eq!(count(&mut d, "idle"), 60);

    // Back to the end of the first turn and forward one: the half second
    // recorded before the second turn runs again first.
    ok(&mut d, "sim.seek", json!({ "frame": 1 }));
    assert_eq!(x(&mut d), 0.0);
    ok(&mut d, "sim.advance", json!({ "n": 1 }));
    assert_eq!(x(&mut d), at_turn, "the same world");
    assert_eq!(count(&mut d, "idle"), 30);
}
