//! §13 Materials and shaders: sprites reference a material, a WGSL shader
//! plus typed parameters; the shipped library draws; materials reload in
//! place.

use argus_ecs::{Entity, FieldType, SchemaBuilder, Transform, Value, Vec2, World};
use argus_material::{parse_material, MaterialSpec};
use argus_render::{Camera, Renderer, Sprite};

fn renderer() -> Option<Renderer> {
    match Renderer::offscreen(100, 100, false) {
        Ok(r) => Some(r),
        // Only a machine with no adapter at all skips; anything else is a bug.
        Err(e) if e.code == argus_render::codes::NO_ADAPTER => {
            eprintln!("skipping: {e}");
            None
        }
        Err(e) => panic!("the renderer did not start: {e}"),
    }
}

/// A camera 10 units tall over 100 px: 10 px a unit, the origin centred.
fn world() -> World {
    let mut w = World::new();
    w.register::<Transform>().unwrap();
    argus_render::register(&mut w).unwrap();
    let paint = SchemaBuilder::new("Paint")
        .field("color", FieldType::Str)
        .field("glow", FieldType::F32)
        .build()
        .unwrap();
    w.register_schema(paint).unwrap();
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
    w
}

#[allow(clippy::too_many_arguments)]
fn sprite(
    w: &mut World,
    x: f32,
    y: f32,
    kind: &str,
    size: f32,
    color: &str,
    material: &str,
    z: f32,
) -> Entity {
    let e = w.spawn();
    w.insert(e, Transform::at(x, y)).unwrap();
    let s = Sprite {
        kind: w.strings().intern(kind),
        size: Vec2::new(size, size),
        color: w.strings().intern(color),
        material: w.strings().intern(material),
        z,
        ..Default::default()
    };
    w.insert(e, s).unwrap();
    e
}

fn spec(name: &str, toml: Option<&str>, wgsl: Option<&str>) -> MaterialSpec {
    let (s, _) = parse_material(
        name,
        toml.map(|t| ("m.toml", t)),
        wgsl.map(|t| ("m.wgsl", t)),
        &mut |p: &str| Err(argus_ecs::ArgusError::new(1, p.to_string())),
    )
    .unwrap();
    argus_material::check(&s).unwrap_or_else(|e| panic!("{}", e.message));
    s
}

fn load(r: &mut Renderer, mats: &[(&str, &MaterialSpec)]) {
    let mut all: Vec<(&str, &MaterialSpec)> = argus_material::builtins()
        .iter()
        .map(|b| (b.name.as_str(), b))
        .collect();
    all.extend_from_slice(mats);
    let errs = r.set_materials(all, |_| false);
    assert!(errs.is_empty(), "{errs:?}");
}

fn px(r: &Renderer, x: f32, y: f32) -> [u8; 3] {
    let img = r.capture().unwrap();
    let (a, b) = r.to_pixel(x, y);
    let p = img.pixel(a as u32, b as u32);
    [p[0], p[1], p[2]]
}

const PAINT: &str = "fn material(in: MaterialInput, p: Params) -> vec4<f32> {\n    let c = sample_atlas(in, in.uv);\n    return vec4<f32>(p.paint.rgb * (1.0 + p.glow), c.a);\n}\n";
const PAINT_TOML: &str = "[params.paint]\ntype = \"color\"\ndefault = \"#0000FF\"\nfrom = \"Paint.color\"\n\n[params.glow]\ntype = \"f32\"\ndefault = 0\n";

