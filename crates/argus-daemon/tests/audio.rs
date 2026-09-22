//! Phase 1 item 3 through the protocol: a script asks for sounds with
//! `q:play`, the log answers with `sound.played` (§8 "audio is observed as
//! events"), and the mixer is read and tuned as data.

mod common;

use common::*;
use serde_json::{json, Value as Json};

const AUDIO: &str = r#"
[buses]
master = { gain_db = 0 }
sfx    = { parent = "master" }
music  = { parent = "master", gain_db = -6 }

[sounds."sfx/hit"]
pitch_jitter = 0.1
variations = [ { wave = "square", freq = 180, ms = 400 }, { wave = "noise", ms = 400 } ]
"#;

const SFX: &str = r#"system "late.sfx" {
  stage = "late",
  reads = { "Health" },
  doc = "Plays the hit sound on the bat once, and asks for one that does not exist.",
  run = function(q)
    if q.tick == 2 then
      q:play("sfx/hit", { entity = q:entity("@bat"), gain = 0.9 })
      q:play("sfx/nope")
    end
  end,
}
"#;

fn played(d: &mut argus_daemon::Daemon) -> Vec<Json> {
    ok(d, "obs.events", json!({ "name": "sound.played" }))["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["fields"].clone())
        .collect()
}

#[test]
fn scripts_ask_for_sounds_and_the_log_says_what_played() {
    let t = temp("audio");
    project(t.path());
    write(t.path(), "audio.toml", AUDIO);
    write(t.path(), "systems/sfx.luau", SFX);
    let mut d = open(t.path());
    ok(&mut d, "sim.step", json!({ "n": 4 }));

    let p = played(&mut d);
    assert_eq!(p.len(), 1, "{p:?}");
    let f = &p[0];
    assert_eq!(f["sound"], "sfx/hit");
    assert_eq!(f["entity"], "@bat");
    assert_eq!(f["bus"], "sfx");
    assert_eq!(f["gain"], 0.9);
    assert!(f["variation"] == 0 || f["variation"] == 1);
    let pitch = f["pitch"].as_f64().unwrap();
    assert!((0.9..=1.1).contains(&pitch), "{pitch}");
    // The bat is 3 units from the listener (the origin, no camera here).
    let att = f["attenuation"].as_f64().unwrap();
    assert!(att > 0.9 && att < 1.0, "{att}");
    assert!(f["pan"].as_f64().unwrap() > 0.0, "right of the listener");
    // The request is in the log too, right before its answer.
    let req = ok(&mut d, "obs.events", json!({ "name": "sound.play" }));
    assert_eq!(req["total"], 2);
    let missing = ok(&mut d, "obs.events", json!({ "name": "sound.missing" }));
    assert_eq!(missing["rows"][0]["fields"]["sound"], "sfx/nope");

    // Status is data: buses, meters, the sound that was asked for and is not there.
    let st = ok(&mut d, "audio.status", json!({}));
    assert_eq!(st["device"], Json::Null, "headless: no device");
    assert_eq!(st["rate"], 48000);
    assert_eq!(st["columns"][0], "bus");
    let sfx = st["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r[0] == "sfx")
        .unwrap()
        .clone();
    assert_eq!(sfx[1], "master");
    assert!(sfx[5].as_f64().unwrap() > -40.0, "sfx peaked: {sfx}");
    assert_eq!(st["missing"], json!(["sfx/nope"]));
    assert!(st["sounds"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s == "sfx/hit"));

    // The agent can play a sound as gameplay would, and tune a bus.
    let r = ok(
        &mut d,
        "audio.play",
        json!({ "sound": "sfx/hit", "pan": -1.0 }),
    );
    assert_eq!(r["name"], "sound.played");
    assert_eq!(r["fields"]["pan"], -1.0);
    let voice = r["fields"]["voice"].as_u64().unwrap();
    let st = ok(&mut d, "audio.bus", json!({ "bus": "sfx", "gain_db": -12 }));
    let sfx = st["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r[0] == "sfx")
        .unwrap()
        .clone();
    assert_eq!(sfx[2], -12.0);
    let stop = ok(&mut d, "audio.stop", json!({ "voice": voice }));
    assert_eq!(stop["stopped"], json!([voice]));
    let e = err(
        &mut d,
        "audio.bus",
        json!({ "bus": "dialogue", "gain_db": 0 }),
    );
    assert_eq!(e.code, argus_audio::codes::UNKNOWN_BUS);
    assert!(e.hint.contains("master, sfx, music"), "{}", e.hint);

    // A clip for a human.
    let c = ok(
        &mut d,
        "audio.clip",
        json!({ "seconds": 1, "path": "clips/hit.wav" }),
    );
    assert!(t.path().join("clips").join("hit.wav").exists(), "{c}");

    // Same scene, same seed: the same variations and jitter (section 6).
    ok(&mut d, "scene.load", json!({ "scene": "arena", "seed": 7 }));
    ok(&mut d, "sim.step", json!({ "n": 4 }));
    let a = played(&mut d);
    ok(&mut d, "scene.load", json!({ "scene": "arena", "seed": 7 }));
    ok(&mut d, "sim.step", json!({ "n": 4 }));
    let b = played(&mut d);
    assert_eq!(a.len(), 1);
    assert_eq!(a, b);
}

#[test]
fn a_bad_audio_toml_is_reported_not_fatal() {
    let t = temp("audio-bad");
    project(t.path());
    write(
        t.path(),
        "audio.toml",
        "[buses]\nsfx = { parent = \"nowhere\" }\n",
    );
    let mut d = open(t.path());
    let st = ok(&mut d, "audio.status", json!({}));
    assert_eq!(st["error"]["code"], argus_audio::codes::BAD_AUDIO_TOML);
    assert_eq!(st["error"]["data"]["ref"], "audio.toml:1");
    let e = err(&mut d, "audio.play", json!({ "sound": "x" }));
    assert!(e.message.contains("not a bus"), "{e}");
    // The world still runs.
    ok(&mut d, "sim.step", json!({ "n": 2 }));
}
