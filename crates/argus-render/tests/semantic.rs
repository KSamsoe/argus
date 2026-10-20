//! §8 L4 and L5: the semantic frame (what the player sees, as data, with
//! no GPU) and the annotated picture (ids and bounds drawn over it).

use argus_ecs::{FieldType, SchemaBuilder, StrId, Transform, Value, Vec2, World};
use argus_render::{annotate, semantic, Camera, Describe, FrameSpec, Image, Sprite, View};

/// A camera 10 units tall over 200 x 100 px: 10 px a unit, the origin at
/// the centre, so world (x, y) is pixel (100 + 10x, 50 - 10y).
fn world() -> World {
    let mut w = World::new();
    w.register::<Transform>().unwrap();
    argus_render::register(&mut w).unwrap();
    for (name, fields) in [
        ("Enemy", vec![("hp", FieldType::I32)]),
        (
            "Hero",
            vec![("hp", FieldType::I32), ("gold", FieldType::F32)],
        ),
    ] {
        let mut b = SchemaBuilder::new(name);
        for (f, t) in fields {
            b = b.field(f, t);
        }
        w.register_schema(b.build().unwrap()).unwrap();
    }
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

fn s(w: &World, x: &str) -> StrId {
    w.strings().intern(x)
}

fn thing(
    w: &mut World,
    id: Option<&str>,
    x: f32,
    y: f32,
    size: f32,
    comp: Option<&str>,
) -> argus_ecs::Entity {
    let e = match id {
        Some(id) => w.spawn_authored(id).unwrap(),
        None => w.spawn(),
    };
    w.insert(e, Transform::at(x, y)).unwrap();
    let sp = Sprite {
        kind: s(w, "rect"),
        color: s(w, "#FF0000"),
        size: Vec2::new(size, size),
        ..Default::default()
    };
    w.insert(e, sp).unwrap();
    if let Some(c) = comp {
        let id = w.component_id(c).unwrap();
        w.insert_default(e, id).unwrap();
    }
    e
}

fn spec() -> FrameSpec<'static> {
    FrameSpec {
        width: 200,
        height: 100,
        limit: 24,
        groups: 12,
        assets: None,
        particles: None,
        view: None,
        ui_canvas: None,
    }
}

/// Bounds are the frame's pixels; described entities, and any that are the
/// only one of their kind on screen, are listed one by one in reading
/// order; the rest are grouped by their game components.
#[test]
fn entities_are_listed_with_pixel_bounds_and_the_rest_grouped() {
    let mut w = world();
    let hero = thing(&mut w, Some("hero"), 0.0, 0.0, 2.0, Some("Hero"));
    for i in 0..5 {
        thing(&mut w, None, -6.0 + i as f32, 3.0, 1.0, Some("Enemy"));
    }
    thing(&mut w, None, 50.0, 0.0, 1.0, Some("Enemy")); // off screen
    thing(&mut w, None, 6.0, -3.0, 1.0, None); // the only one of its kind (its look)
    thing(&mut w, Some("named"), 7.0, 4.0, 1.0, Some("Enemy")); // an id, but one of six enemies
    w.set_field(hero, "Hero.hp", 7i64).unwrap();
    w.set_field(hero, "Hero.gold", 12.5f32).unwrap();
    w.insert(
        hero,
        Describe {
            name: s(&w, "hero"),
            text: s(&w, "HP {Hero.hp}, gold {Hero.gold}"),
            ..Default::default()
        },
    )
    .unwrap();
    let f = semantic(&w, &spec());
    assert_eq!(f.camera.as_deref(), Some("@camera"));
    assert_eq!(f.rect, [-10.0, -5.0, 10.0, 5.0]);
    assert_eq!(f.entities.len(), 2);
    let h = &f.entities[0];
    assert_eq!(
        (h.id.as_str(), h.name.as_str(), h.text.as_str()),
        ("@hero", "hero", "HP 7, gold 12.5")
    );
    assert_eq!(
        h.bounds,
        [90, 40, 110, 60],
        "2 units at 10 px a unit, centred"
    );
    assert_eq!(h.look, "rect #FF0000");
    let lone = &f.entities[1];
    assert_eq!(
        (lone.look.as_str(), lone.bounds),
        ("rect #FF0000", [155, 75, 165, 85]),
        "alone of its kind"
    );
    assert_eq!(f.groups.len(), 1);
    let enemies = &f.groups[0];
    assert_eq!(enemies.what, "Enemy");
    assert_eq!(
        enemies.count, 6,
        "the one off screen is not seen; the named one is counted"
    );
    assert_eq!(enemies.bounds, [35, 5, 175, 25]);
    assert_eq!(enemies.ids.len(), 3);
    assert!(f.problems.is_empty());
    let j = f.to_json();
    assert_eq!(j["entities"][0][0], "@hero");
    assert_eq!(j["columns"][3], "bounds");
    let bytes = j.to_string().len();
    assert!(bytes < 1500, "{bytes}");
}

