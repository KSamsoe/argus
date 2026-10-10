//! Scripts spawning from a prefab (GitHub issue 1), through the daemon and
//! the project's own `prefabs/`: a system spawns `enemy/bat` with the
//! prefab's tag and fields, a seek and the steps after it spawn the same
//! bats in the same places, and a prefab changed by an edit spawns as it
//! is now.

mod common;

use common::*;
use serde_json::{json, Value as Json};

/// Every tenth tick a bat, somewhere the run's random stream picks.
const SPAWNER: &str = r#"return system "sim.spawner" {
  stage = "sim",
  spawns = { "enemy/bat" },
  rng = "bats",
  doc = "A bat every tenth tick, at a column the seed picks.",
  run = function(q)
    if q.tick % 10 == 0 then
      local x = q:rng():range(0, 20)
      q:spawn_prefab("enemy/bat", { ["Transform.pos"] = vector.create(x, 5, 0) })
    end
  end,
}
"#;

fn bats(d: &mut argus_daemon::Daemon) -> Json {
    ok(
        d,
        "world.query",
        json!({ "select": ["Transform.pos", "Health.cur", "Health.max"], "where": { "has": ["Enemy"] }, "limit": 1000 }),
    )["rows"]
        .clone()
}

#[test]
fn a_script_spawns_a_prefab_and_a_seek_spawns_it_again() {
    let t = temp("spawn-prefab");
    project(t.path());
    // The bat prefab places nothing itself: it has no Transform.
    write(
        t.path(),
        "prefabs/enemy/bat.toml",
        "tags = [\"Enemy\"]\n\n[Transform]\n\n[Health]\ncur = 2\nmax = 2\n",
    );
    write(t.path(), "systems/spawner.luau", SPAWNER);
    let mut d = open(t.path());
    let start = bats(&mut d).as_array().unwrap().len();

    ok(&mut d, "sim.step", json!({ "n": 25 }));
    let at25 = bats(&mut d);
    let rows = at25.as_array().unwrap();
    assert_eq!(rows.len(), start + 3, "ticks 0, 10, and 20: {at25}");
    let spawned: Vec<&Json> = rows.iter().filter(|r| r[0][1] == json!(5.0)).collect();
    assert_eq!(spawned.len(), 3, "{at25}");
    for r in &spawned {
        assert_eq!(r[2], json!(2), "the prefab's maximum: {r}");
    }
    let faults = ok(&mut d, "script.errors", json!({}));
    assert_eq!(faults["total"], json!(0), "{faults}");

    // Back to tick 12 and on to 25: the same bats, in the same places.
    ok(&mut d, "sim.seek", json!({ "frame": 12 }));
    ok(&mut d, "sim.step", json!({ "n": 13 }));
    assert_eq!(bats(&mut d), at25);

    // The prefab, changed: the next bat spawns as it is now. (The edit
    // reapplies a prefab to the entities authored from it; the bats a script
    // spawned before it keep what they were spawned with.)
    commit(
        &mut d,
        "agent:claude",
        "tougher bats",
        &[(
            "edit.prefab",
            json!({ "prefab": "enemy/bat", "path": "Health.max", "value": 7 }),
        )],
    );
    ok(&mut d, "sim.step", json!({ "n": 10 }));
    let later = bats(&mut d);
    let maxes: Vec<Json> = later
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r[0][1] == json!(5.0))
        .map(|r| r[2].clone())
        .collect();
    assert_eq!(maxes.len(), 4, "{later}");
    let count = |v: i64| maxes.iter().filter(|m| **m == json!(v)).count();
    assert_eq!((count(2), count(7)), (3, 1), "{later}");
}
