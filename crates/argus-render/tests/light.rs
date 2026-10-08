//! Phase 5 item 3 (§12): "2D lighting as a render pass: point and cone
//! lights, occluders derived from tiles and sprites ...; off by default and
//! zero-cost when off." A grey floor, lit: by nothing (the ambient), by a
//! point light (bright by it, dark far off), past a box (its shadow), by a
//! cone (only where it points); a sprite that glows keeps its colour in the
//! dark. Without Lighting on the camera the frame is as it always was and
//! nothing of the pass is made.

use argus_ecs::{Transform, Vec2, World};
use argus_light::{Emissive, Light, Lighting, Occluder};
use argus_render::{Camera, Renderer, Sprite};

const W: u32 = 320;
const H: u32 = 180;

fn world() -> World {
    let mut w = World::new();
    w.register::<Transform>().unwrap();
    argus_render::register(&mut w).unwrap();
    argus_light::register(&mut w).unwrap();
    let cam = w.spawn_authored("camera").unwrap();
    w.insert(cam, Transform::at(0.0, 0.0)).unwrap();
    w.insert(
        cam,
        Camera {
            height: 10.0,
            ..Default::default()
        },
    )
    .unwrap();
    // The floor: grey across the whole view.
    let floor = w.spawn();
    w.insert(floor, Transform::at(0.0, 0.0)).unwrap();
    let s = Sprite {
        kind: w.strings().intern("rect"),
        size: Vec2::new(40.0, 20.0),
        color: w.strings().intern("#808080"),
        z: -1.0,
        ..Default::default()
    };
    w.insert(floor, s).unwrap();
    w
}

fn camera(w: &World) -> argus_ecs::Entity {
    w.entity("@camera").unwrap()
}

fn light_on(w: &mut World, ambient: &str) {
    let cam = camera(w);
    let l = Lighting {
        ambient: w.strings().intern(ambient),
        ..Default::default()
    };
    w.insert(cam, l).unwrap();
}

fn lamp(w: &mut World, x: f32, y: f32, l: Light) -> argus_ecs::Entity {
    let e = w.spawn();
    w.insert(e, Transform::at(x, y)).unwrap();
    w.insert(e, l).unwrap();
    e
}

/// The pixel at a world point (y up), as drawn.
fn at(img: &argus_render::Image, x: f32, y: f32) -> [u8; 3] {
    let half_x = 5.0 * W as f32 / H as f32;
    let px = ((x / half_x * 0.5 + 0.5) * W as f32) as u32;
    let py = ((0.5 - y / 5.0 * 0.5) * H as f32) as u32;
    let p = img.pixel(px.min(W - 1), py.min(H - 1));
    [p[0], p[1], p[2]]
}

fn bright(c: [u8; 3]) -> u32 {
    c.iter().map(|&x| x as u32).sum()
}

fn renderers() -> Vec<Renderer> {
    [false, true]
        .into_iter()
        .filter_map(|software| Renderer::offscreen(W, H, software).ok())
        .collect()
}

#[test]
fn without_lighting_nothing_changes_and_nothing_is_made() {
    for mut r in renderers() {
        let who = r.gpu().adapter.clone();
        let mut w = world();
        r.render(&w, 0, 1.0).unwrap();
        let plain = r.capture().unwrap();
        // A light with no Lighting on the camera: nothing of it.
        lamp(&mut w, 0.0, 0.0, Light::default());
        let rep = r.render(&w, 0, 1.0).unwrap();
        assert_eq!(rep.lights, 0, "{who}");
        assert_eq!(r.capture().unwrap().rgba, plain.rgba, "{who}");
        assert_eq!(r.gpu().light_format(), None, "{who}: no light map made");
    }
}

#[test]
fn a_point_light_lights_near_it_and_the_ambient_elsewhere() {
    for mut r in renderers() {
        let who = r.gpu().adapter.clone();
        let mut w = world();
        light_on(&mut w, "#101010");
        r.render(&w, 0, 1.0).unwrap();
        let dark = r.capture().unwrap();
        let ambient = at(&dark, 0.0, 0.0);
        assert!(bright(ambient) < 60, "{who}: the ambient alone {ambient:?}");
        lamp(
            &mut w,
            -4.0,
            0.0,
            Light {
                radius: 4.0,
                ..Default::default()
            },
        );
        let rep = r.render(&w, 0, 1.0).unwrap();
        assert_eq!(rep.lights, 1, "{who}");
        let img = r.capture().unwrap();
        let (near, half, far) = (at(&img, -4.0, 0.0), at(&img, -2.0, 0.0), at(&img, 3.0, 0.0));
        // A white light of 1 shows the floor's own grey at its centre.
        assert!(bright(near) > 3 * 120, "{who}: at the light {near:?}");
        assert!(
            bright(half) > bright(far) && bright(half) < bright(near),
            "{who}: fading {near:?} {half:?} {far:?}"
        );
        assert_eq!(far, ambient, "{who}: past its radius, the ambient");
    }
}

