//! A script's math is libm's (docs/multiplayer-checklist.md item 0). Luau's
//! C++ calls the libm crate for it (vendor/README.md), so `math.sin`, `^`,
//! and `vector.angle` are the same bits on every machine, where each
//! platform's C library rounds a few results in a hundred its own way.
//!
//! Both of Luau's paths are checked: a `math` function called as a value
//! (the library's C function) and written in a script (the VM's fast
//! call), and `^` at run time and folded by the compiler.

use mlua::{Compiler, Lua};

/// Inputs over the ranges a game uses and beyond: small, large, negative,
/// near zero, and the edges (from a fixed xorshift, so every run the same).
fn inputs(n: usize, lo: f64, hi: f64) -> Vec<f64> {
    let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut out = vec![0.0, -0.0, lo, hi, 1.0, 0.5, 1e-9, -1e-9];
    out.retain(|x| (lo..=hi).contains(x));
    while out.len() < n {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        let u = (s >> 11) as f64 / (1u64 << 53) as f64;
        out.push(lo + (hi - lo) * u);
    }
    out
}

/// `math.<name>` over the inputs both ways: as a value (`f(x)`) and as a
/// fast call in a loop.
fn both(lua: &Lua, name: &str, xs: &[f64], ys: Option<&[f64]>) -> (Vec<f64>, Vec<f64>) {
    let call = match ys {
        Some(_) => format!("math.{name}(xs[i], ys[i])"),
        None => format!("math.{name}(xs[i])"),
    };
    let src = format!(
        "local xs, ys = ...\nlocal f = math.{name}\nlocal a, b = {{}}, {{}}\n\
         for i = 1, #xs do\n  a[i] = {call}\n  b[i] = f(xs[i], ys and ys[i])\nend\nreturn a, b"
    );
    lua.load(src)
        .call::<(Vec<f64>, Vec<f64>)>((xs.to_vec(), ys.map(|y| y.to_vec())))
        .unwrap()
}

fn same(name: &str, got: &[f64], want: impl Fn(usize) -> f64, xs: &[f64]) {
    let mut wrong = Vec::new();
    for (i, g) in got.iter().enumerate() {
        let w = want(i);
        if g.to_bits() != w.to_bits() && !(g.is_nan() && w.is_nan()) {
            wrong.push(format!("{name}({}) = {g:e}, libm {w:e}", xs[i]));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} differ:\n{}",
        wrong.len(),
        got.len(),
        wrong[..wrong.len().min(10)].join("\n")
    );
}

#[test]
fn a_scripts_math_functions_are_libms_bit_for_bit() {
    let lua = Lua::new();
    let wide = inputs(4000, -1000.0, 1000.0);
    let small = inputs(4000, -4.0, 4.0);
    let unit = inputs(2000, -1.0, 1.0);
    let positive = inputs(4000, 1e-6, 1e6);
    type One = fn(f64) -> f64;
    let ones: [(&str, One, &[f64]); 13] = [
        ("sin", libm::sin, &wide),
        ("cos", libm::cos, &wide),
        ("tan", libm::tan, &small),
        ("asin", libm::asin, &unit),
        ("acos", libm::acos, &unit),
        ("atan", libm::atan, &wide),
        ("sinh", libm::sinh, &small),
        ("cosh", libm::cosh, &small),
        ("tanh", libm::tanh, &small),
        ("exp", libm::exp, &small),
        ("log", libm::log, &positive),
        ("sqrt", f64::sqrt, &positive),
        ("sin", libm::sin, &small),
    ];
    for (name, f, xs) in ones {
        let (fast, slow) = both(&lua, name, xs, None);
        same(name, &fast, |i| f(xs[i]), xs);
        same(name, &slow, |i| f(xs[i]), xs);
    }
    let ys = inputs(4000, -1000.0, 1000.0);
    let (fast, slow) = both(&lua, "atan2", &wide, Some(&ys));
    same("atan2", &fast, |i| libm::atan2(wide[i], ys[i]), &wide);
    same("atan2", &slow, |i| libm::atan2(wide[i], ys[i]), &wide);
    let powers = inputs(4000, -6.0, 6.0);
    let (fast, slow) = both(&lua, "pow", &positive, Some(&powers));
    same(
        "pow",
        &fast,
        |i| libm::pow(positive[i], powers[i]),
        &positive,
    );
    same(
        "pow",
        &slow,
        |i| libm::pow(positive[i], powers[i]),
        &positive,
    );
    // math.log with a base: log2 and log10 of their own, others a quotient.
    for (base, want) in [(2.0, libm::log2 as One), (10.0, libm::log10 as One)] {
        let bases = vec![base; positive.len()];
        let (fast, slow) = both(&lua, "log", &positive, Some(&bases));
        same("log", &fast, |i| want(positive[i]), &positive);
        same("log", &slow, |i| want(positive[i]), &positive);
    }
    let threes = vec![3.0; positive.len()];
    let (fast, _) = both(&lua, "log", &positive, Some(&threes));
    same(
        "log",
        &fast,
        |i| libm::log(positive[i]) / libm::log(3.0),
        &positive,
    );
}

