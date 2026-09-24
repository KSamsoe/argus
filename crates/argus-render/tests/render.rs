//! §2 the core hosts the renderer; §6 rendering interpolates between the
//! last two sim states; §14 described-shape placeholders render without
//! an asset; §17 instanced sprites, one draw call per atlas, GPU culling;
//! §9 identical offscreen in CI.

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

fn shape(
    w: &mut World,
    x: f32,
    y: f32,
    kind: &str,
    size: f32,
    color: &str,
    z: f32,
) -> argus_ecs::Entity {
    let e = w.spawn();
    w.insert(e, Transform::at(x, y)).unwrap();
    let s = Sprite {
        kind: w.strings().intern(kind),
        size: Vec2::new(size, size),
        color: w.strings().intern(color),
        z,
        ..Default::default()
    };
    w.insert(e, s).unwrap();
    e
}

/// Offscreen renderer or skip: a machine with no adapter at all (not even
/// the software rasteriser) cannot run these, and says so. Any other
/// failure to start is a bug and fails the test.
fn renderer(w: u32, h: u32) -> Option<Renderer> {
    match Renderer::offscreen(w, h, false) {
        Ok(r) => Some(r),
        Err(e) if e.code == argus_render::codes::NO_ADAPTER => {
            eprintln!("skipping: {e}");
            None
        }
        Err(e) => panic!("the renderer did not start: {e}"),
    }
}

#[test]
fn described_shapes_render_where_the_camera_puts_them() {
    let Some(mut r) = renderer(200, 100) else {
        return;
    };
    // A camera 10 units tall over a 2:1 target sees x in [-10, 10], y in [-5, 5].
    let mut w = world_with_camera(10.0);
    shape(&mut w, 0.0, 0.0, "rect", 2.0, "#FF0000", 0.0);
    shape(&mut w, 8.0, 3.0, "circle", 2.0, "#00FF00", 0.0);
    shape(&mut w, 50.0, 0.0, "rect", 2.0, "#0000FF", 0.0); // off screen: culled on the GPU
    let rep = r.render(&w, 0, 0.0).unwrap();
    assert_eq!(
        rep.stats.sprites, 3,
        "every sprite is submitted; the GPU culls"
    );
    assert_eq!(rep.stats.draw_calls, 2, "rect run then circle run");
    assert!(rep.missing_atlases.is_empty());
    let img = r.capture().unwrap();
    assert_eq!(
        img.pixel(100, 50)[..3],
        [255, 0, 0],
        "red rect at the centre"
    );
    // 2 units at 10 px/unit: a 20 px square.
    assert_eq!(
        img.count_color([255, 0, 0], 2),
        400,
        "{}",
        img.count_color([255, 0, 0], 2)
    );
    let (cx, cy) = r.to_pixel(8.0, 3.0);
    assert_eq!(
        img.pixel(cx as u32, cy as u32)[..3],
        [0, 255, 0],
        "green circle centre"
    );
    let green = img.count_color([0, 255, 0], 2);
    assert!(
        green > 250 && green < 340,
        "a circle of radius 10 px: {green}"
    );
    assert_eq!(
        img.count_color([0, 0, 255], 2),
        0,
        "the far rect never rasterised"
    );
    assert_eq!(
        img.pixel(5, 5)[..3],
        [69, 69, 80],
        "clear colour (linear 0.06) in sRGB elsewhere"
    );
    let path = std::env::temp_dir().join(format!("argus-render-{}.png", std::process::id()));
    img.save_png(&path).unwrap();
    assert!(std::fs::metadata(&path).unwrap().len() > 100);
    std::fs::remove_file(&path).ok();
}

#[test]
fn z_orders_and_atlas_runs_make_the_draw_calls() {
    let Some(mut r) = renderer(64, 64) else {
        return;
    };
    let mut w = world_with_camera(8.0);
    // Same spot: the higher z wins the pixel regardless of spawn order.
    shape(&mut w, 0.0, 0.0, "rect", 4.0, "#0000FF", 5.0);
    shape(&mut w, 0.0, 0.0, "rect", 4.0, "#FF0000", 1.0);
    let rep = r.render(&w, 0, 0.0).unwrap();
    assert_eq!(rep.stats.draw_calls, 1, "one atlas, one run");
    assert_eq!(r.capture().unwrap().pixel(32, 32)[..3], [0, 0, 255]);
    // Interleaving z across atlases costs extra runs, and only then.
    shape(&mut w, 2.0, 2.0, "circle", 1.0, "#FFFFFF", 3.0);
    let rep = r.render(&w, 0, 0.0).unwrap();
    assert_eq!(rep.stats.draw_calls, 3, "rect z1, circle z3, rect z5");
    assert_eq!(rep.stats.atlases, 2);
}