/// A material's parameters: a default for every sprite, and a field of each
/// sprite's own entity when it has one. Materials split the draw into runs.
#[test]
fn a_material_draws_its_parameters_from_each_sprites_fields() {
    let Some(mut r) = renderer() else { return };
    let mut w = world();
    let paint = spec("materials/paint", Some(PAINT_TOML), Some(PAINT));
    load(&mut r, &[("materials/paint", &paint)]);
    let a = sprite(&mut w, -3.0, 0.0, "rect", 2.0, "", "materials/paint", 0.0);
    let b = sprite(&mut w, 0.0, 0.0, "rect", 2.0, "", "materials/paint", 0.0);
    sprite(&mut w, 3.0, 0.0, "rect", 2.0, "#FFFFFF", "", 0.0);
    let pid = w.component_id("Paint").unwrap();
    w.insert_default(b, pid).unwrap();
    w.set_field(b, "Paint.color", "#00FF00").unwrap();
    let rep = r.render(&w, 0, 1.0).unwrap();
    assert!(
        rep.missing_materials.is_empty() && rep.material_problems.is_empty(),
        "{rep:?}"
    );
    assert_eq!(px(&r, -3.0, 0.0), [0, 0, 255], "no Paint: the default");
    assert_eq!(px(&r, 0.0, 0.0), [0, 255, 0], "its own Paint.color");
    assert_eq!(
        px(&r, 3.0, 0.0),
        [255, 255, 255],
        "no material: the plain sprite"
    );
    assert_eq!(rep.stats.materials, 2);
    assert_eq!(rep.stats.draw_calls, 2, "one run a material");
    // An empty colour keeps the default; a changed field shows next frame.
    w.insert_default(a, pid).unwrap();
    r.render(&w, 0, 1.0).unwrap();
    assert_eq!(px(&r, -3.0, 0.0), [0, 0, 255]);
    w.set_field(a, "Paint.color", "#FF0000").unwrap();
    r.render(&w, 0, 1.0).unwrap();
    assert_eq!(px(&r, -3.0, 0.0), [255, 0, 0]);
    // A material nobody loaded draws plain and is reported.
    sprite(
        &mut w,
        0.0,
        3.0,
        "rect",
        1.0,
        "#FFFFFF",
        "materials/nope",
        0.0,
    );
    let rep = r.render(&w, 0, 1.0).unwrap();
    assert_eq!(rep.missing_materials, ["materials/nope"]);
    assert_eq!(px(&r, 0.0, 3.0), [255, 255, 255]);
}

/// Blend modes: add brightens what is under it, multiply darkens it.
#[test]
fn add_and_multiply_blend_into_what_is_drawn() {
    let Some(mut r) = renderer() else { return };
    let mut w = world();
    let add = spec(
        "fx/add",
        Some("shader = \"unlit\"\nblend = \"add\"\n"),
        None,
    );
    let mul = spec(
        "fx/mul",
        Some("shader = \"unlit\"\nblend = \"multiply\"\n"),
        None,
    );
    load(&mut r, &[("fx/add", &add), ("fx/mul", &mul)]);
    sprite(&mut w, -2.0, 0.0, "rect", 2.0, "#FF0000", "", 0.0);
    sprite(&mut w, -2.0, 0.0, "rect", 2.0, "#00FF00", "fx/add", 1.0);
    sprite(&mut w, 2.0, 0.0, "rect", 2.0, "#FFFF00", "", 0.0);
    sprite(&mut w, 2.0, 0.0, "rect", 2.0, "#00FF00", "fx/mul", 1.0);
    r.render(&w, 0, 1.0).unwrap();
    assert_eq!(px(&r, -2.0, 0.0), [255, 255, 0], "red plus green");
    assert_eq!(px(&r, 2.0, 0.0), [0, 255, 0], "yellow times green");
}