#[test]
fn a_box_casts_a_shadow_and_is_lit_on_the_light_side() {
    for mut r in renderers() {
        let who = r.gpu().adapter.clone();
        let mut w = world();
        light_on(&mut w, "#000000");
        let l = lamp(
            &mut w,
            -5.0,
            0.0,
            Light {
                radius: 12.0,
                falloff: 1.0,
                ..Default::default()
            },
        );
        let wall = w.spawn();
        w.insert(wall, Transform::at(-2.0, 0.0)).unwrap();
        w.insert(
            wall,
            Occluder {
                size: Vec2::new(1.0, 2.0),
                ..Default::default()
            },
        )
        .unwrap();
        let rep = r.render(&w, 0, 1.0).unwrap();
        assert_eq!(rep.shadow_edges, 4, "{who}");
        let img = r.capture().unwrap();
        let (behind, beside, front) = (at(&img, 1.0, 0.0), at(&img, 1.0, 3.5), at(&img, -2.4, 0.0));
        assert!(bright(behind) < 20, "{who}: in its shadow {behind:?}");
        assert!(bright(beside) > 60, "{who}: past its edge {beside:?}");
        assert!(bright(front) > 60, "{who}: its own lit face {front:?}");
        // A light that casts none shines through.
        w.update::<Light, _>(l, |l| l.shadows = false).unwrap();
        r.render(&w, 0, 1.0).unwrap();
        let through = at(&r.capture().unwrap(), 1.0, 0.0);
        assert!(bright(through) > 60, "{who}: no shadows {through:?}");
    }
}

#[test]
fn a_cone_lights_where_it_points() {
    for mut r in renderers() {
        let who = r.gpu().adapter.clone();
        let mut w = world();
        light_on(&mut w, "#000000");
        lamp(
            &mut w,
            0.0,
            0.0,
            Light {
                radius: 6.0,
                falloff: 1.0,
                angle: 25.0,
                direction: 0.0,
                ..Default::default()
            },
        );
        r.render(&w, 0, 1.0).unwrap();
        let img = r.capture().unwrap();
        let (ahead, behind, aside) = (at(&img, 2.0, 0.0), at(&img, -2.0, 0.0), at(&img, 0.5, 2.0));
        assert!(bright(ahead) > 100, "{who}: ahead {ahead:?}");
        assert!(bright(behind) < 10, "{who}: behind {behind:?}");
        assert!(bright(aside) < 10, "{who}: aside {aside:?}");
    }
}

#[test]
fn what_glows_keeps_its_colour_in_the_dark() {
    for mut r in renderers() {
        let who = r.gpu().adapter.clone();
        let mut w = world();
        light_on(&mut w, "#000000");
        let gem = w.spawn();
        w.insert(gem, Transform::at(2.0, 0.0)).unwrap();
        let s = Sprite {
            kind: w.strings().intern("rect"),
            size: Vec2::new(1.0, 1.0),
            color: w.strings().intern("#20E040"),
            ..Default::default()
        };
        w.insert(gem, s).unwrap();
        r.render(&w, 0, 1.0).unwrap();
        assert!(
            bright(at(&r.capture().unwrap(), 2.0, 0.0)) < 10,
            "{who}: unlit, dark"
        );
        w.insert(gem, Emissive::default()).unwrap();
        let rep = r.render(&w, 0, 1.0).unwrap();
        assert_eq!(rep.glowing, 1, "{who}");
        let c = at(&r.capture().unwrap(), 2.0, 0.0);
        assert!(c[1] > 200 && c[0] < 80, "{who}: its own green {c:?}");
    }
}

#[test]
fn the_byte_light_map_lights_as_the_half_float_one() {
    for mut r in renderers() {
        let who = r.gpu().adapter.clone();
        let mut w = world();
        light_on(&mut w, "#202020");
        lamp(&mut w, 0.0, 0.0, Light::default());
        r.render(&w, 0, 1.0).unwrap();
        let first = r.capture().unwrap();
        let fmt = r.gpu().light_format();
        r.gpu_mut().set_light_f16(false);
        r.render(&w, 0, 1.0).unwrap();
        assert_eq!(
            r.gpu().light_format(),
            Some(wgpu::TextureFormat::Rgba8UnormSrgb),
            "{who}"
        );
        let bytes = r.capture().unwrap();
        let c = bytes.compare(&first, 0.1).unwrap();
        assert!(
            c.fraction() < 0.001,
            "{who} ({fmt:?} against bytes): {} of {} pixels differ",
            c.differing,
            c.pixels
        );
    }
}