#[test]
fn positions_interpolate_between_the_last_two_ticks() {
    let Some(mut r) = renderer(100, 100) else {
        return;
    };
    let mut w = world_with_camera(10.0);
    let e = shape(&mut w, -4.0, 0.0, "rect", 1.0, "#FFFFFF", 0.0);
    r.render(&w, 0, 0.0).unwrap();
    // The sim moved it 4 units right during tick 1; real time is half way.
    w.set_field(e, "Transform.pos", Vec2::new(0.0, 0.0))
        .unwrap();
    let rep = r.render(&w, 1, 0.5).unwrap();
    assert_eq!(rep.interpolated, 1);
    let img = r.capture().unwrap();
    assert_eq!(
        img.pixel(30, 50)[..3],
        [255, 255, 255],
        "drawn at x = -2, half way"
    );
    assert_ne!(img.pixel(50, 50)[..3], [255, 255, 255]);
    // A second frame within the same tick still interpolates from tick 0
    // (a display faster than the sim draws several frames per tick).
    let rep = r.render(&w, 1, 0.75).unwrap();
    assert_eq!(rep.interpolated, 1, "every frame of a tick interpolates");
    assert_eq!(
        r.capture().unwrap().pixel(40, 50)[..3],
        [255, 255, 255],
        "drawn at x = -1, three quarters of the way"
    );
    // alpha 1 (or the discrete clock) draws the current state.
    let rep = r.render(&w, 1, 1.0).unwrap();
    assert_eq!(rep.interpolated, 0);
    assert_eq!(r.capture().unwrap().pixel(50, 50)[..3], [255, 255, 255]);
    // A tick gap (two ticks since last frame) is not interpolated across.
    w.set_field(e, "Transform.pos", Vec2::new(4.0, 0.0))
        .unwrap();
    let rep = r.render(&w, 3, 0.5).unwrap();
    assert_eq!(rep.interpolated, 0);
}

