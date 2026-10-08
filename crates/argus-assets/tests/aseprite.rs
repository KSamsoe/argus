//! Phase 5 item 4, §14: "Aseprite and Tiled files import natively with
//! their internal structure exposed through the sidecar"; §13: "Aseprite
//! tags import directly as clips". Files are written by `ase_writer` (the
//! spec's format; there is no Aseprite here) and imported as any sprite.

mod ase_writer;

use argus_assets::{aseprite, decode_png, encode_png, sidecar::Direction, Asset, Importer};
use ase_writer::{fill, stored_zlib, Ase, Key, Prop, BACKGROUND, REFERENCE, VISIBLE};
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// A temp directory removed on drop, used as its path.
struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for Temp {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn project(name: &str) -> Temp {
    let dir = std::env::temp_dir().join(format!("argus-ase-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    Temp(dir)
}

fn put(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join("assets").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

fn sprite(e: &argus_assets::Entry) -> &argus_assets::SpriteAsset {
    match &e.asset {
        Asset::Sprite(s) => s,
        other => panic!("not a sprite: {other:?}"),
    }
}

fn messages(e: &argus_assets::Entry) -> Vec<String> {
    let mut m: Vec<String> = e.warnings.iter().map(|w| w.message.clone()).collect();
    m.sort();
    m
}

/// Frame `i`'s pixels, read back from the cached sheet.
fn pixels(s: &argus_assets::SpriteAsset, i: usize) -> Vec<[u8; 4]> {
    let (tw, th, px) = decode_png(&s.texture).unwrap();
    assert_eq!([tw, th], s.texture_size);
    let [x, y, w, h] = s.frames[i].rect;
    let mut out = Vec::new();
    for yy in y..y + h {
        for xx in x..x + w {
            let at = ((yy * tw + xx) * 4) as usize;
            out.push([px[at], px[at + 1], px[at + 2], px[at + 3]]);
        }
    }
    out
}

const CLEAR: [u8; 4] = [0, 0, 0, 0];
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];

#[test]
fn rgba_grayscale_and_indexed_files_draw_as_aseprite_draws_them() {
    let dir = project("depths");
    let mut a = Ase::new(2, 2, 32);
    a.frame(100).image_layer("art").cel(
        0,
        0,
        0,
        2,
        2,
        &[
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 10, 20, 30, 128,
        ],
    );
    put(&dir, "rgba.aseprite", &a.bytes());
    // Grayscale: a value and an alpha a pixel.
    let mut g = Ase::new(2, 1, 16);
    g.frame(100)
        .image_layer("art")
        .cel(0, 0, 0, 2, 1, &[200, 255, 50, 100]);
    put(&dir, "gray.aseprite", &g.bytes());
    // Indexed: index 0 is clear, except on the background layer; the
    // palette's alpha counts.
    let mut i = Ase::new(3, 2, 8);
    i.frame(100)
        .palette(&[[9, 9, 9, 255], RED, [0, 0, 255, 128]])
        .layer("bg", VISIBLE | BACKGROUND, 0, 0, 0, 255)
        .image_layer("top")
        .cel(0, 0, 1, 3, 1, &[0, 1, 2])
        .cel(1, 0, 0, 3, 1, &[0, 1, 2]);
    put(&dir, "indexed.aseprite", &i.bytes());
    // The old palette chunks: 0 to 255, and 0 to 63 widened as Aseprite does.
    let mut o = Ase::new(2, 1, 8);
    o.frame(100)
        .old_palette(0x0004, &[[0, 0, 0], [10, 20, 30]])
        .image_layer("art")
        .cel(0, 0, 0, 2, 1, &[1, 1]);
    put(&dir, "old4.aseprite", &o.bytes());
    let mut o = Ase::new(2, 1, 8);
    o.frame(100)
        .old_palette(0x0011, &[[0, 0, 0], [63, 32, 1]])
        .image_layer("art")
        .cel(0, 0, 0, 2, 1, &[1, 0]);
    put(&dir, "old11.aseprite", &o.bytes());

    let mut imp = Importer::new(&dir);
    let r = imp.scan();
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    assert_eq!(r.imported.len(), 5);
    for n in ["rgba", "gray", "indexed", "old4", "old11"] {
        let e = imp.get(n).unwrap();
        assert!(e.warnings.is_empty(), "{n}: {:?}", messages(e));
    }
    let px = |n: &str| pixels(sprite(imp.get(n).unwrap()), 0);
    assert_eq!(px("rgba"), vec![RED, GREEN, BLUE, [10, 20, 30, 128]]);
    assert_eq!(px("gray"), vec![[200, 200, 200, 255], [50, 50, 50, 100]]);
    assert_eq!(
        px("indexed"),
        vec![
            CLEAR,
            RED,
            [0, 0, 255, 128],
            [9, 9, 9, 255],
            RED,
            [0, 0, 255, 128]
        ]
    );
    assert_eq!(px("old4"), vec![[10, 20, 30, 255], [10, 20, 30, 255]]);
    assert_eq!(px("old11"), vec![[255, 130, 4, 255], CLEAR]);
    let s = sprite(imp.get("indexed").unwrap());
    assert_eq!(s.source.as_deref(), Some("indexed.aseprite"));
    assert_eq!(s.aseprite.as_ref().unwrap().depth, "indexed");
    assert_eq!(s.texture_size, [4, 2]);
}

#[test]
fn raw_compressed_and_linked_cels_land_at_their_offsets() {
    let dir = project("cels");
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&fill(3, 3, BLUE)).unwrap();
    let deflated = z.finish().unwrap();
    let mut a = Ase::new(8, 8, 32);
    a.frame(100)
        .image_layer("body")
        // 2 x 2 at (-1, -1): only its last pixel is on the canvas.
        .cel(0, -1, -1, 2, 2, &fill(2, 2, RED))
        .frame(100)
        // Compressed, at (6, 5): its right column is off the canvas.
        .cel_zlib(0, 6, 5, 3, 3, &deflated)
        .frame(100)
        .cel_link(0, 1)
        .frame(100)
        // Stored blocks, and frame 0's pixels again.
        .cel_zlib(0, -1, -1, 2, 2, &stored_zlib(&fill(2, 2, RED)));
    put(&dir, "cels.aseprite", &a.bytes());
    let mut imp = Importer::new(&dir);
    let r = imp.scan();
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    let e = imp.get("cels").unwrap();
    assert!(e.warnings.is_empty(), "{:?}", messages(e));
    let s = sprite(e);
    assert_eq!(s.frames.len(), 4);
    let f0 = pixels(s, 0);
    assert_eq!(f0[0], RED);
    assert_eq!(f0.iter().filter(|p| p[3] != 0).count(), 1);
    let f1 = pixels(s, 1);
    for y in 0..8 {
        for x in 0..8 {
            let want = if x >= 6 && y >= 5 { BLUE } else { CLEAR };
            assert_eq!(f1[y * 8 + x], want, "({x}, {y})");
        }
    }
    assert_eq!(
        s.frames[2].rect, s.frames[1].rect,
        "a linked cel's frame is the frame it links to"
    );
    assert_eq!(
        s.frames[3].rect, s.frames[0].rect,
        "frames of the same pixels share their place on the sheet"
    );
    assert_ne!(s.frames[0].rect, s.frames[1].rect);
    assert_eq!(s.texture_size, [8, 16], "two cells, no padding");
    assert_eq!([s.width, s.height], [8, 16]);
}

/// Layers bottom to top: base, a hidden one, a group at half opacity of
/// two, a hidden group, a cel at half opacity, a multiply layer, a tilemap,
/// and a reference layer; and a colour profile and an unknown chunk.
fn stack() -> Vec<u8> {
    let mut a = Ase::new(4, 1, 32);
    a.frame(100)
        .image_layer("base") // 0
        .layer("hidden", 0, 0, 0, 0, 255) // 1
        .layer("group", VISIBLE, 1, 0, 0, 128) // 2
        .layer("a", VISIBLE, 0, 1, 0, 255) // 3
        .layer("b", VISIBLE, 0, 1, 0, 255) // 4
        .layer("hiddengroup", 0, 1, 0, 0, 255) // 5
        .layer("c", VISIBLE, 0, 1, 0, 255) // 6
        .image_layer("half") // 7
        .layer("shade", VISIBLE, 0, 0, 1, 255) // 8
        .layer("tiles", VISIBLE, 2, 0, 0, 255) // 9
        .layer("ref", VISIBLE | REFERENCE, 0, 0, 0, 255) // 10
        .chunk(0x2007, vec![1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        .chunk(0x2FFF, vec![1, 2, 3])
        .cel(0, 0, 0, 4, 1, &fill(4, 1, RED))
        .cel(1, 0, 0, 4, 1, &fill(4, 1, GREEN))
        .cel(3, 0, 0, 2, 1, &fill(2, 1, BLUE))
        .cel(4, 1, 0, 2, 1, &fill(2, 1, WHITE))
        .cel(6, 0, 0, 4, 1, &fill(4, 1, BLACK))
        .cel_full(7, 3, 0, 128, 0, 1, 1, &GREEN)
        .cel(10, 0, 0, 4, 1, &fill(4, 1, BLACK));
    a.bytes()
}

#[test]
fn layers_and_groups_draw_with_their_visibility_and_opacity() {
    let dir = project("layers");
    put(&dir, "stack.aseprite", &stack());
    let mut imp = Importer::new(&dir);
    imp.scan();
    let e = imp.get("stack").unwrap();
    // The group's layers are drawn together, then over the base at half:
    // blue under white is white, not a mix.
    assert_eq!(
        pixels(sprite(e), 0),
        vec![
            [127, 0, 128, 255],
            [255, 128, 128, 255],
            [255, 128, 128, 255],
            [127, 128, 0, 255]
        ]
    );
    assert_eq!(
        messages(e),
        vec![
            "layer \"shade\" blends as multiply, drawn as normal",
            "layer \"tiles\" is a tilemap layer; tilemaps are not imported, so it is skipped",
            "the file has chunks of type 0x2FFF, which the specification does not list; skipped",
        ],
        "a colour profile is no warning"
    );
    assert!(e.warnings.iter().all(|w| w.at == "assets/stack.aseprite"));
    // The structure, for an agent writing a sidecar.
    let j = e.to_json();
    let layers = j["aseprite"]["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 11);
    let layer = |p: &str| layers.iter().find(|l| l["path"] == p).unwrap().clone();
    assert_eq!(layer("group")["used"], true);
    assert_eq!(layer("group")["opacity"], 128);
    assert_eq!(layer("group")["kind"], "group");
    assert_eq!(layer("group/a")["used"], true);
    assert_eq!(layer("hidden")["visible"], false);
    assert_eq!(layer("hidden")["used"], false);
    assert_eq!(layer("hiddengroup/c")["used"], false);
    assert_eq!(layer("shade")["blend"], "multiply");
    assert_eq!(layer("tiles")["kind"], "tilemap");
    assert_eq!(layer("ref")["kind"], "reference");
    assert_eq!(layer("ref")["used"], false);
    assert_eq!(j["aseprite"]["canvas"], serde_json::json!([4, 1]));
    assert_eq!(j["source"], "stack.aseprite");

    // The sidecar picks layers: a hidden one is drawn when named, and a
    // layer in a group by its path, at the group's opacity.
    std::fs::write(
        dir.join("assets/stack.toml"),
        "layers = [\"hidden\", \"group/b\"]\n",
    )
    .unwrap();
    imp.touch("stack");
    let r = imp.scan();
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    let e = imp.get("stack").unwrap();
    assert_eq!(
        pixels(sprite(e), 0),
        vec![GREEN, [128, 255, 128, 255], [128, 255, 128, 255], GREEN]
    );
    let j = e.to_json();
    let layers = j["aseprite"]["layers"].as_array().unwrap();
    let used: Vec<&str> = layers
        .iter()
        .filter(|l| l["used"] == true)
        .map(|l| l["path"].as_str().unwrap())
        .collect();
    assert_eq!(used, vec!["hidden", "group", "group/b"]);
    assert_eq!(
        messages(e),
        vec!["the file has chunks of type 0x2FFF, which the specification does not list; skipped"],
        "the multiply layer and the tilemap are not drawn, so not warned of"
    );

    // A name the file lacks is a warning while another draws.
    std::fs::write(
        dir.join("assets/stack.toml"),
        "# a comment line\nlayers = [\"nothing\", \"a\"]\n",
    )
    .unwrap();
    imp.touch("stack");
    imp.scan();
    let e = imp.get("stack").unwrap();
    assert_eq!(
        pixels(sprite(e), 0),
        vec![[0, 0, 255, 128], [0, 0, 255, 128], CLEAR, CLEAR]
    );
    let w = e
        .warnings
        .iter()
        .find(|w| w.message.starts_with("layers names"))
        .unwrap();
    assert!(
        w.message.contains("\"nothing\", which assets/stack.aseprite does not have; it has base, hidden, group, group/a, group/b"),
        "{}",
        w.message
    );
    assert_eq!(w.at, "assets/stack.toml:2");

    // Naming nothing it has is refused.
    std::fs::write(dir.join("assets/stack.toml"), "layers = [\"nothing\"]\n").unwrap();
    imp.touch("stack");
    let r = imp.scan();
    assert_eq!(r.failed.len(), 1);
    let f = &r.failed[0].error;
    assert_eq!(f.code, argus_assets::codes::BAD_SIDECAR);
    assert!(
        f.message.starts_with("layers names \"nothing\""),
        "{}",
        f.message
    );
    assert_eq!(f.r#ref.as_deref(), Some("assets/stack.toml:1"));
}

#[test]
fn tags_become_clips_in_every_direction_with_the_frames_durations() {
    let dir = project("tags");
    let ms = [100u16, 50, 50, 200, 80, 80];
    let tags = [
        (0, 0, 0, 0, "idle"),
        (1, 3, 0, 0, "run"),
        (1, 3, 1, 0, "back"),
        (1, 3, 2, 0, "bounce"),
        (1, 3, 3, 0, "rebound"),
        (1, 3, 2, 1, "once"),
        (1, 3, 2, 2, "twice"),
        (4, 5, 1, 3, "thrice"),
        (4, 9, 0, 0, "broken"),
        (2, 3, 0, 0, "run"),
    ];
    let mut a = Ase::new(2, 2, 32);
    for (f, ms) in ms.iter().enumerate() {
        a.frame(*ms);
        if f == 0 {
            a.image_layer("art");
        }
        a.cel(0, 0, 0, 2, 2, &fill(2, 2, [f as u8 * 40, 100, 0, 255]));
        if f == 0 {
            // A user data chunk a tag follows the tags: none is the cel's.
            a.tags(&tags);
            for _ in &tags {
                a.user_data(Some("a tag's note"), &[]);
            }
        }
    }
    put(&dir, "fx/slash.aseprite", &a.bytes());
    let mut imp = Importer::new(&dir);
    imp.scan();
    let e = imp.get("fx/slash").unwrap();
    let s = sprite(e);
    let durations: Vec<u32> = s.frames.iter().map(|f| f.duration_ms).collect();
    assert_eq!(durations, vec![100, 50, 50, 200, 80, 80]);
    assert!(
        s.frames[0].properties.is_empty(),
        "{:?}",
        s.frames[0].properties
    );
    let clip = |n: &str| {
        let c = &s.clips[n];
        assert_eq!(c.fps, None, "{n} plays by the frames' durations");
        (c.frames.clone(), c.direction, c.looping)
    };
    assert_eq!(clip("idle"), (vec![0], Direction::Forward, true));
    assert_eq!(clip("run"), (vec![1, 2, 3], Direction::Forward, true));
    assert_eq!(clip("back"), (vec![1, 2, 3], Direction::Reverse, true));
    assert_eq!(clip("bounce"), (vec![1, 2, 3], Direction::PingPong, true));
    assert_eq!(
        clip("rebound"),
        (vec![3, 2, 1], Direction::PingPong, true),
        "a ping-pong reverse starts at its end"
    );
    assert_eq!(
        clip("once"),
        (vec![1, 2, 3], Direction::Forward, false),
        "a ping-pong played once goes one way"
    );
    assert_eq!(
        clip("twice"),
        (vec![1, 2, 3, 2, 1], Direction::Forward, false)
    );
    assert_eq!(
        clip("thrice"),
        (vec![5, 4, 5, 4, 5, 4], Direction::Forward, false)
    );
    assert!(!s.clips.contains_key("broken"));
    assert_eq!(
        messages(e),
        vec![
            "tag \"broken\" runs from frame 4 to 9, and the file has 6 frames; dropped",
            "two tags are named \"run\"; the second (frames 2 to 3) is dropped",
        ]
    );
    let j = e.to_json();
    let t = &j["aseprite"]["tags"];
    assert_eq!(t.as_array().unwrap().len(), 10);
    assert_eq!(
        t[4],
        serde_json::json!({ "name": "rebound", "from": 1, "to": 3, "direction": "pingpong_reverse", "repeat": 0 })
    );
    assert_eq!(t[6]["repeat"], 2);
    assert_eq!(j["clips"]["rebound"]["direction"], "pingpong");
}

#[test]
fn slices_set_pivots_and_hitboxes_and_cel_user_data_is_frame_properties() {
    let dir = project("slices");
    let key = |frame, bounds, center, pivot| Key {
        frame,
        bounds,
        center,
        pivot,
    };
    let mut a = Ase::new(16, 16, 32);
    a.frame(100)
        .image_layer("art")
        .cel(0, 0, 0, 16, 16, &fill(16, 16, RED))
        .slice(
            "pivot",
            &[
                key(0, [0, 0, 16, 16], None, Some([8, 16])),
                key(2, [4, 4, 8, 8], None, Some([4, 8])),
            ],
        )
        .slice(
            "hitbox",
            &[
                key(0, [2, 3, 10, 12], None, None),
                key(1, [0, 0, 0, 0], None, None),
                key(2, [1, 1, 4, 4], None, None),
            ],
        )
        .user_data(Some("a slice's note"), &[])
        .slice("panel", &[key(0, [0, 0, 16, 16], Some([4, 4, 8, 8]), None)])
        .frame(120)
        .cel(0, 0, 0, 16, 16, &fill(16, 16, GREEN))
        .user_data(
            Some("step"),
            &[
                ("damage", Prop::I32(3)),
                ("heavy", Prop::Bool(true)),
                ("tag", Prop::Str("x")),
            ],
        )
        .frame(100)
        .cel(0, 0, 0, 16, 16, &fill(16, 16, BLUE));
    put(&dir, "knight.aseprite", &a.bytes());
    // Two slices with pivots and none named pivot: the first's.
    let mut b = Ase::new(16, 16, 32);
    b.frame(100)
        .image_layer("art")
        .cel(0, 0, 0, 16, 16, &fill(16, 16, RED))
        .slice("feet", &[key(0, [0, 0, 16, 16], None, Some([8, 16]))])
        .slice("head", &[key(0, [4, 0, 8, 4], None, Some([4, 0]))]);
    put(&dir, "gem.aseprite", &b.bytes());
    let mut imp = Importer::new(&dir);
    let r = imp.scan();
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    let e = imp.get("knight").unwrap();
    assert!(e.warnings.is_empty(), "{:?}", messages(e));
    let s = sprite(e);
    let pivots: Vec<[f32; 2]> = s.frames.iter().map(|f| f.pivot).collect();
    assert_eq!(pivots, vec![[0.5, 0.0], [0.5, 0.0], [0.5, 0.25]]);
    let props = |i: usize| -> Vec<(String, String)> {
        s.frames[i]
            .properties
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    };
    let pair = |k: &str, v: &str| (k.to_string(), v.to_string());
    assert_eq!(
        props(0),
        vec![
            pair("slice.hitbox", "2 3 10 12"),
            pair("slice.panel", "0 0 16 16"),
            pair("slice.panel.center", "4 4 8 8"),
        ]
    );
    assert_eq!(
        props(1),
        vec![
            pair("damage", "3"),
            pair("data", "step"),
            pair("heavy", "true"),
            pair("slice.panel", "0 0 16 16"),
            pair("slice.panel.center", "4 4 8 8"),
            pair("tag", "x"),
        ],
        "no hitbox where its key is empty; the cel's user data"
    );
    assert_eq!(props(2)[0], pair("slice.hitbox", "1 1 4 4"));
    let j = e.to_json();
    let sl = &j["aseprite"]["slices"];
    assert_eq!(sl.as_array().unwrap().len(), 3);
    assert_eq!(
        sl[0],
        serde_json::json!({ "name": "pivot", "keys": [
            { "frame": 0, "bounds": [0, 0, 16, 16], "pivot": [8, 16] },
            { "frame": 2, "bounds": [4, 4, 8, 8], "pivot": [4, 8] },
        ] })
    );
    assert_eq!(sl[2]["keys"][0]["center"], serde_json::json!([4, 4, 8, 8]));
    assert_eq!(j["frames"][1]["properties"]["data"], "step");

    let e = imp.get("gem").unwrap();
    assert_eq!(sprite(e).frames[0].pivot, [0.5, 0.0]);
    assert_eq!(
        messages(e),
        vec!["slices \"feet\" and \"head\" have pivots; \"feet\"'s is used (name the one you mean pivot)"]
    );
    assert_eq!(sprite(e).frames[0].properties["slice.head"], "4 0 8 4");
}

#[test]
fn the_sidecar_changes_only_what_it_writes_and_an_empty_frame_is_reported() {
    let dir = project("sidecar");
    let mut a = Ase::new(4, 4, 32);
    a.frame(100)
        .image_layer("art")
        .tags(&[(0, 3, 0, 0, "run"), (0, 2, 3, 0, "spin")]);
    a.cel(0, 0, 0, 4, 4, &fill(4, 4, RED));
    a.frame(100).cel(0, 0, 0, 4, 4, &fill(4, 4, GREEN));
    a.frame(100); // empty
    a.frame(100).cel(0, 0, 0, 4, 4, &fill(4, 4, BLUE));
    put(&dir, "hero.aseprite", &a.bytes());
    let side = r#"pixels_per_unit = 32
pivot = [0.5, 0.0]
grid = [2, 2]
kind = "rect"

[[frames]]
index = 1
duration_ms = 300
properties = { sound = "step" }

[[frames]]
rect = [0, 0, 4, 4]

[clips.run]
fps = 12
loop = false

[clips.spin]
direction = "forward"

[clips.walk]
frames = [0, 1]
"#;
    std::fs::write(dir.join("assets/hero.toml"), side).unwrap();
    let mut imp = Importer::new(&dir);
    let r = imp.scan();
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    let e = imp.get("hero").unwrap();
    assert_eq!(
        messages(e),
        vec![
            "a frame's rect is ignored: the frames are assets/hero.aseprite's",
            "frame 2 is empty",
            "grid is ignored: the frames are assets/hero.aseprite's",
            "kind, size, and color are ignored: assets/hero.aseprite is the art now",
        ]
    );
    let at = |m: &str| {
        e.warnings
            .iter()
            .find(|w| w.message.starts_with(m))
            .unwrap()
            .at
            .clone()
    };
    assert_eq!(at("frame 2"), "assets/hero.aseprite");
    assert_eq!(at("a frame's rect"), "assets/hero.toml:11");
    let s = sprite(e);
    assert_eq!(s.pixels_per_unit, 32.0);
    assert!(s.frames.iter().all(|f| f.pivot == [0.5, 0.0]));
    assert_eq!(s.frames[1].duration_ms, 300);
    assert_eq!(s.frames[0].duration_ms, 100);
    assert_eq!(s.frames[1].properties["sound"], "step");
    let run = &s.clips["run"];
    assert_eq!(
        (run.frames.clone(), run.fps, run.looping, run.direction),
        (vec![0, 1, 2, 3], Some(12.0), false, Direction::Forward),
        "fps and loop change; the tag's frames stay"
    );
    let spin = &s.clips["spin"];
    assert_eq!(
        (spin.frames.clone(), spin.direction, spin.looping),
        (vec![0, 1, 2], Direction::Forward, true),
        "a direction plays the tag's frames from..to that way"
    );
    assert_eq!(s.clips["walk"].frames, vec![0, 1], "a clip of its own");

    // A clip with no frames and no tag of its name is refused.
    std::fs::write(
        dir.join("assets/hero.toml"),
        format!("{side}\n[clips.ghost]\nfps = 5\n"),
    )
    .unwrap();
    imp.touch("hero");
    let r = imp.scan();
    assert_eq!(r.failed.len(), 1, "{:?}", r.failed);
    let f = &r.failed[0].error;
    assert_eq!(f.code, argus_assets::codes::BAD_SIDECAR);
    assert_eq!(
        f.message,
        "clips.ghost needs frames = [...] or from and to: assets/hero.aseprite has no tag \"ghost\" for it to change (its tags: run, spin)"
    );
    assert_eq!(f.r#ref.as_deref(), Some("assets/hero.toml:24"));

    // Beside a PNG, `layers` means nothing and a clip needs its frames, as
    // before.
    encode_png(&dir.join("assets/plain.png"), 4, 4, &fill(4, 4, RED)).unwrap();
    std::fs::write(dir.join("assets/plain.toml"), "layers = [\"art\"]\n").unwrap();
    let r = imp.scan();
    assert!(r.failed.iter().all(|f| f.name != "plain"), "{:?}", r.failed);
    assert_eq!(
        messages(imp.get("plain").unwrap()),
        vec!["unknown key \"layers\", ignored"]
    );
}

#[test]
fn a_changed_file_imports_again_and_a_removed_one_goes() {
    let dir = project("reimport");
    let mut a = Ase::new(4, 4, 32);
    a.frame(100)
        .image_layer("art")
        .cel(0, 0, 0, 4, 4, &fill(4, 4, RED));
    put(&dir, "anim.aseprite", &a.bytes());
    let mut imp = Importer::new(&dir);
    assert_eq!(imp.scan().imported, vec!["anim"]);
    assert!(imp.poll().is_none(), "nothing changed");
    let s = sprite(imp.get("anim").unwrap());
    assert_eq!(s.frames.len(), 1);
    let tex = s.texture.clone();

    // The artist adds a frame and a tag in Aseprite and saves.
    a.tags(&[(0, 1, 0, 0, "blink")])
        .frame(250)
        .cel(0, 0, 0, 4, 4, &fill(4, 4, BLUE));
    put(&dir, "anim.aseprite", &a.bytes());
    let r = imp.poll().expect("the tree changed");
    assert_eq!(r.imported, vec!["anim"]);
    let s = sprite(imp.get("anim").unwrap());
    assert_eq!(s.frames.len(), 2);
    assert_eq!(s.frames[1].duration_ms, 250);
    assert_eq!(pixels(s, 1)[0], BLUE);
    assert_eq!(s.clips["blink"].frames, vec![0, 1]);

    std::fs::remove_file(dir.join("assets/anim.aseprite")).unwrap();
    let r = imp.poll().unwrap();
    assert_eq!(r.removed, vec!["anim"]);
    assert!(!tex.exists());
}

#[test]
fn a_png_and_an_aseprite_of_one_name_are_ambiguous_and_ase_is_one_too() {
    let dir = project("ambiguous");
    let mut a = Ase::new(2, 2, 32);
    a.frame(100)
        .image_layer("art")
        .cel(0, 0, 0, 2, 2, &fill(2, 2, RED));
    put(&dir, "hero.aseprite", &a.bytes());
    encode_png(&dir.join("assets/hero.png"), 2, 2, &fill(2, 2, RED)).unwrap();
    put(&dir, "fx/spark.ase", &a.bytes());
    put(&dir, "gem.aseprite", &a.bytes());
    encode_png(&dir.join("assets/gem.normal.png"), 2, 2, &fill(2, 2, RED)).unwrap();
    let mut imp = Importer::new(&dir);
    imp.scan();
    let f = &imp.failures()["hero"].error;
    assert_eq!(f.code, argus_assets::codes::AMBIGUOUS);
    assert_eq!(
        f.message,
        "hero has both a .png and a .aseprite; one name is one asset"
    );
    let s = sprite(imp.get("fx/spark").unwrap());
    assert_eq!(s.source.as_deref(), Some("fx/spark.ase"));
    let f = &imp.failures()["gem"].error;
    assert_eq!(f.code, argus_assets::codes::AMBIGUOUS);
    assert!(f.message.contains("takes none yet"), "{}", f.message);
}

/// A file with a bit of everything: groups, compressed and linked cels, a
/// palette, tags and their user data, slices, user properties.
fn everything() -> Vec<u8> {
    let mut a = Ase::new(6, 5, 32);
    a.frame(100)
        .palette(&[RED, GREEN])
        .layer("group", VISIBLE, 1, 0, 0, 200)
        .layer("a", VISIBLE, 0, 1, 0, 255)
        .image_layer("b")
        .cel_zlib(1, 1, 1, 2, 2, &stored_zlib(&fill(2, 2, BLUE)))
        .user_data(Some("hi"), &[("n", Prop::I32(1))])
        .cel(2, 0, 0, 1, 1, &RED)
        .tags(&[(0, 1, 2, 0, "t")])
        .user_data(Some("tag"), &[])
        .slice(
            "pivot",
            &[Key {
                frame: 0,
                bounds: [0, 0, 6, 5],
                center: Some([1, 1, 2, 2]),
                pivot: Some([3, 5]),
            }],
        )
        .frame(80)
        .cel_link(1, 0)
        .cel(2, 2, 2, 1, 1, &GREEN);
    a.bytes()
}

#[test]
fn a_malformed_file_is_refused_with_why_and_never_panics() {
    let dir = project("malformed");
    let mut text = b"this is a text file someone named .aseprite".to_vec();
    text.resize(200, b' ');
    put(&dir, "notes.aseprite", &text);
    put(&dir, "tiny.aseprite", &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    let good = everything();
    put(&dir, "cut.aseprite", &good[..good.len() - 7]);
    let mut huge = good.clone();
    huge[8..12].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
    put(&dir, "huge.aseprite", &huge);
    put(&dir, "good.aseprite", &good);
    let mut imp = Importer::new(&dir);
    imp.scan();
    let why = |n: &str| {
        let f = &imp.failures()[n].error;
        assert_eq!(f.code, argus_assets::codes::BAD_SOURCE);
        assert_eq!(
            f.r#ref.as_deref(),
            Some(format!("assets/{n}.aseprite").as_str())
        );
        f.message.clone()
    };
    assert_eq!(
        why("notes"),
        "assets/notes.aseprite cannot be read as an Aseprite file: its header's magic number is 0x6920, not Aseprite's 0xA5E0"
    );
    assert_eq!(
        why("tiny"),
        "assets/tiny.aseprite cannot be read as an Aseprite file: it is 10 bytes, shorter than an Aseprite header (128)"
    );
    assert!(why("cut").contains("frame 1 at byte"), "{}", why("cut"));
    assert!(
        why("huge").contains("its canvas is 65535 x 65535"),
        "{}",
        why("huge")
    );
    let e = imp.get("good").unwrap();
    assert!(e.warnings.is_empty(), "{:?}", messages(e));
    let s = sprite(e);
    assert_eq!(pixels(s, 0)[6 + 1], [0, 0, 255, 200], "the group at 200");
    assert_eq!(s.frames[0].properties["data"], "hi");
    assert_eq!(s.frames[1].properties["data"], "hi", "a link's user data");
    assert_eq!(s.frames[0].pivot, [0.5, 0.0]);

    // Every way of cutting it short is an error, never a panic.
    for n in 0..good.len() {
        assert!(aseprite::describe(&good[..n]).is_err(), "cut at {n}");
    }
    assert_eq!(aseprite::describe(&good).unwrap().layers.len(), 3);
    // Every byte flipped: an import or a failure with a message, never a
    // panic, however the sizes and counts come out.
    let mut imp = Importer::new(&dir);
    for i in 0..good.len() {
        let mut bad = good.clone();
        bad[i] ^= 0xFF;
        let _ = aseprite::describe(&bad);
        if i % 3 == 0 {
            put(&dir, "good.aseprite", &bad);
            imp.touch("good");
            let r = imp.scan();
            for f in &r.failed {
                assert!(!f.error.message.is_empty());
            }
        }
    }
}
