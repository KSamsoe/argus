//! Phase 1 item 4 through the protocol: the importer scans on open,
//! reports every import as events with warnings, answers asset.list and
//! asset.get, feeds the renderer and the mixer, and the watcher hot-swaps
//! a placeholder for its art without touching a reference (§14).

mod common;

use common::*;
use serde_json::{json, Value as Json};

fn events(d: &mut argus_daemon::Daemon, name: &str) -> Vec<Json> {
    ok(d, "obs.events", json!({ "name": name, "limit": 1000 }))["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["fields"].clone())
        .collect()
}

#[test]
fn assets_import_on_open_report_as_events_and_hot_swap() {
    let t = temp("assets");
    project(t.path());
    write(
        t.path(),
        "assets/sprites/hero.toml",
        "kind = \"rect\"\nsize = [8, 8]\ncolor = \"#00FF00\"\npivot = [0.5, 0.0]\n",
    );
    write(
        t.path(),
        "assets/sfx/step.toml",
        "gain = 0.5\nvariations = [{ wave = \"noise\", ms = 40 }]\n",
    );
    write(t.path(), "assets/broken.toml", "\nkind = \"star\"\n");
    // The player draws the hero asset.
    write(
        t.path(),
        "scenes/arena/camera.toml",
        "[Transform]\npos = [0.0, 0.0]\n\n[Camera]\nheight = 20.0\n",
    );
    write(
        t.path(),
        "scenes/arena/blob.toml",
        "[Transform]\npos = [0.0, 0.0]\n\n[Sprite]\natlas = \"sprites/hero\"\nsize = [4.0, 4.0]\n",
    );
    let mut d = argus_daemon::Daemon::new();
    let open = d
        .call(
            0,
            "session.open",
            &json!({ "project": t.path().to_string_lossy() }),
        )
        .unwrap();
    assert_eq!(open["assets"]["total"], 2, "{}", open["assets"]);
    assert_eq!(open["assets"]["failed"], 1);
    let scanned = events(&mut d, "assets.scanned");
    assert_eq!(scanned[0]["assets"], 2);
    let failed = events(&mut d, "asset.failed");
    assert_eq!(failed[0]["asset"], "broken");
    assert_eq!(failed[0]["at"], "assets/broken.toml:2");

    let list = ok(&mut d, "asset.list", json!({}));
    assert_eq!(list["total"], 2);
    assert_eq!(list["columns"][0], "name");
    assert_eq!(
        list["rows"][0],
        json!(["sfx/step", "sound", "sfx/step.toml", 1, [], 0])
    );
    assert_eq!(
        list["rows"][1],
        json!(["sprites/hero", "sprite", "shape", 1, [], 0])
    );
    assert_eq!(list["failed"][0]["name"], "broken");
    let hero = ok(&mut d, "asset.get", json!({ "name": "sprites/hero" }));
    assert_eq!(hero["shape"]["color"], "#00FF00");
    assert_eq!(hero["frames"][0]["pivot"], json!([0.5, 0.0]));
    let e = err(&mut d, "asset.get", json!({ "name": "broken" }));
    assert_eq!(e.code, argus_assets::codes::UNKNOWN_ASSET);
    assert!(e.hint.contains("shapes are rect and circle"), "{}", e.hint);

    // The sidecar's sound reaches the mixer.
    let played = ok(&mut d, "audio.play", json!({ "sound": "sfx/step" }));
    assert_eq!(played["name"], "sound.played", "{played}");
    assert_eq!(played["fields"]["gain"], 0.5);

    // The renderer draws the placeholder asset (if this machine has an adapter).
    if let Ok(f) = d.call(0, "obs.frame", &json!({ "width": 64, "height": 64 })) {
        assert_eq!(f["missing_atlases"], json!([]), "{f}");
        assert_eq!(f["sprites"], 1);
    }

    // The art arrives: 3x3, not a power of two. The watcher picks it up.
    argus_assets::encode_png(
        &t.path().join("assets/sprites/hero.png"),
        3,
        3,
        &[255, 0, 0, 255].repeat(9),
    )
    .unwrap();
    d.refresh_assets();
    let imported = events(&mut d, "asset.imported");
    assert_eq!(imported.len(), 1, "{imported:?}");
    assert_eq!(imported[0]["asset"], "sprites/hero");
    assert_eq!(
        imported[0]["warnings"], 2,
        "the shape keys are ignored, and padding"
    );
    let warnings = events(&mut d, "asset.warning");
    assert!(
        warnings
            .iter()
            .any(|w| w["message"] == "texture 3x3 is not power-of-two, padding to 4x4"),
        "{warnings:?}"
    );
    assert_eq!(
        ok(&mut d, "asset.get", json!({ "name": "sprites/hero" }))["source"],
        "sprites/hero.png"
    );
    // The same scene file, the same reference: nothing was edited to swap the art.
    assert!(read(t.path(), "scenes/arena/blob.toml").contains("atlas = \"sprites/hero\""));

    // Nothing changed: no events.
    d.refresh_assets();
    assert_eq!(events(&mut d, "asset.imported").len(), 1);
    // Fix the broken one and force a full rescan.
    write(t.path(), "assets/broken.toml", "kind = \"circle\"\n");
    let r = ok(&mut d, "asset.import", json!({ "force": true }));
    assert_eq!(r["imported"].as_array().unwrap().len(), 3, "{r}");
    assert_eq!(r["failed"], json!([]));
    // Deleting a file removes the asset.
    std::fs::remove_file(t.path().join("assets/broken.toml")).unwrap();
    let r = ok(&mut d, "asset.import", json!({}));
    assert_eq!(r["removed"], json!(["broken"]));
    assert_eq!(events(&mut d, "asset.removed")[0]["asset"], "broken");
}