#[test]
fn a_scripts_power_is_libms_at_run_time_and_when_folded() {
    let lua = Lua::new();
    let xs = inputs(4000, 1e-3, 1e3);
    let ys = inputs(4000, -5.0, 5.0);
    // x ^ y with both at run time, and with a constant exponent (its own
    // instruction, which takes 2, 0.5, and 3 as products and a root).
    let (run, k): (Vec<f64>, Vec<f64>) = lua
        .load(
            "local xs, ys = ...\nlocal a, b = {}, {}\n\
             for i = 1, #xs do a[i] = xs[i] ^ ys[i]; b[i] = xs[i] ^ 1.7 end\nreturn a, b",
        )
        .call((xs.clone(), ys.clone()))
        .unwrap();
    same("^", &run, |i| libm::pow(xs[i], ys[i]), &xs);
    same("^ 1.7", &k, |i| libm::pow(xs[i], 1.7), &xs);
    // Folded by the compiler: constants, at the level that folds builtins.
    let src = "return 2.5 ^ 1.3, math.sin(1.2), math.atan2(0.3, -2), math.exp(0.7), 7 ^ 0.31";
    let code = Compiler::new()
        .set_optimization_level(2)
        .compile(src)
        .unwrap();
    let folded: (f64, f64, f64, f64, f64) = lua.load(code).call(()).unwrap();
    let want = (
        libm::pow(2.5, 1.3),
        libm::sin(1.2),
        libm::atan2(0.3, -2.0),
        libm::exp(0.7),
        libm::pow(7.0, 0.31),
    );
    let bits = |t: (f64, f64, f64, f64, f64)| [t.0, t.1, t.2, t.3, t.4].map(f64::to_bits);
    assert_eq!(bits(folded), bits(want));
}

#[test]
fn vector_angle_is_libms_atan2() {
    let lua = Lua::new();
    let a = inputs(600, -50.0, 50.0);
    let got: Vec<f64> = lua
        .load(
            "local v = ...\nlocal out = {}\nfor i = 1, #v - 5, 6 do\n\
             out[#out + 1] = vector.angle(vector.create(v[i], v[i+1], v[i+2]), vector.create(v[i+3], v[i+4], v[i+5]))\n\
             end\nreturn out",
        )
        .call(a.clone())
        .unwrap();
    assert_eq!(got.len(), 100);
    for (k, g) in got.iter().enumerate() {
        let v: Vec<f32> = a[k * 6..k * 6 + 6].iter().map(|x| *x as f32).collect();
        let (p, q) = (&v[0..3], &v[3..6]);
        let c = [
            p[1] * q[2] - p[2] * q[1],
            p[2] * q[0] - p[0] * q[2],
            p[0] * q[1] - p[1] * q[0],
        ];
        // C++ takes the float overload of sqrt for a float.
        let sin_a = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt() as f64;
        let cos_a = (p[0] * q[0] + p[1] * q[1] + p[2] * q[2]) as f64;
        assert_eq!(
            g.to_bits(),
            libm::atan2(sin_a, cos_a).to_bits(),
            "{p:?} {q:?}"
        );
    }
}

#[test]
fn the_probe_reads_the_scripts_math_and_takes_two_arguments_where_it_needs_them() {
    let xs = [0.0, 0.5, 1.0, -2.5, 10.0];
    let sin = argus_luau::math_outputs("sin", &xs, &[]).unwrap();
    for (x, s) in xs.iter().zip(&sin) {
        assert_eq!(s.to_bits(), libm::sin(*x).to_bits(), "sin({x})");
    }
    let ys = [1.0, -1.0, 0.25, 3.0, -7.0];
    let atan2 = argus_luau::math_outputs("atan2", &xs, &ys).unwrap();
    for ((x, y), a) in xs.iter().zip(&ys).zip(&atan2) {
        assert_eq!(
            a.to_bits(),
            libm::atan2(*x, *y).to_bits(),
            "atan2({x}, {y})"
        );
    }
    assert!(argus_luau::math_outputs("nope", &xs, &[]).is_err());
}