#[test]
fn a_named_atlas_loads_from_assets_and_a_missing_one_is_reported() {
    let Some(mut r) = renderer(64, 64) else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("argus-atlas-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    // A 2x1 PNG: left pixel yellow, right pixel cyan, two frames by grid.
    let png_path = dir.join("assets").join("tiles.png");
    let img = argus_render::Image {
        width: 2,
        height: 1,
        rgba: vec![255, 255, 0, 255, 0, 255, 255, 255],
    };
    img.save_png(&png_path).unwrap();
    std::fs::write(
        dir.join("assets").join("tiles.atlas.toml"),
        "grid = [2, 1]\n",
    )
    .unwrap();
    let names = r.load_atlases(&dir).unwrap();
    assert_eq!(names, vec!["tiles"]);
    assert_eq!(r.gpu().atlas("tiles").unwrap().frames.len(), 2);

    let mut w = world_with_camera(8.0);
    let e = w.spawn();
    w.insert(e, Transform::at(0.0, 0.0)).unwrap();
    let atlas = w.strings().intern("tiles");
    w.insert(
        e,
        Sprite {
            atlas,
            frame: 1,
            size: Vec2::new(4.0, 4.0),
            ..Default::default()
        },
    )
    .unwrap();
    let ghost = w.spawn();
    w.insert(ghost, Transform::at(-3.0, 0.0)).unwrap();
    let nope = w.strings().intern("nope");
    w.insert(
        ghost,
        Sprite {
            atlas: nope,
            size: Vec2::new(1.0, 1.0),
            ..Default::default()
        },
    )
    .unwrap();
    let rep = r.render(&w, 0, 0.0).unwrap();
    assert_eq!(rep.missing_atlases, vec!["nope".to_string()]);
    let img = r.capture().unwrap();
    assert_eq!(
        img.pixel(32, 32)[..3],
        [0, 255, 255],
        "frame 1 is the cyan half"
    );
    let (gx, gy) = r.to_pixel(-3.0, 0.0);
    assert_eq!(
        img.pixel(gx as u32, gy as u32)[..3],
        [255, 0, 255],
        "a missing atlas draws magenta"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// §17: 200,000 instanced sprites per frame. Release only.
#[test]
#[ignore]
fn perf_200k_sprites_per_frame() {
    let Some(mut r) = renderer(1280, 720) else {
        return;
    };
    let mut w = world_with_camera(200.0);
    let mut seed = 7u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % 100_000) as f32 / 250.0 - 200.0
    };
    for i in 0..200_000 {
        let kind = if i % 2 == 0 { "rect" } else { "circle" };
        shape(
            &mut w,
            next(),
            next(),
            kind,
            1.5,
            "#E8703F",
            ((i / 2) % 4) as f32,
        );
    }
    r.render(&w, 0, 0.0).unwrap();
    let mut best = f64::MAX;
    for _ in 0..5 {
        let rep = r.render(&w, 0, 0.0).unwrap();
        assert_eq!(rep.stats.sprites, 200_000);
        assert_eq!(rep.stats.draw_calls, 8, "four z values, two atlases");
        best = best.min(rep.ms);
    }
    eprintln!(
        "200k sprites: build + submit {best:.2} ms per frame on {}",
        r.gpu().adapter
    );
    assert!(best < 40.0, "{best} ms");
}

/// §14: a placeholder asset and the art that replaces it are one name; the
/// sprite's pivot puts its feet on the position.
#[test]
fn a_placeholder_asset_draws_with_its_pivot_and_the_art_replaces_it() {
    let Some(mut r) = renderer(100, 100) else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("argus-hero-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets/sprites")).unwrap();
    std::fs::write(
        dir.join("assets/sprites/hero.toml"),
        "kind = \"rect\"\nsize = [8, 8]\ncolor = \"#00FF00\"\npivot = [0.5, 0.0]\n",
    )
    .unwrap();
    let mut imp = argus_assets::Importer::new(&dir);
    imp.scan();
    assert_eq!(r.load_catalog(&imp).unwrap(), vec!["sprites/hero"]);

    let mut w = world_with_camera(10.0);
    let e = w.spawn();
    w.insert(e, Transform::at(0.0, 0.0)).unwrap();
    let hero = w.strings().intern("sprites/hero");
    w.insert(
        e,
        Sprite {
            atlas: hero,
            size: Vec2::new(4.0, 4.0),
            ..Default::default()
        },
    )
    .unwrap();
    r.render(&w, 0, 0.0).unwrap();
    let img = r.capture().unwrap();
    // Feet at the origin: the sprite covers y 0..4, above the centre row.
    assert_eq!(img.pixel(50, 45)[..3], [0, 255, 0], "above the position");
    assert_ne!(img.pixel(50, 55)[..3], [0, 255, 0], "nothing below it");

    // The art arrives under the same name; the entity is untouched.
    argus_assets::encode_png(
        &dir.join("assets/sprites/hero.png"),
        2,
        2,
        &[255, 0, 0, 255].repeat(4),
    )
    .unwrap();
    let rep = imp.poll().unwrap();
    assert_eq!(rep.imported, vec!["sprites/hero"]);
    r.reload_sprites(&imp, &rep.imported).unwrap();
    r.render(&w, 0, 0.0).unwrap();
    let img = r.capture().unwrap();
    assert_eq!(
        img.pixel(50, 45)[..3],
        [255, 0, 0],
        "the real art, same reference"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Item 7: the drawing camera is the active one with the highest priority,
/// and its shake moves and rolls the view (the camera's entity stays put).
#[test]
fn the_priority_camera_draws_and_its_shake_moves_and_rolls_the_view() {
    let Some(mut r) = renderer(100, 100) else {
        return;
    };
    let mut w = world_with_camera(10.0);
    let boss = w.spawn();
    w.insert(boss, Transform::at(20.0, 0.0)).unwrap();
    w.insert(
        boss,
        Camera {
            height: 10.0,
            priority: 5,
            ..Default::default()
        },
    )
    .unwrap();
    shape(&mut w, 20.0, 0.0, "rect", 2.0, "#FF0000", 0.0);
    shape(&mut w, 0.0, 0.0, "rect", 2.0, "#00FF00", 0.0);
    shape(&mut w, 22.0, 0.0, "rect", 1.0, "#0000FF", 1.0);
    let rep = r.render(&w, 0, 0.0).unwrap();
    assert_eq!(rep.camera, Some(boss));
    assert_eq!(rep.view_center, [20.0, 0.0]);
    let img = r.capture().unwrap();
    assert_eq!(
        img.pixel(50, 50)[..3],
        [255, 0, 0],
        "the boss camera frames the red box"
    );
    assert_eq!(img.pixel(70, 50)[..3], [0, 0, 255], "blue two units right");

    // Shake moves the view one unit right: red lands ten pixels left.
    w.insert(
        boss,
        argus_render::CameraShake {
            offset: Vec2::new(1.0, 0.0),
            ..Default::default()
        },
    )
    .unwrap();
    r.render(&w, 1, 1.0).unwrap();
    assert_eq!(r.capture().unwrap().pixel(40, 50)[..3], [255, 0, 0]);
    // A quarter-turn roll: what is to the right of centre shows below it.
    w.update::<argus_render::CameraShake, _>(boss, |s| {
        s.offset = Vec2::ZERO;
        s.angle = std::f32::consts::FRAC_PI_2;
    })
    .unwrap();
    let rep = r.render(&w, 2, 1.0).unwrap();
    assert!((rep.view_angle - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    let img = r.capture().unwrap();
    assert_eq!(img.pixel(50, 70)[..3], [0, 0, 255], "rolled a quarter turn");
    let (px, py) = r.to_pixel(22.0, 0.0);
    assert!(
        (px - 50.0).abs() < 0.5 && (py - 70.0).abs() < 0.5,
        "to_pixel knows the roll: {px} {py}"
    );

    // Deactivated, it hands the view back to the other camera.
    w.update::<Camera, _>(boss, |c| c.active = false).unwrap();
    let rep = r.render(&w, 3, 1.0).unwrap();
    assert_eq!(rep.view_center, [0.0, 0.0]);
    assert_eq!(r.capture().unwrap().pixel(50, 50)[..3], [0, 255, 0]);
}

/// §13: particles are drawn instanced, one run per pool from its effect's
/// atlas at its z, culled to the view, and between their last two ticks'
/// positions like any sprite.
#[test]
fn particle_pools_draw_as_one_run_each_between_ticks() {
    use argus_particles::{Emitter, ParticleStore};
    use std::cell::RefCell;
    use std::rc::Rc;
    let Some(mut r) = renderer(100, 100) else {
        return;
    };
    let mut w = world_with_camera(10.0);
    let mut sched = argus_sim::Scheduler::new(argus_sim::SchedulerConfig::continuous(60.0).seed(1));
    let store = Rc::new(RefCell::new(ParticleStore::default()));
    argus_particles::install(&mut w, &mut sched, store.clone()).unwrap();
    for (name, text) in [
        ("fx/red", "burst = 1\nspeed = 60\ndirection = 0\nspread = 0\nlifetime = 100\nsize = 1\ncolor = \"#FF0000\"\nz = 1\n"),
        ("fx/green", "burst = 3\nspeed = 0\nlifetime = 100\nsize = 1\ncolor = \"#00FF00\"\nparticle = \"circle\"\nz = 2\n"),
    ] {
        let (spec, _) = argus_assets::effect::parse_emitter(name, text).unwrap();
        store.borrow_mut().set_effect(name, &spec);
    }
    for (fx, x, y) in [
        ("fx/red", -4.0, 0.0),
        ("fx/green", 3.0, 3.0),
        ("fx/green", 100.0, 0.0),
    ] {
        let e = w.spawn();
        w.insert(e, Transform::at(x, y)).unwrap();
        let effect = w.strings().intern(fx);
        w.insert(
            e,
            Emitter {
                effect,
                ..Default::default()
            },
        )
        .unwrap();
    }
    // Born at x = -4, then three ticks at one unit a tick.
    sched.step(&mut w, 4).unwrap();
    let tick = sched.tick();
    let rep = r
        .render_frame(&w, tick, 1.0, &[], Some(&store.borrow()))
        .unwrap();
    assert_eq!(rep.particles, 4, "the far pool is out of view");
    assert_eq!(rep.stats.draw_calls, 2, "a rect run and a circle run");
    let img = r.capture().unwrap();
    let px = |x: f32, y: f32| {
        let (a, b) = r.to_pixel(x, y);
        img.pixel(a as u32, b as u32)
    };
    assert_eq!(px(-1.0, 0.0)[..3], [255, 0, 0]);
    assert_eq!(px(3.0, 3.0)[..3], [0, 255, 0]);
    assert_ne!(px(-1.8, 0.0)[..3], [255, 0, 0]);
    // Half way through the next tick's real time: half a step back.
    r.render_frame(&w, tick, 0.5, &[], Some(&store.borrow()))
        .unwrap();
    let img = r.capture().unwrap();
    let px = |x: f32, y: f32| {
        let (a, b) = r.to_pixel(x, y);
        img.pixel(a as u32, b as u32)
    };
    assert_eq!(px(-1.8, 0.0)[..3], [255, 0, 0]);
    assert_ne!(px(-0.7, 0.0)[..3], [255, 0, 0]);
}

/// Phase 2 item 2: the bar the window shows while a mark's note is typed,
/// drawn over the frame's bottom edge pixel for pixel, and gone when set
/// to none.
#[test]
fn an_overlay_bar_draws_over_the_bottom_of_the_frame() {
    let Some(mut r) = renderer(480, 270) else {
        return;
    };
    let mut w = world_with_camera(10.0);
    shape(&mut w, 0.0, 0.0, "rect", 40.0, "#20A040", 0.0);
    let bar = argus_render::status_bar(
        480,
        "MARK 3 AT 0:41.2  NOTE: dash feels late_",
        [255, 214, 90, 255],
    );
    assert_eq!(bar.height, 16);
    r.gpu_mut().set_overlay(Some(&bar));
    r.render(&w, 0, 1.0).unwrap();
    let img = r.capture().unwrap();
    if let Ok(dir) = std::env::var("ARGUS_SHOTS") {
        img.save_png(&std::path::Path::new(&dir).join("overlay.png"))
            .unwrap();
    }
    // Above the bar, the green the rect fills the view with.
    let above = img.pixel(240, 270 - 17);
    assert!(above[1] > 120 && above[0] < 80, "{above:?}");
    // The bar: dark (a little of the green shows through), the text on it.
    let dark = img.pixel(470, 270 - 3);
    assert!(dark[0] < 40 && dark[1] < 80, "{dark:?}");
    let yellow = (254..270).any(|y| {
        (0..300).any(|x| {
            let p = img.pixel(x, y);
            p[0] > 200 && p[1] > 170 && p[2] < 140
        })
    });
    assert!(yellow, "the text is on the bar");

    r.gpu_mut().set_overlay(None);
    r.render(&w, 0, 1.0).unwrap();
    let img = r.capture().unwrap();
    let p = img.pixel(470, 270 - 3);
    assert!(p[1] > 120, "gone: {p:?}");
}

/// The game's own text (found by a mark: Hollow's signs spoke to the agent
/// only): a shown Describe's words over its sprite, a HUD line at the top
/// left, nothing for one not shown.
#[test]
fn shown_text_is_drawn_over_its_entity_and_the_hud_at_the_top() {
    let Some(mut r) = renderer(640, 360) else {
        return;
    };
    let mut w = world_with_camera(12.0);
    let sign = shape(&mut w, 0.0, -2.0, "rect", 1.0, "#8A6A40", 0.0);
    let quiet = shape(&mut w, 5.0, -2.0, "rect", 1.0, "#8A6A40", 0.0);
    let hud = w.spawn();
    for (e, text, shown) in [
        (sign, "Mind the pit.", true),
        (quiet, "Not shown.", false),
        (hud, "Hollow. HP {@camera:Camera.height}", true),
    ] {
        let d = argus_render::Describe {
            text: w.strings().intern(text),
            role: w.strings().intern(if e == hud { "hud" } else { "" }),
            shown,
            ..Default::default()
        };
        w.insert(e, d).unwrap();
    }
    r.render(&w, 0, 1.0).unwrap();
    let img = r.capture().unwrap();
    if let Ok(dir) = std::env::var("ARGUS_SHOTS") {
        img.save_png(&std::path::Path::new(&dir).join("text.png"))
            .unwrap();
    }
    let cream = |x0: u32, y0: u32, x1: u32, y1: u32| {
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let p = img.pixel(x, y);
                p[0] > 200 && p[1] > 190 && p[2] > 150
            })
            .count()
    };
    // 30 px a unit: the sign's top is at y -1.5, row 225; its words sit above.
    let (sx, _) = r.to_pixel(0.0, -1.5);
    assert!(
        cream(sx as u32 - 45, 195, sx as u32 + 45, 224) > 40,
        "the sign's words"
    );
    let (qx, _) = r.to_pixel(5.0, -1.5);
    assert_eq!(
        cream(qx as u32 - 40, 195, qx as u32 + 40, 224),
        0,
        "not shown, not drawn"
    );
    assert!(cream(0, 0, 220, 16) > 40, "the HUD line at the top left");
}

/// Phase 2 item 6: the post stack. Each pass changes the picture as it
/// says, in the drawing camera's order, and the F8 bar stays still under a
/// shake.
#[test]
fn post_passes_change_the_picture_in_order() {
    use argus_post::*;
    let Some(mut r) = renderer(320, 180) else {
        return;
    };
    let mut w = world_with_camera(18.0);
    argus_post::register(&mut w).unwrap();
    let cam = w.entity("camera").unwrap();
    shape(&mut w, 0.0, 0.0, "rect", 4.0, "#FF0000", 0.0);
    let shot = |r: &mut argus_render::Renderer, w: &argus_ecs::World, name: &str| {
        r.render(w, 0, 1.0).unwrap();
        let img = r.capture().unwrap();
        if let Ok(dir) = std::env::var("ARGUS_SHOTS") {
            img.save_png(&std::path::Path::new(&dir).join(format!("post-{name}.png")))
                .unwrap();
        }
        img
    };
    let plain = shot(&mut r, &w, "none");
    assert_eq!(plain.pixel(160, 90)[..3], [255, 0, 0]);
    let clear = plain.pixel(5, 90);

    // Flash: half way to white.
    w.insert(
        cam,
        PostFlash {
            amount: 0.5,
            ..Default::default()
        },
    )
    .unwrap();
    let img = shot(&mut r, &w, "flash");
    let p = img.pixel(160, 90);
    assert!(p[0] == 255 && p[1] > 150 && p[1] < 230, "pink: {p:?}");
    w.remove(cam, w.component_id("PostFlash").unwrap()).unwrap();

    // Fade all the way: black.
    w.insert(
        cam,
        PostFade {
            amount: 1.0,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(shot(&mut r, &w, "fade").pixel(160, 90)[..3], [0, 0, 0]);
    w.remove(cam, w.component_id("PostFade").unwrap()).unwrap();

    // Shake: the picture moves right by a tenth of the height (18 px).
    w.insert(
        cam,
        PostShake {
            offset: argus_ecs::Vec2::new(0.1, 0.0),
            ..Default::default()
        },
    )
    .unwrap();
    let img = shot(&mut r, &w, "shake");
    // The rect spans x 140..180 still; shaken, 158..198.
    assert_eq!(
        img.pixel(150, 90)[..3],
        clear[..3],
        "the left edge moved away"
    );
    assert_eq!(
        img.pixel(190, 90)[..3],
        [255, 0, 0],
        "and the right edge came"
    );
    // The bar is not shaken.
    let bar = argus_render::status_bar(320, "F8", [255, 214, 90, 255]);
    r.gpu_mut().set_overlay(Some(&bar));
    let img = shot(&mut r, &w, "shake-bar");
    let top = img.pixel(0, 180 - bar.height);
    assert!(
        top[0] > 200 && top[1] > 170,
        "the bar's top line at its place: {top:?}"
    );
    r.gpu_mut().set_overlay(None);
    w.remove(cam, w.component_id("PostShake").unwrap()).unwrap();

    // Vignette: the corners darken, the middle does not.
    w.insert(
        cam,
        PostVignette {
            amount: 1.0,
            radius: 0.3,
            softness: 0.3,
            ..Default::default()
        },
    )
    .unwrap();
    let img = shot(&mut r, &w, "vignette");
    assert_eq!(img.pixel(160, 90)[..3], [255, 0, 0]);
    assert!(
        img.pixel(1, 1)[0] < clear[0] / 2,
        "{:?} against {clear:?}",
        img.pixel(1, 1)
    );
    w.remove(cam, w.component_id("PostVignette").unwrap())
        .unwrap();

    // Palette: red becomes one of the four greens.
    w.insert(cam, PostPalette::default()).unwrap();
    let img = shot(&mut r, &w, "palette");
    let p = img.pixel(160, 90);
    assert!(p[1] > p[0] && p[1] > p[2], "green: {p:?}");
    w.remove(cam, w.component_id("PostPalette").unwrap())
        .unwrap();

    // Aberration: toward the edge, red and blue part at the rect's side.
    shape(&mut w, 12.0, 0.0, "rect", 4.0, "#FFFFFF", 0.0);
    w.insert(
        cam,
        PostAberration {
            amount: 0.03,
            ..Default::default()
        },
    )
    .unwrap();
    let img = shot(&mut r, &w, "aberration");
    let (x, _) = r.to_pixel(10.0, 0.0);
    let edge = (x as u32 - 4..x as u32 + 4)
        .map(|x| img.pixel(x, 90))
        .find(|p| p[0] != p[2]);
    assert!(edge.is_some(), "a fringe where white meets the ground");

    // Five at once, the ones §17 times.
    w.remove(cam, w.component_id("PostAberration").unwrap())
        .unwrap();
    w.insert(
        cam,
        PostShake {
            offset: argus_ecs::Vec2::new(0.01, 0.0),
            ..Default::default()
        },
    )
    .unwrap();
    w.insert(
        cam,
        PostFlash {
            amount: 0.2,
            ..Default::default()
        },
    )
    .unwrap();
    w.insert(cam, PostAberration::default()).unwrap();
    w.insert(cam, PostVignette::default()).unwrap();
    w.insert(
        cam,
        PostPalette {
            amount: 0.3,
            ..Default::default()
        },
    )
    .unwrap();
    let rep = r.render(&w, 0, 1.0).unwrap();
    assert!(rep.stats.sprites > 0);
    let img = r.capture().unwrap();
    if let Ok(dir) = std::env::var("ARGUS_SHOTS") {
        img.save_png(&std::path::Path::new(&dir).join("post-five.png"))
            .unwrap();
    }
}

/// §17: Hollow's post stack, five passes (flash, shake, aberration,
/// vignette, palette) at 1080p, under 1.5 ms of GPU time. Measured as the
/// difference between frames with the five and frames without, each frame
/// waited for, so it includes the passes' CPU side too.
#[test]
#[ignore]
fn perf_post_stack_five_passes_at_1080p() {
    use argus_post::*;
    let Some(mut r) = renderer(1920, 1080) else {
        return;
    };
    let mut w = world_with_camera(12.0);
    argus_post::register(&mut w).unwrap();
    for i in 0..300 {
        shape(
            &mut w,
            (i % 30) as f32 - 15.0,
            (i / 30) as f32 - 5.0,
            "rect",
            0.8,
            "#8A6A40",
            0.0,
        );
    }
    let cam = w.entity("camera").unwrap();
    let time = |r: &mut argus_render::Renderer, w: &argus_ecs::World| {
        for _ in 0..20 {
            r.render(w, 0, 1.0).unwrap();
            r.gpu().wait();
        }
        let n = 200;
        let t = std::time::Instant::now();
        for _ in 0..n {
            r.render(w, 0, 1.0).unwrap();
            r.gpu().wait();
        }
        t.elapsed().as_secs_f64() * 1e3 / n as f64
    };
    let without = time(&mut r, &w);
    w.insert(
        cam,
        PostShake {
            offset: argus_ecs::Vec2::new(0.01, 0.005),
            ..Default::default()
        },
    )
    .unwrap();
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
    let with = time(&mut r, &w);
    let cost = with - without;
    println!(
        "1920x1080 on {}: {without:.3} ms a frame without, {with:.3} ms with five passes: {cost:.3} ms",
        r.gpu().adapter
    );
    assert!(cost < 1.5, "five passes took {cost:.3} ms");
}
