//! §9: "Everything runs identically in CI with the software render
//! adapter." The software rasteriser (WARP on Windows, lavapipe or llvmpipe
//! on Linux) draws what the GPU draws, as a person would see it: shapes,
//! and the same with a stack of post passes. Found by phase 5 item 0's
//! image goldens: on WARP a constant buffer bound through the descriptor
//! heap read the one in the next bind group made with the same layout, so
//! the world drew with the UI's view and every post pass with the last
//! pass's numbers, and a frame showed nothing but its text.

use argus_ecs::{Transform, Vec2, World};
use argus_render::{Camera, Renderer, Sprite};

fn world_with_camera(height: f32) -> World {
    let mut w = World::new();
    w.register::<Transform>().unwrap();
    argus_render::register(&mut w).unwrap();
    let cam = w.spawn_authored("camera").unwrap();
    w.insert(cam, Transform::at(0.0, 0.0)).unwrap();
    w.insert(
        cam,
        Camera {
            height,
            ..Default::default()
        },
    )
    .unwrap();
    w
}

fn shape(w: &mut World, x: f32, y: f32, kind: &str, size: f32, color: &str) {
    let e = w.spawn();
    w.insert(e, Transform::at(x, y)).unwrap();
    let s = Sprite {
        kind: w.strings().intern(kind),
        size: Vec2::new(size, size),
        color: w.strings().intern(color),
        ..Default::default()
    };
    w.insert(e, s).unwrap();
}

/// A GPU's renderer and the software one, or none on a machine with no GPU
/// (then the software one is all there is, and nothing to compare).
fn pair(w: u32, h: u32) -> Option<(Renderer, Renderer)> {
    let soft = match Renderer::offscreen(w, h, true) {
        Ok(r) => r,
        Err(e) if e.code == argus_render::codes::NO_ADAPTER => {
            eprintln!("skipping: {e}");
            return None;
        }
        Err(e) => panic!("the software renderer did not start: {e}"),
    };
    assert!(
        soft.gpu().software,
        "{} is not the software rasteriser",
        soft.gpu().adapter
    );
    let gpu = Renderer::offscreen(w, h, false).ok()?;
    if gpu.gpu().software {
        eprintln!("skipping: no GPU here to compare with");
        return None;
    }
    Some((gpu, soft))
}

/// The same world drawn by both: the share of pixels a person would see
/// differ.
fn differ(world: &World) -> f64 {
    let Some((mut gpu, mut soft)) = pair(320, 180) else {
        return 0.0;
    };
    gpu.render(world, 0, 1.0).unwrap();
    soft.render(world, 0, 1.0).unwrap();
    let (a, b) = (soft.capture().unwrap(), gpu.capture().unwrap());
    let c = a.compare(&b, 0.1).unwrap();
    println!(
        "software ({}) against {}: {} of {} pixels differ",
        soft.gpu().adapter,
        gpu.gpu().adapter,
        c.differing,
        c.pixels
    );
    c.fraction()
}

#[test]
fn the_software_rasteriser_draws_what_the_gpu_draws() {
    let mut w = world_with_camera(10.0);
    shape(&mut w, 0.0, 0.0, "rect", 2.0, "#E8703F");
    shape(&mut w, 6.0, 2.0, "circle", 3.0, "#3FA0E8");
    shape(&mut w, -6.0, -2.0, "rect", 1.0, "#70E83F");
    shape(&mut w, 50.0, 0.0, "rect", 2.0, "#FFFFFF"); // off the view: culled
    let f = differ(&w);
    assert!(f < 0.005, "{:.2}% of the pixels differ", f * 100.0);
}

