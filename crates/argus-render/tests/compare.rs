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