#[test]
fn a_normal_mapped_sprite_is_lit_by_where_its_normals_face() {
    for mut r in renderers() {
        let who = r.gpu().adapter.clone();
        let mut w = world();
        light_on(&mut w, "#000000");
        // A white square whose every pixel leans toward +x.
        r.gpu_mut().add_atlas_rgba(
            "slope",
            8,
            8,
            &[255u8; 8 * 8 * 4],
            vec![[0.0, 0.0, 1.0, 1.0]],
        );
        let e = w.spawn();
        w.insert(e, Transform::at(0.0, 0.0)).unwrap();
        let s = Sprite {
            atlas: w.strings().intern("slope"),
            size: Vec2::new(2.0, 2.0),
            ..Default::default()
        };
        w.insert(e, s).unwrap();
        // A low light to its right.
        lamp(
            &mut w,
            3.0,
            0.0,
            Light {
                radius: 10.0,
                falloff: 1.0,
                height: 1.0,
                ..Default::default()
            },
        );
        let shot = |r: &mut Renderer, w: &World| {
            r.render(w, 0, 1.0).unwrap();
            bright(at(&r.capture().unwrap(), 0.0, 0.0))
        };
        let flat = shot(&mut r, &w);
        let normal = [230u8, 128, 204, 255].repeat(64);
        assert!(
            r.gpu_mut().set_atlas_normal("slope", 8, 8, &normal),
            "{who}"
        );
        assert!(r.gpu().atlas("slope").unwrap().normal_mapped());
        let toward = shot(&mut r, &w);
        assert!(
            toward > flat + 30,
            "{who}: facing the light {toward} against flat {flat}"
        );
        // Mirrored, its normals face away.
        w.update::<Sprite, _>(e, |s| s.flip_x = true).unwrap();
        let away = shot(&mut r, &w);
        assert!(
            away + 100 < flat,
            "{who}: facing away {away} against flat {flat}"
        );
        // The floor around it, with no normal map, is lit as before.
        let floor = |r: &Renderer| bright(at(&r.capture().unwrap(), 0.0, 3.0));
        let lit_floor = floor(&r);
        r.gpu_mut().add_atlas_rgba(
            "slope",
            8,
            8,
            &[255u8; 8 * 8 * 4],
            vec![[0.0, 0.0, 1.0, 1.0]],
        );
        shot(&mut r, &w);
        assert_eq!(floor(&r), lit_floor, "{who}: the floor, flat either way");
    }
}

/// Past 255 shadow-casting lights the stencil's numbers run out and a new
/// pass begins: the light after them still lights, and still has its
/// shadow.
#[test]
fn the_three_hundredth_shadowed_light_still_lights_and_shadows() {
    for mut r in renderers() {
        let who = r.gpu().adapter.clone();
        let mut w = world();
        light_on(&mut w, "#000000");
        // 299 dim lights far up, out of the way, each casting a shadow.
        let wall = w.spawn();
        w.insert(wall, Transform::at(0.0, 4.0)).unwrap();
        w.insert(
            wall,
            Occluder {
                size: Vec2::new(0.2, 0.2),
                ..Default::default()
            },
        )
        .unwrap();
        for i in 0..299 {
            lamp(
                &mut w,
                -8.0 + (i % 30) as f32 * 0.5,
                4.5,
                Light {
                    radius: 1.0,
                    intensity: 0.01,
                    ..Default::default()
                },
            );
        }
        // The three hundredth, lighting the floor past a box.
        lamp(
            &mut w,
            -5.0,
            -2.0,
            Light {
                radius: 12.0,
                falloff: 1.0,
                ..Default::default()
            },
        );
        let post = w.spawn();
        w.insert(post, Transform::at(-2.0, -2.0)).unwrap();
        w.insert(
            post,
            Occluder {
                size: Vec2::new(1.0, 1.0),
                ..Default::default()
            },
        )
        .unwrap();
        let rep = r.render(&w, 0, 1.0).unwrap();
        assert_eq!(rep.lights, 300, "{who}");
        let img = r.capture().unwrap();
        let (lit, shadowed) = (at(&img, 1.0, -0.5), at(&img, 1.0, -2.0));
        assert!(bright(lit) > 60, "{who}: lit {lit:?}");
        assert!(bright(shadowed) < 20, "{who}: shadowed {shadowed:?}");
    }
}
