//! The cross-platform determinism spike's math probe (phase 5 item 11).
//! A game's floats are the same on two platforms when every operation
//! gives the same bits. `+ - * /` and `sqrt` are exactly rounded by IEEE
//! 754 everywhere; the functions below are not, and each platform's C
//! library (or Rust's port of musl's, in wasm) computes them its own way.
//! The probe evaluates each over the same inputs, as Rust's `f32` and `f64`
//! and as a Luau script's `math` (the C library Luau was built against),
//! and says how many results differ from the `libm` crate, a pure-Rust
//! port of musl that gives the same bits on every platform, and a digest
//! of the results to compare across platforms.
use serde_json::{json, Map, Value as Json};

/// splitmix64: the same inputs on every platform.
struct Inputs(u64);

impl Inputs {
    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
}

/// The inputs of a function: one argument, or two.
fn inputs(name: &str, n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut r = Inputs(0x0005_eed0_fa79_u64 ^ name.len() as u64);
    let mut a = Vec::with_capacity(n);
    let mut b = Vec::new();
    for i in 0..n {
        match name {
            "sin" | "cos" | "tan" => a.push(if i % 4 == 0 {
                r.range(-1000.0, 1000.0)
            } else {
                r.range(-10.0, 10.0)
            }),
            "atan2" => {
                a.push(r.range(-10.0, 10.0));
                b.push(r.range(-10.0, 10.0));
            }
            "exp" => a.push(r.range(-30.0, 30.0)),
            "ln" => a.push(10f64.powf(r.range(-6.0, 4.0))),
            "pow" => {
                a.push(10f64.powf(r.range(-3.0, 1.0)));
                b.push(r.range(-6.0, 6.0));
            }
            _ => a.push(r.range(0.0, 1e6)),
        }
    }
    (a, b)
}

/// How the results compare with the reference's.
#[derive(Default)]
struct Tally {
    digest: u64,
    differ: usize,
    nan_bits: usize,
    max_ulp: u64,
    first: Option<Json>,
}

impl Tally {
    fn new() -> Tally {
        Tally {
            digest: 0xcbf29ce484222325,
            ..Tally::default()
        }
    }
    fn add(&mut self, bits: u64, reference: u64, ulp: u64, nan: bool, input: &str) {
        for b in bits.to_le_bytes() {
            self.digest ^= b as u64;
            self.digest = self.digest.wrapping_mul(0x100000001b3);
        }
        if bits == reference {
            return;
        }
        if nan {
            self.nan_bits += 1;
            return;
        }
        self.differ += 1;
        self.max_ulp = self.max_ulp.max(ulp);
        if self.first.is_none() {
            self.first = Some(
                json!({ "input": input, "got": format!("{bits:x}"), "reference": format!("{reference:x}") }),
            );
        }
    }
    fn json(self, n: usize) -> Json {
        json!({
            "n": n,
            "digest": format!("{:016x}", self.digest),
            "differ": self.differ,
            "max_ulp": self.max_ulp,
            "nan_bits": self.nan_bits,
            "first": self.first,
        })
    }
}

fn ulp32(a: f32, b: f32) -> u64 {
    (a.to_bits() as i64 - b.to_bits() as i64).unsigned_abs()
}

fn ulp64(a: f64, b: f64) -> u64 {
    (a.to_bits() as i128 - b.to_bits() as i128).unsigned_abs() as u64
}

/// A function and the reference's, each of one argument or two.
type Pair<T> = (fn(T, T) -> T, fn(T, T) -> T);

/// Rust's `f32` function and the reference's, by name.
fn f32_pair(name: &str) -> Pair<f32> {
    match name {
        "sin" => (|x, _| x.sin(), |x, _| libm::sinf(x)),
        "cos" => (|x, _| x.cos(), |x, _| libm::cosf(x)),
        "tan" => (|x, _| x.tan(), |x, _| libm::tanf(x)),
        "atan2" => (|y, x| y.atan2(x), libm::atan2f),
        "exp" => (|x, _| x.exp(), |x, _| libm::expf(x)),
        "ln" => (|x, _| x.ln(), |x, _| libm::logf(x)),
        "pow" => (|x, y| x.powf(y), libm::powf),
        _ => (|x, _| x.sqrt(), |x, _| libm::sqrtf(x)),
    }
}

/// Rust's `f64` function and the reference's, by name.
fn f64_pair(name: &str) -> Pair<f64> {
    match name {
        "sin" => (|x, _| x.sin(), |x, _| libm::sin(x)),
        "cos" => (|x, _| x.cos(), |x, _| libm::cos(x)),
        "tan" => (|x, _| x.tan(), |x, _| libm::tan(x)),
        "atan2" => (|y, x| y.atan2(x), libm::atan2),
        "exp" => (|x, _| x.exp(), |x, _| libm::exp(x)),
        "ln" => (|x, _| x.ln(), |x, _| libm::log(x)),
        "pow" => (|x, y| x.powf(y), libm::pow),
        _ => (|x, _| x.sqrt(), |x, _| libm::sqrt(x)),
    }
}