/// HUD lines come first and in their order; text holes read this entity's
/// fields or a named one's, and a hole that names nothing says so.
#[test]
fn hud_text_fills_its_holes_in_reading_order() {
    let mut w = world();
    let hero = thing(&mut w, Some("hero"), 0.0, 0.0, 2.0, Some("Hero"));
    w.set_field(hero, "Hero.hp", 3i64).unwrap();
    let a = w.spawn();
    w.insert(
        a,
        Describe {
            text: s(&w, "Gold {@hero:Hero.gold}"),
            role: s(&w, "hud"),
            order: 2,
            ..Default::default()
        },
    )
    .unwrap();
    let b = w.spawn();
    w.insert(
        b,
        Describe {
            text: s(&w, "HP {@hero:Hero.hp}"),
            order: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let c = w.spawn();
    w.insert(
        c,
        Describe {
            text: s(&w, "Mana {@hero:Hero.mana} {Nope.x} {@ghost:Hero.hp}"),
            order: 3,
            ..Default::default()
        },
    )
    .unwrap();
    let d = w.spawn();
    w.insert(
        d,
        Describe {
            text: s(&w, "secret"),
            hidden: true,
            ..Default::default()
        },
    )
    .unwrap();
    let f = semantic(&w, &spec());
    let texts: Vec<&str> = f.hud.iter().map(|h| h.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "HP 3",
            "Gold 0",
            "Mana {?@hero:Hero.mana} {?Nope.x} {?@ghost:Hero.hp}"
        ]
    );
    assert_eq!(f.problems.len(), 3, "{:?}", f.problems);
    assert!(
        f.problems[2].contains("no entity @ghost"),
        "{:?}",
        f.problems
    );
    // Reading order among entities: top to bottom, then left to right.
    let mut w = world();
    for (id, x, y) in [("c", 3.0, -2.0), ("a", 3.0, 2.0), ("b", -3.0, 2.0)] {
        let e = thing(&mut w, Some(id), x, y, 1.0, None);
        w.insert(e, Describe::default()).unwrap();
    }
    let order: Vec<String> = semantic(&w, &spec())
        .entities
        .iter()
        .map(|e| e.id.clone())
        .collect();
    assert_eq!(order, ["@b", "@a", "@c"]);
}

/// Past the limit, listed entities join their kind's group; past the group
/// limit, groups are counted in omitted. A rotated sprite's bounds hold its
/// corners; a described entity without a sprite is a point.
#[test]
fn limits_rotation_and_points() {
    let mut w = world();
    for i in 0..4 {
        let e = thing(
            &mut w,
            Some(&format!("n{i}")),
            i as f32,
            0.0,
            1.0,
            Some("Enemy"),
        );
        w.insert(e, Describe::default()).unwrap();
    }
    let f = semantic(
        &w,
        &FrameSpec {
            limit: 2,
            groups: 1,
            ..spec()
        },
    );
    assert_eq!(f.entities.len(), 2);
    assert_eq!(f.groups[0].count, 2, "the other two, grouped");
    assert_eq!(f.omitted, 0);
    let mut w = world();
    let r = thing(&mut w, Some("r"), 0.0, 0.0, 2.0, None);
    w.update::<Transform, _>(r, |t| t.rot = std::f32::consts::FRAC_PI_4)
        .unwrap();
    let p = w.spawn_authored("sign").unwrap();
    w.insert(p, Transform::at(-5.0, 2.0)).unwrap();
    w.insert(
        p,
        Describe {
            text: s(&w, "Keep out"),
            ..Default::default()
        },
    )
    .unwrap();
    for (k, c) in [("A", 1.0), ("B", 2.0), ("C", 3.0)] {
        let sc = SchemaBuilder::new(k)
            .field("v", FieldType::I32)
            .build()
            .unwrap();
        w.register_schema(sc).unwrap();
        thing(&mut w, None, c, -3.0, 0.5, Some(k));
        thing(&mut w, None, c, -4.0, 0.5, Some(k));
    }
    let f = semantic(
        &w,
        &FrameSpec {
            groups: 2,
            ..spec()
        },
    );
    let rb = f.entities.iter().find(|e| e.id == "@r").unwrap().bounds;
    assert_eq!(
        rb,
        [85, 35, 115, 65],
        "a 2-unit square turned 45 degrees spans 2.83 units"
    );
    let sign = f.entities.iter().find(|e| e.id == "@sign").unwrap();
    assert_eq!(
        (sign.bounds, sign.text.as_str()),
        ([50, 30, 50, 30], "Keep out")
    );
    assert_eq!((f.groups.len(), f.omitted), (2, 1));
}

#[test]
fn a_described_thing_whose_sprite_is_hidden_is_not_listed() {
    // It was, at its point, though nothing of it was on the screen. A
    // described thing with no sprite at all still is.
    let mut w = world();
    let chest = thing(&mut w, Some("chest"), 2.0, 1.0, 1.0, None);
    w.insert(
        chest,
        Describe {
            text: s(&w, "A locked chest"),
            ..Default::default()
        },
    )
    .unwrap();
    w.update::<Sprite, _>(chest, |s| s.visible = false).unwrap();
    let sign = w.spawn_authored("sign").unwrap();
    w.insert(sign, Transform::at(-5.0, 2.0)).unwrap();
    w.insert(sign, Describe::default()).unwrap();
    let f = semantic(&w, &spec());
    let ids: Vec<&str> = f.entities.iter().map(|e| e.id.as_str()).collect();
    assert!(!ids.contains(&"@chest"), "{ids:?}");
    assert!(ids.contains(&"@sign"), "{ids:?}");
    // Shown again, it is listed with its sprite's bounds.
    w.update::<Sprite, _>(chest, |s| s.visible = true).unwrap();
    let f = semantic(&w, &spec());
    let c = f.entities.iter().find(|e| e.id == "@chest").expect("listed");
    assert_eq!(c.bounds, [115, 35, 125, 45]);
}

/// Effects on screen: the camera's shake and the materials sprites draw with.
#[test]
fn effects_report_shake_and_materials() {
    let mut w = world();
    let e = thing(&mut w, None, 0.0, 0.0, 1.0, None);
    w.update::<Sprite, _>(e, |sp| sp.material = w.strings().intern("dissolve"))
        .unwrap();
    let cam = w.entity("@camera").unwrap();
    w.insert(
        cam,
        argus_render::CameraShake {
            trauma: 0.5,
            ..Default::default()
        },
    )
    .unwrap();
    let f = semantic(&w, &spec());
    let kinds: Vec<&str> = f
        .effects
        .iter()
        .map(|x| x["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["shake", "material"]);
    assert_eq!(f.effects[1]["material"], "dissolve");
    assert!(f.entities[0].look.ends_with("+dissolve"));
    // A given view (the one a picture was drawn with) is used as given.
    let f = semantic(
        &w,
        &FrameSpec {
            view: Some(View {
                center: [5.0, 0.0],
                half: [10.0, 5.0],
                angle: 0.0,
            }),
            ..spec()
        },
    );
    assert_eq!(f.entities[0].bounds, [45, 45, 55, 55]);
    let _ = Value::Bool(true);
}

/// The annotation draws each box and its label, and a label that would
/// cover another goes below its box or is left off.
#[test]
fn annotation_draws_boxes_and_labels_that_do_not_overlap() {
    let mut w = world();
    for (id, x) in [("hero", 0.0), ("twin", 0.3), ("third", 0.6)] {
        let e = thing(&mut w, Some(id), x, 0.0, 2.0, None);
        w.insert(e, Describe::default()).unwrap();
    }
    let f = semantic(&w, &spec());
    let mut img = Image {
        width: 200,
        height: 100,
        rgba: vec![20; 200 * 100 * 4],
    };
    let marks = annotate(&mut img, &f);
    assert_eq!(marks.len(), 3);
    let labelled: Vec<bool> = marks.iter().map(|m| m.labelled).collect();
    assert_eq!(labelled, [true, true, false], "above, below, then no room");
    let yellow = img.count_color([255, 220, 40], 0);
    assert!(yellow > 80, "boxes and letters: {yellow}");
    // The box edge is yellow where the hero's bounds begin.
    let b = marks[0].bounds;
    assert_eq!(
        img.pixel(b[0] as u32, (b[1] + 5) as u32)[..3],
        [255, 220, 40]
    );
}

/// Swarm's stress scene at L4: fifty thousand enemies on screen are one
/// group, in well under a KB, in a few milliseconds. Release only:
/// `cargo test --release -p argus-render --test semantic -- --ignored`.
#[test]
#[ignore]
fn perf_fifty_thousand_entities() {
    let mut w = world();
    let enemy = w.component_id("Enemy").unwrap();
    let sp = w.component_id_of::<Sprite>().unwrap();
    let tr = w.component_id_of::<Transform>().unwrap();
    let look = Sprite {
        kind: s(&w, "circle"),
        color: s(&w, "#E8703F"),
        size: Vec2::new(0.1, 0.1),
        ..Default::default()
    };
    for i in 0..50_000u32 {
        let e = w.spawn_with(&[tr, sp, enemy]).unwrap();
        w.insert(
            e,
            Transform::at((i % 200) as f32 * 0.1 - 10.0, (i / 200) as f32 * 0.04 - 5.0),
        )
        .unwrap();
        w.insert(e, look).unwrap();
    }
    let mut best = f64::MAX;
    for _ in 0..5 {
        let t = std::time::Instant::now();
        let f = semantic(&w, &spec());
        best = best.min(t.elapsed().as_secs_f64() * 1e3);
        assert_eq!(f.groups[0].count, 50_000);
    }
    let bytes = semantic(&w, &spec()).to_json().to_string().len();
    println!("50,000 entities: semantic frame {best:.1} ms, {bytes} bytes");
    assert!(best < 20.0, "{best} ms");
}

#[test]
fn a_described_things_words_are_in_the_worlds_language() {
    // Describe's name and text went into the frame (and the screen reader)
    // as written; a string key is that key's string now, in the player's
    // language, and other words stay as written.
    let mut w = world();
    argus_ui::register(&mut w).unwrap();
    let chest = thing(&mut w, Some("chest"), 2.0, 1.0, 1.0, None);
    w.insert(
        chest,
        Describe {
            name: s(&w, "chest.name"),
            text: s(&w, "chest.text"),
            ..Default::default()
        },
    )
    .unwrap();
    let sign = w.spawn_authored("sign").unwrap();
    w.insert(sign, Transform::at(-5.0, 2.0)).unwrap();
    w.insert(
        sign,
        Describe {
            text: s(&w, "Keep out"),
            ..Default::default()
        },
    )
    .unwrap();
    let tables = argus_ui::Strings::from_texts(
        "en",
        &[
            ("en", "[chest]\nname = \"chest\"\ntext = \"A locked chest\"\n"),
            ("fr", "[chest]\nname = \"coffre\"\ntext = \"Un coffre ferm\u{e9}\"\n"),
        ],
    );
    let said = |w: &World, tables: Option<&argus_ui::Strings>| {
        let f = argus_render::semantic_with(w, &spec(), tables);
        let c = f.entities.iter().find(|e| e.id == "@chest").unwrap();
        let g = f.entities.iter().find(|e| e.id == "@sign").unwrap();
        (c.name.clone(), c.text.clone(), g.text.clone())
    };
    let words = |a: &str, b: &str, c: &str| (a.to_string(), b.to_string(), c.to_string());
    assert_eq!(said(&w, None), words("chest.name", "chest.text", "Keep out"));
    assert_eq!(said(&w, Some(&tables)), words("chest", "A locked chest", "Keep out"));
    let set = w.spawn();
    let fr = w.strings().intern("fr");
    w.insert(
        set,
        argus_ui::UiSettings {
            language: fr,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        said(&w, Some(&tables)),
        words("coffre", "Un coffre ferm\u{e9}", "Keep out")
    );
}