#[test]
fn and_through_five_post_passes() {
    use argus_post::*;
    let mut w = world_with_camera(10.0);
    argus_post::register(&mut w).unwrap();
    for i in 0..40 {
        shape(
            &mut w,
            (i % 10) as f32 * 2.0 - 9.0,
            (i / 10) as f32 * 2.0 - 3.0,
            if i % 3 == 0 { "circle" } else { "rect" },
            1.2,
            if i % 2 == 0 { "#8A6A40" } else { "#40A08A" },
        );
    }
    let cam = w.entity("camera").unwrap();
    w.insert(
        cam,
        PostFlash {
            amount: 0.3,
            ..Default::default()
        },
    )
    .unwrap();
    w.insert(cam, PostAberration::default()).unwrap();
    w.insert(cam, PostVignette::default()).unwrap();
    w.insert(
        cam,
        PostPalette {
            amount: 0.5,
            ..Default::default()
        },
    )
    .unwrap();
    let f = differ(&w);
    assert!(f < 0.01, "{:.2}% of the pixels differ", f * 100.0);
}

/// A sprite drawn larger than its art keeps every texel a hard-edged
/// square, as a GPU draws it. WARP minifies a sample at level 0 where a GPU
/// magnifies, and with the atlas sampler's linear min filter it blurred
/// zoomed-in sprites and spread a frame's edge column into a clear one
/// beside it, so picture goldens did not show what players see.
#[test]
fn a_zoomed_in_sprite_stays_sharp() {
    let mut r = match Renderer::offscreen(64, 36, true) {
        Ok(r) => r,
        Err(e) if e.code == argus_render::codes::NO_ADAPTER => {
            eprintln!("skipping: {e}");
            return;
        }
        Err(e) => panic!("the software renderer did not start: {e}"),
    };
    // A clear texel, then a red one, 18 pixels each.
    let art: Vec<u8> = [[0u8, 0, 0, 0], [255, 0, 0, 255]].concat();
    r.gpu_mut()
        .add_atlas_rgba("art", 2, 1, &art, vec![[0.0, 0.0, 1.0, 1.0]]);
    let mut w = world_with_camera(4.0);
    r.render(&w, 0, 1.0).unwrap();
    let bg = r.capture().unwrap().pixel(32, 18);
    let e = w.spawn();
    w.insert(e, Transform::at(0.0, 0.0)).unwrap();
    let s = Sprite {
        atlas: w.strings().intern("art"),
        size: Vec2::new(4.0, 2.0),
        ..Default::default()
    };
    w.insert(e, s).unwrap();
    r.render(&w, 1, 1.0).unwrap();
    let img = r.capture().unwrap();
    let near = |p: [u8; 4], q: [u8; 4]| (0..3).all(|i| (p[i] as i32 - q[i] as i32).abs() <= 2);
    let red = [255, 0, 0, 255];
    let between: Vec<(u32, [u8; 4])> = (14..50)
        .map(|x| (x, img.pixel(x, 18)))
        .filter(|&(_, p)| !near(p, bg) && !near(p, red))
        .collect();
    assert!(
        between.is_empty(),
        "{}: pixels neither the background {bg:?} nor red: {between:?}",
        r.gpu().adapter
    );
    assert!(near(img.pixel(40, 18), red), "{:?}", img.pixel(40, 18));
}

/// A Background on the drawing camera is what shows where nothing is drawn;
/// without one, the engine's dark grey (a band around Delve's map and the
/// templates' floors, before there was a Background).
#[test]
fn the_background_is_the_cameras() {
    let mut r = match Renderer::offscreen(64, 36, true) {
        Ok(r) => r,
        Err(e) if e.code == argus_render::codes::NO_ADAPTER => {
            eprintln!("skipping: {e}");
            return;
        }
        Err(e) => panic!("the software renderer did not start: {e}"),
    };
    let mut w = world_with_camera(10.0);
    r.render(&w, 0, 1.0).unwrap();
    let grey = r.capture().unwrap().pixel(1, 1);
    let cam = w.entity("@camera").unwrap();
    let color = w.strings().intern("#336699");
    w.insert(cam, argus_render::Background { color }).unwrap();
    r.render(&w, 1, 1.0).unwrap();
    let p = r.capture().unwrap().pixel(1, 1);
    let near = |a: u8, b: u8| (a as i32 - b as i32).abs() <= 2;
    assert!(
        near(p[0], 0x33) && near(p[1], 0x66) && near(p[2], 0x99),
        "{p:?}, the grey was {grey:?}"
    );
    assert_ne!(p, grey);
}
