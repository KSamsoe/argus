//! Sprite pages (phase 5 item 1): BC7 blocks the GPU decodes draw exactly
//! as the same blocks decoded here, which is what a device without BC7
//! draws, and what decides whether a sprite ships compressed. The two
//! encodings the encoder writes are checked on every adapter there is.

use argus_assets::bc7;
use argus_ecs::{Transform, Vec2, World};
use argus_render::{Camera, Renderer, Sprite, Texels};

const N: u32 = 64;

/// Smooth colour, and a flat one fading across: blocks of both kinds the
/// encoder writes (both draws blend over the same background).
fn picture() -> Vec<u8> {
    let mut out = Vec::new();
    for y in 0..N {
        for x in 0..N {
            if (x / 8 + y / 8) % 2 == 0 {
                out.extend([(x * 4) as u8, (y * 4) as u8, 128, 255]);
            } else {
                out.extend([30, 20, 60, (100 + (x % 8) * 20) as u8]);
            }
        }
    }
    out
}

fn drawn(r: &mut Renderer, atlas: &str) -> argus_render::Image {
    let mut w = World::new();
    w.register::<Transform>().unwrap();
    argus_render::register(&mut w).unwrap();
    let cam = w.spawn_authored("camera").unwrap();
    w.insert(cam, Transform::at(0.0, 0.0)).unwrap();
    w.insert(
        cam,
        Camera {
            height: N as f32,
            ..Default::default()
        },
    )
    .unwrap();
    let e = w.spawn();
    w.insert(e, Transform::at(0.0, 0.0)).unwrap();
    let s = Sprite {
        atlas: w.strings().intern(atlas),
        size: Vec2::new(N as f32, N as f32),
        ..Default::default()
    };
    w.insert(e, s).unwrap();
    r.render(&w, 0, 1.0).unwrap();
    r.capture().unwrap()
}

#[test]
fn bc7_blocks_draw_as_they_decode_here() {
    let src = picture();
    let blocks = bc7::encode(N, N, &src);
    for mode in [5, 6] {
        assert!(
            blocks.chunks(16).any(|b| b[0].trailing_zeros() == mode),
            "a block in mode {mode}"
        );
    }
    let here = bc7::decode(N, N, &blocks).unwrap();
    let mut tried = 0;
    for software in [false, true] {
        let Ok(mut r) = Renderer::offscreen(N, N, software) else {
            continue;
        };
        if !r.gpu().bc {
            continue;
        }
        tried += 1;
        let g = r.gpu_mut();
        let on_gpu = g.add_page("bc7", N, N, Texels::Bc7(&blocks));
        g.add_atlas_on("bc7", &on_gpu, vec![[0.0, 0.0, 1.0, 1.0]], vec![[0.5, 0.5]]);
        let decoded = g.add_page("here", N, N, Texels::Rgba(&here));
        g.add_atlas_on(
            "here",
            &decoded,
            vec![[0.0, 0.0, 1.0, 1.0]],
            vec![[0.5, 0.5]],
        );
        let (a, b) = (drawn(&mut r, "bc7"), drawn(&mut r, "here"));
        let colours: std::collections::BTreeSet<&[u8]> = a.rgba.chunks(4).collect();
        assert!(
            colours.len() > 64,
            "the picture was drawn: {} colours",
            colours.len()
        );
        let c = a.compare(&b, 0.0).unwrap();
        assert_eq!(
            c.differing,
            0,
            "{}: {} of {} pixels differ",
            r.gpu().adapter,
            c.differing,
            c.pixels
        );
    }
    if tried == 0 {
        eprintln!("skipping: no adapter here reads BC7");
    }
}
