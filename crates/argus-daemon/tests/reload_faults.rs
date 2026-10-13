//! A reload switches every faulted system on again, its file changed or not
//! (issue 13: "fix the file and run argus reload" held only for a changed
//! file, though a fault can come from the run's data, a model's answer).
//! A system the scene's settings switched off stays off.

mod common;

use common::*;
use serde_json::json;

#[test]
fn a_reload_switches_faulted_systems_on_and_leaves_the_scenes_off() {
    let t = temp("reload-faults");
    project(t.path());
    write(
        t.path(),
        "scenes/arena/scene.toml",
        "clock = \"continuous@60\"\nseed = 7\n\n[systems]\ndisable = [\"sim.quiet\"]\n",
    );
    write(
        t.path(),
        "systems/picky.luau",
        r#"return {
  system "sim.picky" {
    stage = "sim",
    doc = "Fails on the third tick only, as a bad answer from outside would.",
    run = function(q)
      if q.tick == 2 then error("tick two") end
    end,
  },
  system "sim.quiet" {
    stage = "sim",
    doc = "Switched off by the scene.",
    run = function(q) end,
  },
}"#,
    );
    let mut d = open(t.path());
    ok(&mut d, "sim.step", json!({ "n": 4 }));
    let disabled = |d: &mut argus_daemon::Daemon| {
        let mut v: Vec<String> = ok(d, "world.summary", json!({}))["disabled"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                s.as_str()
                    .unwrap_or_else(|| s["name"].as_str().unwrap())
                    .to_string()
            })
            .collect();
        v.sort();
        v
    };
    assert_eq!(disabled(&mut d), ["sim.picky", "sim.quiet"]);
    let r = ok(&mut d, "script.reload", json!({}));
    assert_eq!(r["reloaded"], json!([]), "no file changed: {r}");
    assert_eq!(r["switched_on"], json!(["sim.picky"]), "{r}");
    assert_eq!(disabled(&mut d), ["sim.quiet"], "the scene's stays off");
    ok(&mut d, "sim.step", json!({ "n": 2 }));
    assert_eq!(disabled(&mut d), ["sim.quiet"], "past tick two it runs");
}