/// The functions measured; `sqrt` is the control, exact everywhere.
pub const FUNCTIONS: [&str; 8] = ["sin", "cos", "tan", "atan2", "exp", "ln", "pow", "sqrt"];

/// Each function over `n` inputs: as Rust's `f32`, Rust's `f64`, and Luau's
/// `math`, each with its digest and how many results differ from the
/// reference (`differ`, the largest by how many units in the last place,
/// and the first, to look at). Luau's also `as_f32`: the same after
/// rounding to an f32, as a script's double is when a component keeps it.
pub fn math_probe(n: usize) -> Json {
    let mut rust32 = Map::new();
    let mut rust64 = Map::new();
    let mut luau = Map::new();
    for name in FUNCTIONS {
        let (a, b) = inputs(name, n);
        let two = !b.is_empty();
        let (f, r) = f32_pair(name);
        let mut t = Tally::new();
        for i in 0..n {
            let (x, y) = (a[i] as f32, if two { b[i] as f32 } else { 0.0 });
            let (got, want) = (f(x, y), r(x, y));
            let nan = got.is_nan() && want.is_nan();
            t.add(
                got.to_bits() as u64,
                want.to_bits() as u64,
                ulp32(got, want),
                nan,
                &format!("{:x} {:x}", x.to_bits(), y.to_bits()),
            );
        }
        rust32.insert(name.into(), t.json(n));
        let (f, r) = f64_pair(name);
        let mut t = Tally::new();
        for i in 0..n {
            let (x, y) = (a[i], if two { b[i] } else { 0.0 });
            let (got, want) = (f(x, y), r(x, y));
            let nan = got.is_nan() && want.is_nan();
            t.add(
                got.to_bits(),
                want.to_bits(),
                ulp64(got, want),
                nan,
                &format!("{:x} {:x}", x.to_bits(), y.to_bits()),
            );
        }
        rust64.insert(name.into(), t.json(n));
        let lname = if name == "ln" { "log" } else { name };
        let (_, r) = f64_pair(name);
        let got = argus_luau::math_outputs(lname, &a, &b);
        luau.insert(
            name.into(),
            match got {
                Ok(got) => {
                    // And as a component keeps them: a script's double,
                    // written to an f32 field, rounded.
                    let (mut t, mut kept) = (Tally::new(), Tally::new());
                    for i in 0..n {
                        let y = if two { b[i] } else { 0.0 };
                        let want = r(a[i], y);
                        let nan = got[i].is_nan() && want.is_nan();
                        let at = format!("{:x} {:x}", a[i].to_bits(), y.to_bits());
                        t.add(
                            got[i].to_bits(),
                            want.to_bits(),
                            ulp64(got[i], want),
                            nan,
                            &at,
                        );
                        let (g, w) = (got[i] as f32, want as f32);
                        kept.add(
                            g.to_bits() as u64,
                            w.to_bits() as u64,
                            ulp32(g, w),
                            nan,
                            &at,
                        );
                    }
                    let mut j = t.json(n);
                    j["as_f32"] = kept.json(n);
                    j
                }
                Err(e) => json!({ "error": e.message }),
            },
        );
    }
    // A page has no clock fine enough, and its functions are musl's.
    let speed = if cfg!(target_arch = "wasm32") {
        Json::Null
    } else {
        speed()
    };
    json!({ "rust_f32": rust32, "rust_f64": rust64, "luau": luau, "speed": speed })
}

/// What the reference costs beside the platform's own: nanoseconds a call
/// of each `f32` function over a million inputs, the platform's and the
/// `libm` crate's (what moving the engine's math to `libm` would cost).
fn speed() -> Json {
    let mut out = Map::new();
    for name in FUNCTIONS {
        let (a, b) = inputs(name, 1 << 16);
        let two = !b.is_empty();
        let (a, b): (Vec<f32>, Vec<f32>) = (
            a.iter().map(|x| *x as f32).collect(),
            if two {
                b.iter().map(|x| *x as f32).collect()
            } else {
                vec![0.0; a.len()]
            },
        );
        let (f, r) = f32_pair(name);
        let time = |g: fn(f32, f32) -> f32| {
            let t = std::time::Instant::now();
            let mut acc = 0.0f32;
            for _ in 0..16 {
                for i in 0..a.len() {
                    acc += g(std::hint::black_box(a[i]), b[i]);
                }
            }
            std::hint::black_box(acc);
            t.elapsed().as_secs_f64() * 1e9 / (16 * a.len()) as f64
        };
        out.insert(
            name.into(),
            json!({ "platform_ns": time(f), "libm_ns": time(r) }),
        );
    }
    Json::Object(out)
}
