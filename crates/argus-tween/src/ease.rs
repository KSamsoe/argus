//! Easing curves: progress 0 to 1 in, eased progress out (Penner's set).
//! `back` and `elastic` overshoot, which is what makes a squash read.

use std::f32::consts::PI;

/// Every easing name `ease` knows.
pub const NAMES: &[&str] = &[
    "linear",
    "step",
    "in_quad",
    "out_quad",
    "in_out_quad",
    "in_cubic",
    "out_cubic",
    "in_out_cubic",
    "in_quart",
    "out_quart",
    "in_out_quart",
    "in_sine",
    "out_sine",
    "in_out_sine",
    "in_expo",
    "out_expo",
    "in_out_expo",
    "in_back",
    "out_back",
    "in_out_back",
    "in_elastic",
    "out_elastic",
    "in_out_elastic",
    "in_bounce",
    "out_bounce",
    "in_out_bounce",
];

fn out_bounce(t: f32) -> f32 {
    let (n, d) = (7.5625, 2.75);
    if t < 1.0 / d {
        n * t * t
    } else if t < 2.0 / d {
        let t = t - 1.5 / d;
        n * t * t + 0.75
    } else if t < 2.5 / d {
        let t = t - 2.25 / d;
        n * t * t + 0.9375
    } else {
        let t = t - 2.625 / d;
        n * t * t + 0.984375
    }
}

/// Ease `t` (clamped to 0..1) by the named curve. `None` for an unknown name.
pub fn ease(name: &str, t: f32) -> Option<f32> {
    let t = t.clamp(0.0, 1.0);
    let c1 = 1.70158f32;
    let c2 = c1 * 1.525;
    let c3 = c1 + 1.0;
    let c4 = (2.0 * PI) / 3.0;
    let c5 = (2.0 * PI) / 4.5;
    let in_out = |f: &dyn Fn(f32) -> f32| {
        if t < 0.5 {
            f(2.0 * t) / 2.0
        } else {
            1.0 - f(2.0 - 2.0 * t) / 2.0
        }
    };
    Some(match name {
        "" | "linear" => t,
        "step" => {
            if t >= 1.0 {
                1.0
            } else {
                0.0
            }
        }
        "in_quad" => t * t,
        "out_quad" => 1.0 - (1.0 - t) * (1.0 - t),
        "in_out_quad" => in_out(&|x| x * x),
        "in_cubic" => t * t * t,
        "out_cubic" => 1.0 - (1.0 - t).powi(3),
        "in_out_cubic" => in_out(&|x| x * x * x),
        "in_quart" => t.powi(4),
        "out_quart" => 1.0 - (1.0 - t).powi(4),
        "in_out_quart" => in_out(&|x| x.powi(4)),
        "in_sine" => 1.0 - (t * PI / 2.0).cos(),
        "out_sine" => (t * PI / 2.0).sin(),
        "in_out_sine" => -((PI * t).cos() - 1.0) / 2.0,
        "in_expo" => {
            if t == 0.0 {
                0.0
            } else {
                2f32.powf(10.0 * t - 10.0)
            }
        }
        "out_expo" => {
            if t == 1.0 {
                1.0
            } else {
                1.0 - 2f32.powf(-10.0 * t)
            }
        }
        "in_out_expo" => {
            if t == 0.0 || t == 1.0 {
                t
            } else if t < 0.5 {
                2f32.powf(20.0 * t - 10.0) / 2.0
            } else {
                (2.0 - 2f32.powf(-20.0 * t + 10.0)) / 2.0
            }
        }
        "in_back" => c3 * t * t * t - c1 * t * t,
        "out_back" => 1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2),
        "in_out_back" => {
            if t < 0.5 {
                ((2.0 * t).powi(2) * ((c2 + 1.0) * 2.0 * t - c2)) / 2.0
            } else {
                ((2.0 * t - 2.0).powi(2) * ((c2 + 1.0) * (t * 2.0 - 2.0) + c2) + 2.0) / 2.0
            }
        }
        "in_elastic" => {
            if t == 0.0 || t == 1.0 {
                t
            } else {
                -(2f32.powf(10.0 * t - 10.0)) * ((t * 10.0 - 10.75) * c4).sin()
            }
        }
        "out_elastic" => {
            if t == 0.0 || t == 1.0 {
                t
            } else {
                2f32.powf(-10.0 * t) * ((t * 10.0 - 0.75) * c4).sin() + 1.0
            }
        }
        "in_out_elastic" => {
            if t == 0.0 || t == 1.0 {
                t
            } else if t < 0.5 {
                -(2f32.powf(20.0 * t - 10.0) * ((20.0 * t - 11.125) * c5).sin()) / 2.0
            } else {
                (2f32.powf(-20.0 * t + 10.0) * ((20.0 * t - 11.125) * c5).sin()) / 2.0 + 1.0
            }
        }
        "in_bounce" => 1.0 - out_bounce(1.0 - t),
        "out_bounce" => out_bounce(t),
        "in_out_bounce" => {
            if t < 0.5 {
                (1.0 - out_bounce(1.0 - 2.0 * t)) / 2.0
            } else {
                (1.0 + out_bounce(2.0 * t - 1.0)) / 2.0
            }
        }
        _ => return None,
    })
}
