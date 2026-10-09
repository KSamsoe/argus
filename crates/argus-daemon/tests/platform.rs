//! The platform through the protocol (phase 5 item 9): the daemon runs a
//! game on the null platform, which `platform.status` shows; a save goes
//! to its cloud and loads from there when the disk has none; the overlay
//! is an input; and a seek replays across the game's requests.

use argus_daemon::Daemon;
use serde_json::{json, Value as Json};
use std::path::{Path, PathBuf};

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn game(tag: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("daemon-platform-{tag}"));
    let _ = std::fs::remove_dir_all(&root);
    write(
        &root,
        "argus.toml",
        "name = \"plat\"\nclock = \"continuous@60\"\ndefault_scene = \"s\"\nplugins = []\nmodules = [\"steam\"]\n",
    );
    write(
        &root,
        "platform.toml",
        "[steam]\napp_id = 480\n\n[achievements.early]\nname = \"Early\"\nsteam = \"ACH_WIN_ONE_GAME\"\n",
    );
    write(
        &root,
        "input.toml",
        "[actions.pause]\ntype = \"button\"\n[actions.pause.bindings]\nkeyboard = [\"escape\"]\nplatform = [\"overlay\"]\n",
    );
    write(
        &root,
        "scenes/s/scene.toml",
        "clock = \"continuous@60\"\nseed = 1\n",
    );
    write(
        &root,
        "scenes/s/player.toml",
        "[PlayerSlot]\nslot = 1\n\n[Actions]\n\n[Transform]\n",
    );
    write(
        &root,
        "systems/plan.luau",
        r#"return system "sim.plan" {
  stage = "sim",
  reads = { "Actions" },
  writes = { "Transform" },
  run = function(q)
    for _, a, t in q:each("Actions", "Transform") do
      t.pos = vector.create(q.tick, if a.pause then 1 else 0, 0)
    end
    if q.tick == 8 then
      q:achieve("early")
      q:achieve("missing")
      q:presence("status", "Early on")
    end
  end,
}
"#,
    );
    root
}

fn open(root: &Path) -> Daemon {
    let mut d = Daemon::new();
    d.call(
        0,
        "session.open",
        &json!({ "project": root.canonicalize().unwrap().to_string_lossy() }),
    )
    .unwrap();
    d
}

fn events(d: &mut Daemon, name: &str) -> Vec<Json> {
    d.call(0, "obs.events", &json!({ "name": name, "limit": 50 }))
        .unwrap()["rows"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

#[test]
fn the_null_platform_takes_the_games_requests_and_says_so() {
    let root = game("status");
    let mut d = open(&root);
    d.call(0, "sim.step", &json!({ "n": 10 })).unwrap();
    let s = d.call(0, "platform.status", &json!({})).unwrap();
    assert_eq!(s["platform"], "null");
    assert_eq!(s["steam_app_id"], 480);
    assert_eq!(s["achievements"][0]["id"], "early");
    assert_eq!(s["achievements"][0]["unlocked"], true);
    assert_eq!(s["state"]["presence"]["status"], "Early on");
    assert_eq!(s["config"]["achievements"][0]["api"], "ACH_WIN_ONE_GAME");
    // Each request answered in the log, after it; one platform.toml does
    // not declare is refused.
    assert_eq!(events(&mut d, "platform.achieved").len(), 1);
    let errs = events(&mut d, "platform.error");
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].to_string().contains("missing"), "{errs:?}");
    // Reset forgets.
    d.call(0, "platform.reset", &json!({})).unwrap();
    let s = d.call(0, "platform.status", &json!({})).unwrap();
    assert_eq!(s["achievements"][0]["unlocked"], false);
}

#[test]
fn a_save_loads_from_the_cloud_when_the_disk_has_none() {
    let root = game("cloud");
    let mut d = open(&root);
    d.call(0, "platform.reset", &json!({ "cloud": true }))
        .unwrap();
    d.call(0, "sim.step", &json!({ "n": 4 })).unwrap();
    let saved = d.call(0, "sim.save", &json!({ "name": "x" })).unwrap();
    assert_eq!(saved["cloud"], true);
    let s = d.call(0, "platform.status", &json!({})).unwrap();
    assert!(
        s["state"]["cloud"]["files"].to_string().contains("x.save"),
        "{s}"
    );
    std::fs::remove_file(root.join(".argus/saves/x.save")).unwrap();
    d.call(0, "sim.step", &json!({ "n": 5 })).unwrap();
    let l = d.call(0, "sim.load", &json!({ "name": "x" })).unwrap();
    assert_eq!(l["from"], "cloud");
    assert_eq!(d.call(0, "sim.status", &json!({})).unwrap()["tick"], 4);
    // A save at a path of its own stays out of the cloud.
    let other = d
        .call(
            0,
            "sim.save",
            &json!({ "name": "y", "path": "fixtures/y.save" }),
        )
        .unwrap();
    assert_eq!(other["cloud"], false);
}

#[test]
fn the_overlay_is_an_input_and_a_seek_replays_across_requests() {
    let root = game("overlay");
    let mut d = open(&root);
    d.call(0, "sim.step", &json!({ "n": 3 })).unwrap();
    d.call(0, "platform.overlay", &json!({ "open": true }))
        .unwrap();
    d.call(0, "sim.step", &json!({ "n": 2 })).unwrap();
    let y = |d: &mut Daemon| {
        d.call(0, "world.get", &json!({ "entity": "@player" }))
            .unwrap()["components"]["Transform"]["pos"][1]
            .clone()
    };
    assert_eq!(y(&mut d), 1.0, "the pause action held while it is open");
    d.call(0, "platform.overlay", &json!({ "open": false }))
        .unwrap();
    d.call(0, "sim.step", &json!({ "n": 15 })).unwrap();
    assert_eq!(y(&mut d), 0.0);
    let digest = d.call(0, "sim.status", &json!({ "digest": true })).unwrap()["digest"].clone();
    let s = d.call(0, "sim.seek", &json!({ "frame": 2 })).unwrap();
    assert!(s["diverged"].is_null(), "{s}");
    d.call(0, "sim.step", &json!({ "n": 18 })).unwrap();
    assert_eq!(
        d.call(0, "sim.status", &json!({ "digest": true })).unwrap()["digest"],
        digest,
        "the same world after the overlay and the achievement, replayed"
    );
}
