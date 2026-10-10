//! Image goldens' comparison (§9: "image goldens exist as an opt-in for
//! rendering work, with a perceptual threshold"; phase 5 item 0). No GPU:
//! pictures made by hand.

use argus_render::Image;

fn flat(w: u32, h: u32, c: [u8; 4]) -> Image {
    Image {
        width: w,
        height: h,
        rgba: c.repeat((w * h) as usize),
    }
}

fn set(img: &mut Image, i: usize, c: [u8; 4]) {
    img.rgba[i * 4..i * 4 + 4].copy_from_slice(&c);
}

#[test]
fn a_picture_is_compared_as_the_eye_sees_it() {
    let golden = flat(10, 10, [100, 120, 140, 255]);
    let same = golden.compare(&golden, 0.1).unwrap();
    assert_eq!((same.pixels, same.differing, same.worst), (100, 0, 0.0));

    // A change too small to see passes at a person's threshold, and counts
    // at zero.
    let mut faint = golden.clone();
    set(&mut faint, 0, [102, 120, 140, 255]);
    assert_eq!(faint.compare(&golden, 0.1).unwrap().differing, 0);
    assert_eq!(faint.compare(&golden, 0.0).unwrap().differing, 1);

    // Ten pixels made red: those ten, red in the difference picture, and
    // the rest the golden faded.
    let mut red = golden.clone();
    for i in 0..10 {
        set(&mut red, i, [255, 0, 0, 255]);
    }
    let c = red.compare(&golden, 0.1).unwrap();
    assert_eq!(c.differing, 10);
    assert!((c.fraction() - 0.1).abs() < 1e-9);
    assert!(c.worst > 0.1 && c.worst <= 1.0, "{}", c.worst);
    assert_eq!(c.diff.pixel(0, 0), [255, 0, 0, 255]);
    let faded = c.diff.pixel(5, 5);
    assert!(faded[0] == faded[1] && faded[0] > 200, "{faded:?}");

    // Brightness weighs more than hue: the same step in blue alone is
    // smaller than in grey.
    let mut grey = golden.clone();
    set(&mut grey, 0, [130, 150, 170, 255]);
    let mut blue = golden.clone();
    set(&mut blue, 0, [100, 120, 170, 255]);
    assert!(grey.compare(&golden, 0.0).unwrap().worst > blue.compare(&golden, 0.0).unwrap().worst);

    // A translucent pixel is seen over white.
    let clear = flat(1, 1, [0, 0, 0, 0]);
    let white = flat(1, 1, [255, 255, 255, 255]);
    assert_eq!(clear.compare(&white, 0.0).unwrap().worst, 0.0);

    // Sizes must agree, and the error says what to do.
    let e = flat(5, 5, [0; 4]).compare(&golden, 0.1).unwrap_err();
    assert!(
        e.message.contains("5x5") && e.message.contains("10x10"),
        "{e}"
    );
}

#[test]
fn a_release_check_forgives_an_edge_moved_by_a_pixel_but_not_a_colour() {
    // A tan row of three pixels on a dark ground, one row lower in the
    // other picture: a texel boundary that fell the other way (Hollow's
    // feather on klab's GPU, distribution item 5).
    let (dark, tan) = ([24, 27, 42, 255], [184, 155, 112, 255]);
    let (w, h) = (12u32, 8u32);
    let mut a = flat(w, h, dark);
    let mut b = flat(w, h, dark);
    for x in 4..7 {
        set(&mut a, (3 * w + x) as usize, tan);
        set(&mut b, (4 * w + x) as usize, tan);
    }
    assert_eq!(
        a.compare(&b, 0.1).unwrap().differing,
        6,
        "strictly, six pixels"
    );
    let near = a.compare_near(&b, 0.1).unwrap();
    assert_eq!(near.differing, 0, "moved by a pixel, not changed");
    assert_eq!(near.pixels, (w * h) as u64);
    // A colour that is not anywhere near: it still counts.
    let mut c = flat(w, h, dark);
    set(&mut c, (3 * w + 5) as usize, [255, 0, 255, 255]);
    assert_eq!(flat(w, h, dark).compare_near(&c, 0.1).unwrap().differing, 1);
    // Moved by two pixels is a change.
    let mut d = flat(w, h, dark);
    for x in 4..7 {
        set(&mut d, (5 * w + x) as usize, tan);
    }
    assert_eq!(a.compare_near(&d, 0.1).unwrap().differing, 6);
}

/// The differing pixels' groups (touching, diagonals included), largest
/// first: what a release's check tells a speck from a change by.
#[test]
fn differing_pixels_are_counted_in_groups() {
    let (w, h) = (16u32, 16u32);
    let dark = [20, 20, 20, 255];
    let golden = flat(w, h, dark);
    let mut b = golden.clone();
    let mark = |b: &mut Image, x: u32, y: u32| set(b, (y * w + x) as usize, [250, 250, 250, 255]);
    // A speck of two, side by side; one alone; and a blob of six.
    mark(&mut b, 1, 1);
    mark(&mut b, 2, 1);
    mark(&mut b, 8, 2);
    for (x, y) in [(10, 10), (11, 10), (12, 10), (10, 11), (11, 11), (12, 12)] {
        mark(&mut b, x, y);
    }
    let c = b.compare_near(&golden, 0.1).unwrap();
    assert_eq!(c.differing, 9);
    assert_eq!(c.groups(), vec![6, 2, 1]);
    assert!(golden
        .compare_near(&golden, 0.1)
        .unwrap()
        .groups()
        .is_empty());
}
