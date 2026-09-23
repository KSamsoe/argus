//! Phase 1 item 10 through the protocol: a torch plays an effect asset for
//! as long as the scene runs, a script throws sparks where a hit lands with
//! a particles.burst event, the pools show up as Emitter fields and in
//! obs.frame, and editing the effect file changes the torch on the next
//! import (§13 "Particles and VFX", §14 assets).

mod common;

use common::*;
use serde_json::json;
use std::path::Path;

fn project(root: &Path) {
    write(
        root,
        "argus.toml",
        "name = \"particles\"\nclock = \"continuous@60\"\ndefault_scene = \"room\"\nplugins = []\n",
    );
    write(
        root,
        "assets/fx/flame.toml",
        "type = \"emitter\"\nrate = 30\nlifetime = 0.5\nspeed = [0.5, 1]\ndirection = 90\nspread = 30\nsize = [[0, 0.3], [1, 0]]\ncolor = [[0, \"#FFD070FF\"], [1, \"#FF402000\"]]\nz = 1\n",
    );
    write(
        root,
        "assets/fx/sparks.toml",
        "type = \"emitter\"\nlifetime = 0.25\nspeed = [4, 8]\ngravity = [0, -20]\nparticle = \"circle\"\nsize = 0.15\ncolor = \"#FFFFA0\"\nz = 2\n",
    );
    write(
        root,
        "systems/hits.luau",
        r#"system "sim.hits" {
  stage = "sim",
  doc = "A hit lands on tick 10: sparks there.",
  run = function(q)
    if q.tick == 10 then
      q:emit("particles.burst", { emitter = q:entity("@sparks"), count = 12, pos = vector.create(6, 2, 0) })
    end
  end,
}
"#,
    );
    write(
        root,
        "scenes/room/scene.toml",
        "clock = \"continuous@60\"\nseed = 3\n",
    );
    write(
        root,
        "scenes/room/camera.toml",
        "[Transform]\npos = [4.0, 2.0]\n\n[Camera]\nheight = 10.0\n",
    );
    write(
        root,
        "scenes/room/torch.toml",
        "[Transform]\npos = [2.0, 1.0]\n\n[Emitter]\neffect = \"fx/flame\"\n",
    );
    write(
        root,
        "scenes/room/sparks.toml",
        "[Transform]\npos = [0.0, 0.0]\n\n[Emitter]\neffect = \"fx/sparks\"\nplaying = false\n",
    );
    write(
        root,
        "scenes/room/typo.toml",
        "[Transform]\npos = [0.0, 0.0]\n\n[Emitter]\neffect = \"fx/flam\"\n",
    );
}

fn emitter(d: &mut argus_daemon::Daemon, id: &str) -> serde_json::Value {
    ok(d, "world.get", json!({ "entity": id }))["components"]["Emitter"].clone()
}

#[test]
fn a_torch_burns_sparks_fly_and_an_edit_to_the_effect_shows_next_import() {
    let t = temp("particles");
    project(t.path());
    let mut d = open(t.path());

    let fx = ok(&mut d, "asset.list", json!({ "kind": "emitter" }));
    assert_eq!(fx["total"], 2, "{fx}");

    // The sparks: twelve at the hit, gone a quarter second later.
    ok(&mut d, "sim.step", json!({ "n": 11 }));
    let s = emitter(&mut d, "@sparks");
    assert_eq!(
        (s["live"].clone(), s["emitted"].clone()),
        (json!(12), json!(12)),
        "{s}"
    );
    assert_eq!(s["started"], false, "not playing; the burst came by event");
    ok(&mut d, "sim.step", json!({ "n": 20 }));
    assert_eq!(emitter(&mut d, "@sparks")["live"], 0);

    // The torch: 30 a second that live half a second, so 15 at a time.
    ok(&mut d, "sim.step", json!({ "n": 29 }));
    let torch = emitter(&mut d, "@torch");
    assert_eq!(torch["live"], 15, "{torch}");
    assert_eq!(
        torch["emitted"],
        30 - 1,
        "the rate starts the tick after the start"
    );

    // The misspelt effect is reported once, naming the real ones.
    let err = ok(&mut d, "obs.events", json!({ "name": "particles.error" }));
    assert_eq!(err["total"], 1);
    assert_eq!(err["rows"][0]["fields"]["emitter"], "@typo");
    let msg = err["rows"][0]["fields"]["message"].as_str().unwrap();
    assert!(
        msg.contains("fx/flame") && msg.contains("fx/sparks"),
        "{msg}"
    );

    // The system owns the counters, and who_writes says so.
    let w = ok(
        &mut d,
        "world.who_writes",
        json!({ "component": "Emitter" }),
    );
    assert_eq!(w["systems"], json!(["particles.update"]));

    // A frame draws them.
    if let Some(f) = frame(&mut d, json!({ "width": 320, "height": 180 })) {
        assert_eq!(f["particles"], 15, "{f}");
    }

    // Edit the effect: double the rate, and the torch doubles.
    write(
        t.path(),
        "assets/fx/flame.toml",
        "type = \"emitter\"\nrate = 60\nlifetime = 0.5\nspeed = [0.5, 1]\ndirection = 90\nspread = 30\nsize = [[0, 0.3], [1, 0]]\nz = 1\n",
    );
    let r = ok(&mut d, "asset.import", json!({}));
    assert!(
        r["imported"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n == "fx/flame"),
        "{r}"
    );
    ok(&mut d, "sim.step", json!({ "n": 60 }));
    assert_eq!(emitter(&mut d, "@torch")["live"], 30);
}