/// The library draws: dissolve, outline, palette, and lit, each driven by
/// its per-sprite component or its parameters.
#[test]
fn the_library_dissolves_outlines_recolours_and_lights() {
    let Some(mut r) = renderer() else { return };
    let mut w = world();
    // A palette: row 0 red then green, row 1 blue then white; art with a
    // black half and a white half.
    let red = [255u8, 0, 0, 255];
    let green = [0u8, 255, 0, 255];
    let blue = [0u8, 0, 255, 255];
    let white = [255u8, 255, 255, 255];
    let pal: Vec<u8> = [red, green, blue, white].concat();
    r.gpu_mut()
        .add_atlas_rgba("pal", 2, 2, &pal, vec![[0.0, 0.0, 1.0, 1.0]]);
    let art: Vec<u8> = [[0u8, 0, 0, 255], white].concat();
    r.gpu_mut()
        .add_atlas_rgba("art", 2, 1, &art, vec![[0.0, 0.0, 1.0, 1.0]]);
    let teams = spec(
        "materials/teams",
        Some("shader = \"palette\"\n\n[params.palette]\ndefault = \"pal\"\n"),
        None,
    );
    let lamp = spec("materials/lamp", Some("shader = \"lit\"\n\n[params.light]\nfrom = \"@lamp:Transform.pos\"\n\n[params.radius]\ndefault = 3\n"), None);
    load(
        &mut r,
        &[("materials/teams", &teams), ("materials/lamp", &lamp)],
    );

    let gone = sprite(&mut w, -3.0, 3.0, "rect", 2.0, "#FFFFFF", "dissolve", 0.0);
    let half = sprite(&mut w, 0.0, 3.0, "rect", 2.0, "#FFFFFF", "dissolve", 0.0);
    let ring = sprite(&mut w, 3.0, 3.0, "circle", 3.0, "#FFFFFF", "outline", 0.0);
    let t1 = w.spawn();
    w.insert(t1, Transform::at(-3.0, -3.0)).unwrap();
    let s = Sprite {
        atlas: w.strings().intern("art"),
        size: Vec2::new(2.0, 1.0),
        material: w.strings().intern("materials/teams"),
        ..Default::default()
    };
    w.insert(t1, s).unwrap();
    let t2 = w.spawn();
    w.insert(t2, Transform::at(0.0, -3.0)).unwrap();
    w.insert(t2, s).unwrap();
    w.insert(t2, argus_material::Palette { row: 1 }).unwrap();
    let lamp_e = w.spawn_authored("lamp").unwrap();
    w.insert(lamp_e, Transform::at(1.5, -3.0)).unwrap();
    sprite(
        &mut w,
        3.5,
        -3.0,
        "rect",
        3.0,
        "#FFFFFF",
        "materials/lamp",
        0.0,
    );

    w.insert(gone, argus_material::Dissolve { amount: 1.0 })
        .unwrap();
    w.insert(half, argus_material::Dissolve { amount: 0.5 })
        .unwrap();
    let orange = w.strings().intern("#FF8000");
    w.insert(ring, argus_material::Outline { color: orange })
        .unwrap();
    let rep = r.render(&w, 0, 1.0).unwrap();
    assert!(
        rep.material_problems.is_empty(),
        "{:?}",
        rep.material_problems
    );
    let img = r.capture().unwrap();
    let count = |x0: f32, y0: f32, x1: f32, y1: f32, rgb: [u8; 3]| {
        let (a, b) = r.to_pixel(x0, y1);
        let (c, d) = r.to_pixel(x1, y0);
        let mut n = 0;
        for y in b as u32..d as u32 {
            for x in a as u32..c as u32 {
                let p = img.pixel(x, y);
                if (0..3).all(|k| (p[k] as i32 - rgb[k] as i32).abs() <= 2) {
                    n += 1;
                }
            }
        }
        n
    };
    // Dissolve: all gone at 1; about half at 0.5, with an orange edge.
    assert_eq!(
        count(-4.0, 2.0, -2.0, 4.0, [255, 255, 255]),
        0,
        "dissolved away"
    );
    let left = count(-1.0, 2.0, 1.0, 4.0, [255, 255, 255]);
    assert!(left > 80 && left < 320, "about half of 400 px: {left}");
    // Outline: orange just outside the circle, white inside.
    assert_eq!(px(&r, 3.0, 3.0), [255, 255, 255]);
    assert!(
        count(1.5, 1.5, 4.5, 4.5, [255, 128, 0]) > 20,
        "an orange ring"
    );
    // Palette: black -> column 0, white -> column 1; row 0 and row 1.
    assert_eq!(px(&r, -3.5, -3.0), [255, 0, 0]);
    assert_eq!(px(&r, -2.5, -3.0), [0, 255, 0]);
    assert_eq!(px(&r, -0.5, -3.0), [0, 0, 255]);
    assert_eq!(px(&r, 0.5, -3.0), [255, 255, 255]);
    // Lit: brightest by the lamp, ambient beyond its radius.
    let near = px(&r, 2.2, -3.0);
    let far = px(&r, 4.9, -3.0);
    assert!(near[0] > far[0] + 60, "{near:?} vs {far:?}");
    // The light follows the lamp entity.
    w.set_field(lamp_e, "Transform.pos", Vec2::new(5.5, -3.0))
        .unwrap();
    r.render(&w, 0, 1.0).unwrap();
    let (near2, far2) = (px(&r, 2.2, -3.0), px(&r, 4.9, -3.0));
    assert!(far2[0] > near2[0] + 60, "{near2:?} vs {far2:?}");
}

