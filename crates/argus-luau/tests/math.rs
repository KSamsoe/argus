//! Luau's own math, over many inputs (phase 5 item 11, the determinism
//! spike's probe): what a script gets from `math.<name>` on this platform,
//! the C library Luau was built against, which is what Rust's `f64` calls.

#[test]
fn a_scripts_math_is_the_c_librarys_and_takes_two_arguments_where_it_needs_them() {
    let xs = [0.0, 0.5, 1.0, -2.5, 10.0];
    let sin = argus_luau::math_outputs("sin", &xs, &[]).unwrap();
    for (x, s) in xs.iter().zip(&sin) {
        assert_eq!(s.to_bits(), x.sin().to_bits(), "sin({x})");
    }
    let ys = [1.0, -1.0, 0.25, 3.0, -7.0];
    let atan2 = argus_luau::math_outputs("atan2", &xs, &ys).unwrap();
    for ((x, y), a) in xs.iter().zip(&ys).zip(&atan2) {
        assert_eq!(a.to_bits(), x.atan2(*y).to_bits(), "atan2({x}, {y})");
    }
    assert!(argus_luau::math_outputs("nope", &xs, &[]).is_err());
}
