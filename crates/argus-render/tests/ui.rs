//! The UI drawn (phase 3 item 1): over the world and its post passes, on
//! the canvas scaled to fit the target, clipped where a node clips.

use argus_camera::Camera;
use argus_ecs::{Parent, Transform, Vec2, World};
use argus_render::Renderer;
use argus_sim::{Scheduler, SchedulerConfig};
use argus_ui::{Shared, UiNode, UiPanel, UiText};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

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

/// An empty `argus-<name>-<pid>` in the temp directory.
fn temp(name: &str) -> Temp {
    let dir = std::env::temp_dir().join(format!("argus-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Temp(dir)
}

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

fn world() -> (World, Scheduler, Rc<RefCell<Shared>>) {
    let mut w = World::new();
    w.register::<Transform>().unwrap();
    argus_render::register(&mut w).unwrap();
    argus_post::register(&mut w).unwrap();
    let cam = w.spawn_authored("camera").unwrap();
    w.insert(cam, Transform::at(0.0, 0.0)).unwrap();
    w.insert(
        cam,
        Camera {
            height: 18.0,
            ..Default::default()
        },
    )
    .unwrap();
    let mut s = Scheduler::new(SchedulerConfig::default());
    let shared = Rc::new(RefCell::new(Shared::default()));
    argus_ui::install(&mut w, &mut s, shared.clone()).unwrap();
    (w, s, shared)
}

fn node(w: &mut World, parent: Option<argus_ecs::Entity>, n: UiNode) -> argus_ecs::Entity {
    let e = w.spawn();
    w.insert(e, n).unwrap();
    if let Some(p) = parent {
        w.insert(e, Parent { entity: p }).unwrap();
    }
    e
}

fn shot(r: &mut Renderer, w: &World, name: &str) -> argus_render::Image {
    r.render(w, 0, 1.0).unwrap();
    let img = r.capture().unwrap();
    if let Ok(dir) = std::env::var("ARGUS_SHOTS") {
        img.save_png(&std::path::Path::new(&dir).join(format!("ui-{name}.png")))
            .unwrap();
    }
    img
}

#[test]
fn a_panel_and_its_words_draw_over_the_world_and_its_flash_scaled_to_fit() {
    let Some(mut r) = renderer(960, 540) else {
        return;
    };
    let (mut w, mut s, shared) = world();
    r.set_ui(Some(shared.clone()));
    let top = node(&mut w, None, UiNode::default());
    let panel = node(
        &mut w,
        Some(top),
        UiNode {
            absolute: true,
            size: Vec2::new(200.0, 100.0),
            offset: Vec2::new(100.0, 50.0),
            pad: Vec2::new(10.0, 10.0),
            ..UiNode::default()
        },
    );
    let (red, blue, white) = (
        w.strings().intern("#FF0000"),
        w.strings().intern("#0000FF"),
        w.strings().intern("#FFFFFF"),
    );
    w.insert(
        panel,
        UiPanel {
            color: red,
            border: 4.0,
            border_color: blue,
            ..UiPanel::default()
        },
    )
    .unwrap();
    let label = node(&mut w, Some(panel), UiNode::default());
    let ok = w.strings().intern("OK OK OK");
    w.insert(
        label,
        UiText {
            text: ok,
            size: 16.0,
            color: white,
            ..UiText::default()
        },
    )
    .unwrap();
    // A node that clips: its child's red spills out, and is cut off.
    let clipper = node(
        &mut w,
        Some(top),
        UiNode {
            absolute: true,
            size: Vec2::new(40.0, 40.0),
            offset: Vec2::new(600.0, 300.0),
            clip: true,
            ..UiNode::default()
        },
    );
    let spill = node(
        &mut w,
        Some(clipper),
        UiNode {
            size: Vec2::new(120.0, 120.0),
            ..UiNode::default()
        },
    );
    w.insert(
        spill,
        UiPanel {
            color: red,
            ..UiPanel::default()
        },
    )
    .unwrap();
    s.run_load(&mut w).unwrap();

    let img = shot(&mut r, &w, "panel");
    assert_eq!(img.pixel(200, 100)[..3], [255, 0, 0], "the panel");
    assert_eq!(img.pixel(101, 100)[..3], [0, 0, 255], "its border");
    let words = (110..190)
        .flat_map(|x| (60..80).map(move |y| (x, y)))
        .filter(|&(x, y)| img.pixel(x, y)[..3] == [255, 255, 255])
        .count();
    assert!(words > 50, "white words on it: {words} pixels");
    assert_eq!(img.pixel(620, 320)[..3], [255, 0, 0], "inside the clip");
    assert_ne!(img.pixel(680, 380)[..3], [255, 0, 0], "outside it, cut off");

    // A full flash whitens the world, not the UI.
    let cam = w.entity("camera").unwrap();
    w.insert(
        cam,
        argus_post::PostFlash {
            amount: 1.0,
            ..Default::default()
        },
    )
    .unwrap();
    let img = shot(&mut r, &w, "flash");
    assert_eq!(img.pixel(5, 5)[..3], [255, 255, 255], "the world flashed");
    assert_eq!(img.pixel(200, 100)[..3], [255, 0, 0], "the panel did not");

    // Half the size: the canvas scales to fit.
    let Some(mut small) = renderer(480, 270) else {
        return;
    };
    small.set_ui(Some(shared));
    let img = shot(&mut small, &w, "half");
    assert_eq!(img.pixel(100, 50)[..3], [255, 0, 0]);
    assert_ne!(img.pixel(45, 20)[..3], [255, 0, 0]);
}

#[test]
fn a_font_asset_draws_crisp_from_its_distance_field_at_any_size() {
    let Some(mut r) = renderer(960, 540) else {
        return;
    };
    // Delve's font, imported as a project does, its atlas loaded.
    let dir = temp("render-font");
    std::fs::create_dir_all(dir.join("assets/fonts")).unwrap();
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/delve/assets/fonts/LiberationSans-Regular.ttf"),
        dir.join("assets/fonts/sans.ttf"),
    )
    .unwrap();
    let mut imp = argus_assets::Importer::new(&dir);
    imp.scan();
    let loaded = r.load_catalog(&imp).unwrap();
    assert!(loaded.contains(&"fonts/sans".to_string()), "{loaded:?}");
    assert!(r.gpu().material_index("sdf_text").is_some(), "the built-in");
    let (_, f) = imp.fonts().next().unwrap();
    let (mut w, mut s, shared) = world();
    shared.borrow_mut().fonts.add(
        "fonts/sans",
        Rc::new(argus_ui::SdfFont::from_asset("fonts/sans", f)),
    );
    r.set_ui(Some(shared.clone()));
    let top = node(&mut w, None, UiNode::default());
    let (sans, white) = (
        w.strings().intern("fonts/sans"),
        w.strings().intern("#FFFFFF"),
    );
    let label = |w: &mut World, text: &str, size: f32, at: (f32, f32)| {
        let e = node(
            w,
            Some(top),
            UiNode {
                absolute: true,
                offset: Vec2::new(at.0, at.1),
                ..UiNode::default()
            },
        );
        let t = w.strings().intern(text);
        w.insert(
            e,
            UiText {
                text: t,
                size,
                font: sans,
                color: white,
                wrap: false,
                ..UiText::default()
            },
        )
        .unwrap();
        e
    };
    let big = label(&mut w, "H", 160.0, (100.0, 60.0));
    label(&mut w, "Small words, kerned: AVATAR", 14.0, (400.0, 100.0));
    s.run_load(&mut w).unwrap();
    let img = shot(&mut r, &w, "sdf");

    // The big H: solid inside, a pixel of blend at its edges, and nothing
    // stretched from 32 pixels an em.
    let rect = w.get::<argus_ui::UiRect>(big).unwrap();
    let bg = img.pixel(5, 5)[0];
    let (mut solid, mut edge) = (0, 0);
    for y in rect.pos.y as u32..(rect.pos.y + rect.size.y) as u32 {
        for x in rect.pos.x as u32..(rect.pos.x + rect.size.x) as u32 {
            match img.pixel(x, y)[0] {
                235.. => solid += 1,
                v if v > bg.saturating_add(12) => edge += 1,
                _ => {}
            }
        }
    }
    println!("H: {solid} solid, {edge} edge pixels");
    assert!(solid > 2500, "{solid}");
    assert!(edge * 5 < solid, "a crisp edge: {edge} blended of {solid}");
    // The small words are there too.
    let words = (400..700)
        .flat_map(|x| (100..125).map(move |y| (x, y)))
        .filter(|&(x, y)| img.pixel(x, y)[0] > 160)
        .count();
    assert!(words > 150, "small words: {words} pixels");
}

#[test]
fn a_stem_thinner_than_a_pixel_keeps_its_ink_wherever_it_falls() {
    // Outpost's HUD in a 640 x 360 frame (phase 5 item 3's look at the lit
    // colony): its 15-pixel Liberation Sans at two thirds, where a stem is
    // under a pixel wide. "colonists" lost the middle of its t, which read
    // as two letters drawn over each other: the two pixels either side of
    // the stem both read the field just under the edge, the field rising
    // and falling again between them, so the edge's blend, measured as the
    // field's change from one pixel to the next, closed to nothing. An l is
    // a stem alone: drawn a tenth of a pixel further along each time, every
    // one keeps its ink all down its height.
    let dir = std::env::temp_dir().join(format!("argus-render-stem-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets/fonts")).unwrap();
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/outpost/assets/fonts/LiberationSans-Regular.ttf"),
        dir.join("assets/fonts/sans.ttf"),
    )
    .unwrap();
    let mut imp = argus_assets::Importer::new(&dir);
    imp.scan();
    let (_, f) = imp.fonts().next().unwrap();
    let font = Rc::new(argus_ui::SdfFont::from_asset("fonts/sans", f));
    // Sizes whose l is a stem under a pixel wide at two thirds; where its
    // ink is in its cell, between the field's margins.
    let sizes = [15.0f32, 13.0, 12.0, 11.0];
    let ink = |size: f32| {
        let g = argus_ui::FontFace::glyphs(font.as_ref(), "l", size)[0].clone();
        let margin = (f.spread.ceil() + 1.0) * size / f.size;
        (g.y + margin, g.y + g.h - margin)
    };
    let (w_px, h_px) = (640u32, 360u32);
    let k = w_px as f32 / 960.0;
    let rs: Vec<Renderer> = [false, true]
        .into_iter()
        .filter_map(|software| Renderer::offscreen(w_px, h_px, software).ok())
        .collect();
    for mut r in rs {
        let who = r.gpu().adapter.clone();
        r.load_catalog(&imp).unwrap();
        let (mut w, mut s, shared) = world();
        shared.borrow_mut().fonts.add("fonts/sans", font.clone());
        r.set_ui(Some(shared.clone()));
        let top = node(&mut w, None, UiNode::default());
        let (sans, white) = (
            w.strings().intern("fonts/sans"),
            w.strings().intern("#FFFFFF"),
        );
        let l = w.strings().intern("l");
        // A row of ls a size, each a tenth of a target pixel further along.
        let mut labels = Vec::new();
        for (j, &size) in sizes.iter().enumerate() {
            for i in 0..10 {
                let e = node(
                    &mut w,
                    Some(top),
                    UiNode {
                        absolute: true,
                        offset: Vec2::new(
                            60.0 + 80.0 * i as f32 + 0.15 * i as f32,
                            60.0 + 60.0 * j as f32,
                        ),
                        ..UiNode::default()
                    },
                );
                w.insert(
                    e,
                    UiText {
                        text: l,
                        size,
                        font: sans,
                        color: white,
                        wrap: false,
                        ..UiText::default()
                    },
                )
                .unwrap();
                labels.push((size, i, e));
            }
        }
        s.run_load(&mut w).unwrap();
        let img = shot(&mut r, &w, "stem");
        let bg = img.pixel(5, 5)[0];
        let mut lost = Vec::new();
        for &(size, i, e) in &labels {
            let rect = w.get::<argus_ui::UiRect>(e).unwrap();
            let cols =
                (rect.pos.x * k).floor() as u32..((rect.pos.x + rect.size.x) * k).ceil() as u32;
            // The middle half of its height, in the target's pixels.
            let (a, b) = (rect.pos.y + ink(size).0, rect.pos.y + ink(size).1);
            let rows = ((a + 0.25 * (b - a)) * k) as u32..((a + 0.75 * (b - a)) * k).ceil() as u32;
            let brightest: Vec<u8> = rows
                .clone()
                .map(|y| cols.clone().map(|x| img.pixel(x, y)[0]).max().unwrap_or(0))
                .collect();
            println!("{who}: size {size} l {i} rows {rows:?} brightest {brightest:?}");
            if !brightest.iter().all(|&v| v > bg.saturating_add(60)) {
                lost.push(format!("size {size}, {i} tenths along: {brightest:?}"));
            }
        }
        assert!(
            lost.is_empty(),
            "{who}: ls that lost their stems (the brightest pixel down the middle rows, over {bg}):\n{}",
            lost.join("\n")
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A panel can be one frame of a sheet (the art passes cut each panel into
/// a file of its own): frame 1 of two; past the last, the first.
#[test]
fn a_panel_draws_one_frame_of_its_image() {
    let Some(mut r) = renderer(960, 540) else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("argus-panel-frame-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    // Two frames by grid: red, then blue.
    argus_render::Image {
        width: 2,
        height: 1,
        rgba: vec![255, 0, 0, 255, 0, 0, 255, 255],
    }
    .save_png(&dir.join("assets").join("plates.png"))
    .unwrap();
    std::fs::write(
        dir.join("assets").join("plates.atlas.toml"),
        "grid = [2, 1]\n",
    )
    .unwrap();
    r.load_atlases(&dir).unwrap();
    let (mut w, mut s, shared) = world();
    r.set_ui(Some(shared.clone()));
    let top = node(&mut w, None, UiNode::default());
    let panel = node(
        &mut w,
        Some(top),
        UiNode {
            absolute: true,
            size: Vec2::new(200.0, 100.0),
            offset: Vec2::new(100.0, 50.0),
            ..UiNode::default()
        },
    );
    let image = w.strings().intern("plates");
    for (frame, want) in [(0, [255, 0, 0]), (1, [0, 0, 255]), (7, [255, 0, 0])] {
        w.insert(
            panel,
            UiPanel {
                image,
                frame,
                ..UiPanel::default()
            },
        )
        .unwrap();
        s.run_load(&mut w).unwrap();
        let img = shot(&mut r, &w, &format!("panel-frame-{frame}"));
        assert_eq!(img.pixel(200, 100)[..3], want, "frame {frame}");
    }
    std::fs::remove_dir_all(&dir).ok();
}