/// Hot reload: a changed material compiles in place and the next frame
/// draws it; its index, and the sprites naming it, stay as they are.
#[test]
fn a_changed_material_reloads_in_place() {
    let Some(mut r) = renderer() else { return };
    let mut w = world();
    let red = spec("materials/flat", None, Some("fn material(in: MaterialInput, p: Params) -> vec4<f32> {\n    return vec4<f32>(1.0, 0.0, 0.0, 1.0);\n}\n"));
    load(&mut r, &[("materials/flat", &red)]);
    let idx = r.gpu().material_index("materials/flat").unwrap();
    sprite(&mut w, 0.0, 0.0, "rect", 4.0, "", "materials/flat", 0.0);
    r.render(&w, 0, 1.0).unwrap();
    assert_eq!(px(&r, 0.0, 0.0), [255, 0, 0]);
    let blue = spec("materials/flat", None, Some("fn material(in: MaterialInput, p: Params) -> vec4<f32> {\n    return vec4<f32>(0.0, 0.0, 1.0, 1.0);\n}\n"));
    let t = std::time::Instant::now();
    load(&mut r, &[("materials/flat", &blue)]);
    r.render(&w, 0, 1.0).unwrap();
    let ms = t.elapsed().as_secs_f64() * 1e3;
    assert_eq!(px(&r, 0.0, 0.0), [0, 0, 255]);
    assert_eq!(r.gpu().material_index("materials/flat"), Some(idx));
    println!(
        "reload and redraw: {ms:.1} ms (compile {:.1} ms)",
        r.material_compile_ms()
    );
    assert!(ms < 500.0, "{ms} ms");
    // Unchanged, nothing compiles again.
    let before = r.material_compile_ms();
    load(&mut r, &[("materials/flat", &blue)]);
    assert_eq!(r.material_compile_ms(), before);
    // Gone, it is dropped and the sprite draws plain.
    load(&mut r, &[]);
    let rep = r.render(&w, 0, 1.0).unwrap();
    assert_eq!(rep.missing_materials, ["materials/flat"]);
}

/// A parameter whose field is the wrong type is reported and the default
/// is drawn.
#[test]
fn a_parameter_from_the_wrong_kind_of_field_is_reported() {
    let Some(mut r) = renderer() else { return };
    let mut w = world();
    let bad = spec("materials/bad", Some("[params.paint]\ntype = \"color\"\ndefault = \"#0000FF\"\nfrom = \"Paint.glow\"\n\n[params.glow]\ntype = \"f32\"\ndefault = 0\nfrom = \"@nobody:Paint.glow\"\n"), Some(PAINT));
    load(&mut r, &[("materials/bad", &bad)]);
    sprite(&mut w, 0.0, 0.0, "rect", 2.0, "", "materials/bad", 0.0);
    let rep = r.render(&w, 0, 1.0).unwrap();
    assert_eq!(
        rep.material_problems.len(),
        2,
        "{:?}",
        rep.material_problems
    );
    assert!(
        rep.material_problems[0].contains("a f32 field cannot give a color parameter"),
        "{:?}",
        rep.material_problems
    );
    assert!(
        rep.material_problems[1].contains("@nobody"),
        "{:?}",
        rep.material_problems
    );
    assert_eq!(px(&r, 0.0, 0.0), [0, 0, 255]);
    let _ = Value::Bool(true);
}

