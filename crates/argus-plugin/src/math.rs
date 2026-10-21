//! Sines, logarithms, and powers that give the same bits on every machine.
//!
//! `x.sin()` on a float is the platform's own C library, and Windows,
//! Linux, a Mac, and a browser each round a few results in a hundred
//! differently in the last bit. A replay on another machine, or the other
//! player's half of a match, then drifts from that bit on. These are the
//! engine's functions (the `libm` crate's, as the engine's own systems and
//! Luau's `math` use), so a plugin's world is the same everywhere. They
//! take `f32` or `f64`:
//!
//! ```
//! use argus_plugin::math;
//!
//! /// Glide toward `to`, `tau` seconds being the time constant.
//! fn glide(x: f32, to: f32, dt: f32, tau: f32) -> f32 {
//!     x + (to - x) * (1.0 - math::exp(-dt / tau))
//! }
//! ```
//!
//! `+`, `-`, `*`, `/`, `sqrt`, `abs`, `floor`, and `round` are exact
//! everywhere already, so they stay the float's own. A small power is
//! clearest as a product (`d * d`), or [`powi`].
//!
//! They work inside a system's run, where the engine is; called anywhere
//! else (a plugin's own unit test) they panic.

use crate::abi::{self, math_op};
use core::ops::{Div, Mul};
use core::sync::atomic::{AtomicPtr, Ordering};

static API: AtomicPtr<abi::HostApi> = AtomicPtr::new(core::ptr::null_mut());

/// Remember the engine's table: every system's run passes through here.
pub(crate) fn bind(api: *const abi::HostApi) {
    API.store(api as *mut abi::HostApi, Ordering::Relaxed);
}

fn api() -> &'static abi::HostApi {
    let p = API.load(Ordering::Relaxed);
    assert!(
        !p.is_null(),
        "argus_plugin::math needs the engine: call it from a system's run"
    );
    // SAFETY: the engine's table, which lives as long as the engine.
    unsafe { &*p }
}

/// `f32` or `f64`.
pub trait Real: Copy + Mul<Output = Self> + Div<Output = Self> {
    /// One.
    const ONE: Self;
    #[doc(hidden)]
    fn op(op: u32, a: Self, b: Self) -> Self;
}

impl Real for f32 {
    const ONE: f32 = 1.0;
    fn op(op: u32, a: f32, b: f32) -> f32 {
        // SAFETY: a pure function of its arguments.
        unsafe { (api().math32)(op, a, b) }
    }
}

impl Real for f64 {
    const ONE: f64 = 1.0;
    fn op(op: u32, a: f64, b: f64) -> f64 {
        // SAFETY: as above.
        unsafe { (api().math64)(op, a, b) }
    }
}

/// sin(x), x in radians.
pub fn sin<T: Real>(x: T) -> T {
    T::op(math_op::SIN, x, x)
}

/// cos(x).
pub fn cos<T: Real>(x: T) -> T {
    T::op(math_op::COS, x, x)
}

/// (sin(x), cos(x)).
pub fn sin_cos<T: Real>(x: T) -> (T, T) {
    (sin(x), cos(x))
}

/// tan(x).
pub fn tan<T: Real>(x: T) -> T {
    T::op(math_op::TAN, x, x)
}

/// asin(x).
pub fn asin<T: Real>(x: T) -> T {
    T::op(math_op::ASIN, x, x)
}

/// acos(x).
pub fn acos<T: Real>(x: T) -> T {
    T::op(math_op::ACOS, x, x)
}

/// atan(x).
pub fn atan<T: Real>(x: T) -> T {
    T::op(math_op::ATAN, x, x)
}

/// The angle of the point (x, y): y first, as `f32::atan2` is `y.atan2(x)`.
pub fn atan2<T: Real>(y: T, x: T) -> T {
    T::op(math_op::ATAN2, y, x)
}

/// sinh(x).
pub fn sinh<T: Real>(x: T) -> T {
    T::op(math_op::SINH, x, x)
}

/// cosh(x).
pub fn cosh<T: Real>(x: T) -> T {
    T::op(math_op::COSH, x, x)
}

/// tanh(x).
pub fn tanh<T: Real>(x: T) -> T {
    T::op(math_op::TANH, x, x)
}

/// e to the x.
pub fn exp<T: Real>(x: T) -> T {
    T::op(math_op::EXP, x, x)
}

/// 2 to the x.
pub fn exp2<T: Real>(x: T) -> T {
    T::op(math_op::EXP2, x, x)
}

/// The natural logarithm.
pub fn ln<T: Real>(x: T) -> T {
    T::op(math_op::LN, x, x)
}

/// Log base 2.
pub fn log2<T: Real>(x: T) -> T {
    T::op(math_op::LOG2, x, x)
}

/// Log base 10.
pub fn log10<T: Real>(x: T) -> T {
    T::op(math_op::LOG10, x, x)
}

/// x to the y.
pub fn pow<T: Real>(x: T, y: T) -> T {
    T::op(math_op::POW, x, y)
}

/// sqrt(x*x + y*y), without overflowing on the way.
pub fn hypot<T: Real>(x: T, y: T) -> T {
    T::op(math_op::HYPOT, x, y)
}

/// The cube root.
pub fn cbrt<T: Real>(x: T) -> T {
    T::op(math_op::CBRT, x, x)
}

/// x to a whole power, by products in a fixed order (needs no engine).
pub fn powi<T: Real>(x: T, n: i32) -> T {
    let (mut a, mut b, mut r) = (x, n.unsigned_abs(), T::ONE);
    while b > 0 {
        if b & 1 == 1 {
            r = r * a;
        }
        b >>= 1;
        if b > 0 {
            a = a * a;
        }
    }
    if n < 0 {
        T::ONE / r
    } else {
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powi_is_the_products() {
        assert_eq!(powi(3.0f32, 0), 1.0);
        assert_eq!(powi(3.0f32, 4), 81.0);
        assert_eq!(powi(2.0f64, -2), 0.25);
        let x = 1.1f32;
        assert_eq!(powi(x, 3).to_bits(), (x * (x * x)).to_bits());
    }
}
