//! `test.run` edits the live world, never the project: a scenario that
//! moves an authored entity, despawns one, and spawns one leaves every file
//! as it was, a spawn that would need a file is refused, and the scene is
//! reloaded afterwards. (Hollow's planks test once rewrote the player's
//! file through `edit.commit`.)

mod common;

use common::*;
use serde_json::json;

#[test]
fn a_scenario_edits_the_world_and_leaves_the_files() {
    let t = temp("test-run-edits");
    let root = t.path();
    project(root);
    write(
        root,
        "tests/edits.luau",
        r#"return scenario "edits stay in the world" {
  scene = "arena", seed = 7,
  run = function(t)
    local tx = t:call("edit.begin", { author = "test:edits", note = "move, despawn, spawn" }).tx
    t:call("edit.set", { tx = tx, entity = "@player", path = "Transform.pos", value = { 5, 6 } })
    t:call("edit.despawn", { tx = tx, entity = "@bat" })
    t:call("edit.spawn", { tx = tx, components = { Health = { cur = 1 } } })
    t:call("edit.commit", { tx = tx })
    t:step(1)
    t.expect:field("@player", "Transform.pos.x"):near(5, 0.001)
    t.expect:count({ has = { "Enemy" } }):eq(0)

    local tx2 = t:call("edit.begin", { author = "test:edits", note = "an id is a file" }).tx
    t:call("edit.spawn", { tx = tx2, id = "ghost", components = { Health = { cur = 1 } } })
    local ok, err = pcall(function() t:call("edit.commit", { tx = tx2 }) end)
    assert(not ok and string.find(err, "a test cannot spawn @ghost", 1, true), tostring(err))
  end,
}
"#,
    );
    let files = [
        "scenes/arena/player.toml",
        "scenes/arena/bat.toml",
        "scenes/arena/door.toml",
    ];
    let before: Vec<String> = files.iter().map(|f| read(root, f)).collect();
    let mut d = open(root);
    let r = ok(&mut d, "test.run", json!({}));
    assert_eq!(r["ok"], true, "{r:#}");
    assert_eq!(r["passed_runs"], 1, "{r:#}");

    let after: Vec<String> = files.iter().map(|f| read(root, f)).collect();
    assert_eq!(before, after, "a test wrote the scene's files");
    assert!(!root.join("scenes/arena/ghost.toml").exists());

    // The scene the test ran in was reloaded: the player is back, the bat too.
    let p = ok(&mut d, "world.get", json!({ "entity": "@player" }));
    assert_eq!(
        p["components"]["Transform"]["pos"],
        json!([0.0, 0.0]),
        "{p}"
    );
    let bat = ok(&mut d, "world.get", json!({ "entity": "@bat" }));
    assert_eq!(bat["authored"], true, "{bat}");
}