/// An effect names a material, and its particles draw with it: here an
/// additive one, so green sparks over red make yellow.
#[test]
fn particles_draw_with_their_effects_material() {
    use std::cell::RefCell;
    use std::rc::Rc;
    let Some(mut r) = renderer() else { return };
    let mut w = world();
    let add = spec(
        "fx/add",
        Some("shader = \"unlit\"\nblend = \"add\"\n"),
        None,
    );
    load(&mut r, &[("fx/add", &add)]);
    let mut sched = argus_sim::Scheduler::new(argus_sim::SchedulerConfig::continuous(60.0).seed(1));
    let store = Rc::new(RefCell::new(argus_particles::ParticleStore::default()));
    argus_particles::install(&mut w, &mut sched, store.clone()).unwrap();
    let (fx, _) = argus_assets::effect::parse_emitter(
        "fx/sparks",
        "burst = 1\nspeed = 0\nlifetime = 100\nsize = 2\ncolor = \"#00FF00\"\nmaterial = \"fx/add\"\nz = 1\n",
    )
    .unwrap();
    store.borrow_mut().set_effect("fx/sparks", &fx);
    sprite(&mut w, 0.0, 0.0, "rect", 2.0, "#FF0000", "", 0.0);
    let e = w.spawn();
    w.insert(e, Transform::at(0.0, 0.0)).unwrap();
    let effect = w.strings().intern("fx/sparks");
    w.insert(
        e,
        argus_particles::Emitter {
            effect,
            ..Default::default()
        },
    )
    .unwrap();
    sched.step(&mut w, 1).unwrap();
    let rep = r
        .render_frame(&w, sched.tick(), 1.0, &[], Some(&store.borrow()))
        .unwrap();
    assert_eq!((rep.particles, rep.stats.materials), (1, 2), "{rep:?}");
    assert_eq!(px(&r, 0.0, 0.0), [255, 255, 0]);
}

/// Draw order is z, whatever the material: a sprite with its own material
/// under a higher sheet with none is covered by it, and over a lower one
/// covers it (the colony template's art pass found the first drawn on top).
#[test]
fn a_material_sprite_keeps_its_place_in_z() {
    let Some(mut r) = renderer() else { return };
    let mut w = world();
    let flat = spec(
        "materials/flat",
        None,
        Some("fn material(in: MaterialInput, p: Params) -> vec4<f32> {\n    return vec4<f32>(1.0, 0.0, 0.0, 1.0);\n}\n"),
    );
    load(&mut r, &[("materials/flat", &flat)]);
    sprite(&mut w, -2.0, 0.0, "rect", 2.0, "", "materials/flat", 1.0);
    sprite(&mut w, -2.0, 0.0, "rect", 4.0, "#0000FF", "", 50.0);
    sprite(&mut w, 2.0, 0.0, "rect", 2.0, "", "materials/flat", 50.0);
    sprite(&mut w, 2.0, 0.0, "rect", 4.0, "#0000FF", "", 1.0);
    r.render(&w, 0, 1.0).unwrap();
    assert_eq!(px(&r, -2.0, 0.0), [0, 0, 255], "the higher sheet covers it");
    assert_eq!(px(&r, 2.0, 0.0), [255, 0, 0], "it covers the lower sheet");
}
