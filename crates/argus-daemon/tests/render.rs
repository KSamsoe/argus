//! Phase 1 item 1 through the protocol: `obs.frame` renders the world
//! offscreen to a PNG (section 8 L5), `render.status` reports, and the
//! renderer's components are authored like any other.

mod common;

use argus_daemon::Daemon;
use common::*;
use serde_json::json;

fn open_with_sprites(name: &str) -> (Temp, Daemon) {
    let t = temp(name);
    project(t.path());
    write(
        t.path(),
        "scenes/arena/camera.toml",
        "[Transform]\npos = [0.0, 0.0]\n\n[Camera]\nheight = 20.0\n",
    );
    write(
        t.path(),
        "scenes/arena/blob.toml",
        "[Transform]\npos = [0.0, 0.0]\n\n[Sprite]\nkind = \"rect\"\nsize = [4.0, 4.0]\ncolor = \"#FF0000\"\n",
    );
    let d = open(t.path());
    (t, d)
}

#[test]
fn obs_frame_writes_a_png_of_the_authored_sprites() {
    let (t, mut d) = open_with_sprites("frame");
    let r = match d.call(0, "obs.frame", &json!({ "width": 128, "height": 64 })) {
        Ok(r) => r,
        Err(e) if e.code == argus_render::codes::NO_ADAPTER => {
            eprintln!("skipping: {e}");
            return;
        }
        Err(e) => panic!("obs.frame: {e}"),
    };
    assert_eq!(r["width"], 128);
    assert_eq!(r["sprites"], 1, "{r}");
    assert_eq!(r["draw_calls"], 1);
    assert_eq!(r["missing_atlases"], json!([]));
    let path = std::path::PathBuf::from(r["path"].as_str().unwrap());
    assert!(
        path.ends_with(
            std::path::Path::new(".argus")
                .join("frames")
                .join("frame-0.png")
        ),
        "{path:?}"
    );
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[1..4], b"PNG");

    // A named path, relative to the project.
    let r = ok(
        &mut d,
        "obs.frame",
        json!({ "path": "shots/one.png", "width": 32, "height": 32 }),
    );
    assert!(t.path().join("shots").join("one.png").exists(), "{r}");

    let st = ok(&mut d, "render.status", json!({}));
    assert_eq!(st["window"], false);
    assert_eq!(st["size"], json!([32, 32]));
    assert_eq!(st["last"]["sprites"], 1);
    assert!(st["atlases"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a == "rect"));

    // The components are described like any other.
    let desc = ok(&mut d, "world.describe", json!({ "component": "Sprite" }));
    let fields: Vec<&str> = desc["rows"][0]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert!(fields.contains(&"atlas") && fields.contains(&"kind") && fields.contains(&"color"));
    let bad = err(&mut d, "obs.frame", json!({ "format": "svg" }));
    assert_eq!(bad.code, argus_daemon::codes::BAD_PARAMS);
}

/// The daemon's copy of the no-adapter code is the renderer's.
#[test]
fn the_no_adapter_code_matches_the_renderer() {
    assert_eq!(
        argus_daemon::codes::NO_ADAPTER,
        argus_render::codes::NO_ADAPTER
    );
}

/// Phase 5 item 2: the frame as a colour-blind eye sees it, the game left
/// as it is. The red blob as drawn, then as no colour at all.
#[test]
fn obs_frame_sees_as_a_colour_blind_eye_sees() {
    let (_t, mut d) = open_with_sprites("frame-see-as");
    let centre = |d: &mut Daemon, see_as: &str| -> Option<[u8; 3]> {
        let r = match d.call(
            0,
            "obs.frame",
            &json!({ "width": 64, "height": 64, "see_as": see_as, "software": true }),
        ) {
            Ok(r) => r,
            Err(e) if e.code == argus_render::codes::NO_ADAPTER => return None,
            Err(e) => panic!("obs.frame: {e}"),
        };
        let (w, _, rgba) =
            argus_assets::decode_png(std::path::Path::new(r["path"].as_str().unwrap())).unwrap();
        let i = ((32 * w + 32) * 4) as usize;
        Some([rgba[i], rgba[i + 1], rgba[i + 2]])
    };
    let Some(red) = centre(&mut d, "normal") else {
        return;
    };
    assert!(red[0] > 200 && red[1] < 40, "{red:?}");
    let grey = centre(&mut d, "achromatopsia").unwrap();
    assert!(
        grey[0].abs_diff(grey[1]) <= 1 && grey[1].abs_diff(grey[2]) <= 1,
        "{grey:?}"
    );
    // And as it is again: the renderer is kept between calls.
    assert_eq!(centre(&mut d, "normal").unwrap(), red);
    let e = err(&mut d, "obs.frame", json!({ "see_as": "sepia" }));
    assert!(e.message.contains("deuteranopia"), "{}", e.message);
}
