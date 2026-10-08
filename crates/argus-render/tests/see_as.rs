//! Phase 5 item 2: a frame as a colour-blind eye sees it, to check
//! readability without guessing. A red and a green that anyone with full
//! colour vision tells apart turn to two yellows for deuteranopia, the
//! commonest kind, and to greys for achromatopsia; the game is not changed,
//! only the way it is looked at.

use argus_ecs::{Transform, Vec2, World};
use argus_render::{Camera, Renderer, SeeAs, Sprite};

fn world() -> World {
    let mut w = World::new();
    w.register::<Transform>().unwrap();
    argus_render::register(&mut w).unwrap();
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
    for (x, color) in [(-3.0, "#D02020"), (3.0, "#20B020")] {
        let e = w.spawn();
        w.insert(e, Transform::at(x, 0.0)).unwrap();
        let s = Sprite {
            kind: w.strings().intern("rect"),
            size: Vec2::new(4.0, 4.0),
            color: w.strings().intern(color),
            ..Default::default()
        };
        w.insert(e, s).unwrap();
    }
    w
}

/// The red swatch's pixel and the green's.
fn swatches(r: &mut Renderer, w: &World, kind: Option<SeeAs>) -> ([u8; 3], [u8; 3]) {
    r.gpu_mut().set_see_as(kind);
    r.render(w, 0, 1.0).unwrap();
    let img = r.capture().unwrap();
    let at = |x: u32| {
        let i = ((img.height / 2 * img.width + x) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    };
    (at(img.width / 4), at(img.width * 3 / 4))
}

#[test]
fn red_and_green_are_seen_as_a_colour_blind_eye_sees_them() {
    let w = world();
    let mut tried = 0;
    for software in [false, true] {
        let Ok(mut r) = Renderer::offscreen(320, 180, software) else {
            continue;
        };
        tried += 1;
        let who = r.gpu().adapter.clone();
        let (red, green) = swatches(&mut r, &w, None);
        assert!(red[0] > 150 && red[1] < 80, "{who}: red as drawn {red:?}");
        assert!(
            green[1] > 120 && green[0] < 80,
            "{who}: green as drawn {green:?}"
        );

        // Deuteranopia: both yellow, red over green in neither as drawn.
        let (red, green) = swatches(&mut r, &w, Some(SeeAs::Deuteranopia));
        for (c, what) in [(red, "red"), (green, "green")] {
            assert!(
                c[0] > c[2] && c[1] > c[2] && c[0].abs_diff(c[1]) < 40,
                "{who}: {what} seen with deuteranopia {c:?}"
            );
        }

        // Achromatopsia: grey.
        let (red, green) = swatches(&mut r, &w, Some(SeeAs::Achromatopsia));
        for c in [red, green] {
            assert!(
                c[0].abs_diff(c[1]) <= 1 && c[1].abs_diff(c[2]) <= 1,
                "{who}: {c:?}"
            );
        }

        // And back to the frame as it is.
        let (red, _) = swatches(&mut r, &w, None);
        assert!(red[0] > 150 && red[1] < 80, "{who}: {red:?}");
    }
    if tried == 0 {
        eprintln!("skipping: no adapter");
    }
}

#[test]
fn every_kind_has_its_name() {
    for k in SeeAs::ALL {
        assert_eq!(SeeAs::parse(k.name()), Some(k));
    }
    assert_eq!(SeeAs::parse("normal"), None);
}
