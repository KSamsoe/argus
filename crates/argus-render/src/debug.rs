//! Debug drawing (2D staples item 1): the shapes systems drew
//! ([`argus_sim::Draws`]) as quads on the UI canvas, under the UI and over
//! everything else, after the light and the post passes, so a shape comes
//! out as asked: its colour, and its width in canvas pixels at any zoom.
//! The same shapes in a picture's pixels are the semantic frame's `debug`.

use crate::semantic::{SemanticFrame, View};
use argus_ecs::Entity;
use argus_sim::draw::{Draws, Shape};
use argus_ui::draw::{Paint, Quad};
use argus_ui::{Fit, Shared};
use serde_json::json;

/// A quad turned `rot` radians about `pivot` (a fraction of it, y up),
/// which sits on `quad.pos`.
pub(crate) struct Piece {
    pub quad: Quad,
    pub rot: f32,
    pub pivot: [f32; 2],
}

/// World points into a target's pixels, through a view.
struct Lens {
    view: View,
    size: [f32; 2],
    cs: f32,
    sn: f32,
}

impl Lens {
    fn new(view: View, size: [f32; 2]) -> Lens {
        let (sn, cs) = view.angle.sin_cos();
        Lens { view, size, cs, sn }
    }

    /// A world point in the target's pixels (y down).
    fn px(&self, p: [f32; 2]) -> [f32; 2] {
        let (dx, dy) = (p[0] - self.view.center[0], p[1] - self.view.center[1]);
        let (vx, vy) = (dx * self.cs + dy * self.sn, -dx * self.sn + dy * self.cs);
        let [w, h] = self.size;
        [
            vx / (2.0 * self.view.half[0]) * w + w / 2.0,
            h / 2.0 - vy / (2.0 * self.view.half[1]) * h,
        ]
    }

    /// Target pixels a world unit spans.
    fn scale(&self) -> f32 {
        self.size[1] / (2.0 * self.view.half[1].max(1e-6))
    }
}

/// A circle's outline as this many points: about one every 3 pixels of
/// its edge, 12 to 64.
fn ring_points(radius_px: f32) -> usize {
    ((radius_px * std::f32::consts::TAU / 3.0) as usize).clamp(12, 64)
}

fn ring(at: [f32; 2], radius: f32, n: usize) -> impl Iterator<Item = [f32; 2]> {
    (0..=n).map(move |i| {
        let t = i as f32 / n as f32 * std::f32::consts::TAU;
        [at[0] + radius * t.cos(), at[1] + radius * t.sin()]
    })
}

/// A line `w` thick from `a` to `b` (canvas pixels), its ends squared off
/// half its width past them, so outlines meet at their corners.
fn seg(out: &mut Vec<Piece>, a: [f32; 2], b: [f32; 2], w: f32, color: [f32; 4], clip: [f32; 4]) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = (dx * dx + dy * dy).sqrt();
    let (ux, uy) = if len > 0.0 {
        (dx / len, dy / len)
    } else {
        (1.0, 0.0)
    };
    out.push(Piece {
        quad: Quad {
            pos: [a[0] - ux * w * 0.5, a[1] - uy * w * 0.5],
            size: [len + w, w],
            paint: Paint::Flat,
            color,
            clip,
            node: Entity::NULL,
        },
        // Canvas y is down; the quad turns in y up.
        rot: (-uy).atan2(ux),
        pivot: [0.0, 0.5],
    });
}

/// The shapes as quads on the canvas `fit` puts in a target `target`
/// pixels big showing `view`, in the systems' order and each system's.
pub(crate) fn pieces(
    draws: &Draws,
    shared: &Shared,
    view: View,
    target: [f32; 2],
    fit: &Fit,
) -> Vec<Piece> {
    let lens = Lens::new(view, target);
    let at = |p: [f32; 2]| fit.to_canvas(lens.px(p));
    let k = lens.scale() / fit.scale;
    let tl = fit.to_canvas([0.0, 0.0]);
    let br = fit.to_canvas(target);
    let clip = [tl[0], tl[1], br[0], br[1]];
    let mut out = Vec::new();
    for (_, d) in draws.iter() {
        let c = d.color;
        let color = [
            c[0] as f32 / 255.0,
            c[1] as f32 / 255.0,
            c[2] as f32 / 255.0,
            c[3] as f32 / 255.0,
        ];
        let w = d.size;
        match &d.shape {
            Shape::Line { from, to, arrow } => {
                let (a, b) = (at(*from), at(*to));
                seg(&mut out, a, b, w, color, clip);
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let len = (dx * dx + dy * dy).sqrt();
                if *arrow && len > 0.0 {
                    let (ux, uy) = (dx / len, dy / len);
                    let head = (4.0 + 3.0 * w).min(len * 0.5);
                    // Back from the tip, 30 degrees either side.
                    let (cs, sn) = (0.866_025_4, 0.5);
                    for s in [sn, -sn] {
                        let (bx, by) = (ux * cs - uy * s, ux * s + uy * cs);
                        seg(
                            &mut out,
                            b,
                            [b[0] - bx * head, b[1] - by * head],
                            w,
                            color,
                            clip,
                        );
                    }
                }
            }
            Shape::Box {
                at: p,
                size,
                fill: true,
            } => out.push(Piece {
                quad: Quad {
                    pos: at(*p),
                    size: [size[0] * k, size[1] * k],
                    paint: Paint::Flat,
                    color,
                    clip,
                    node: Entity::NULL,
                },
                rot: -view.angle,
                pivot: [0.5, 0.5],
            }),
            Shape::Box { at: p, size, .. } => {
                let (hw, hh) = (size[0] * 0.5, size[1] * 0.5);
                let corners = [
                    at([p[0] - hw, p[1] - hh]),
                    at([p[0] + hw, p[1] - hh]),
                    at([p[0] + hw, p[1] + hh]),
                    at([p[0] - hw, p[1] + hh]),
                ];
                for i in 0..4 {
                    seg(&mut out, corners[i], corners[(i + 1) % 4], w, color, clip);
                }
            }
            Shape::Circle {
                at: p,
                radius,
                fill: true,
            } => out.push(Piece {
                quad: Quad {
                    pos: at(*p),
                    size: [2.0 * radius * k, 2.0 * radius * k],
                    paint: Paint::Image {
                        sprite: "circle".into(),
                        frame: 0,
                        sub: [0.0, 0.0, 1.0, 1.0],
                    },
                    color,
                    clip,
                    node: Entity::NULL,
                },
                rot: 0.0,
                pivot: [0.5, 0.5],
            }),
            Shape::Circle { at: p, radius, .. } => {
                let pts: Vec<[f32; 2]> =
                    ring(*p, *radius, ring_points(radius * k)).map(at).collect();
                for pair in pts.windows(2) {
                    seg(&mut out, pair[0], pair[1], w, color, clip);
                }
            }
            Shape::Text { at: p, text } => {
                for quad in argus_ui::draw::label(shared, text, w, at(*p), color, clip) {
                    out.push(Piece {
                        quad,
                        rot: 0.0,
                        pivot: [0.0, 1.0],
                    });
                }
            }
        }
    }
    out
}

/// The most shapes a semantic frame lists; the rest are counted.
const LISTED: usize = 40;

impl SemanticFrame {
    /// List the shapes systems drew (2D staples item 1) under `debug`, in
    /// this frame's pixels: each with the system that drew it, those off
    /// the frame left out, and past [`LISTED`] counted.
    pub fn with_debug(&mut self, draws: &Draws) {
        if draws.is_empty() {
            return;
        }
        let size = [self.size[0] as f32, self.size[1] as f32];
        let lens = Lens::new(self.view, size);
        let k = lens.scale();
        let r = |v: f32| (v * 10.0).round() / 10.0;
        let pt = |p: [f32; 2]| {
            let q = lens.px(p);
            [r(q[0]), r(q[1])]
        };
        let inside = |b: [f32; 4]| b[2] >= 0.0 && b[3] >= 0.0 && b[0] <= size[0] && b[1] <= size[1];
        let span = |ps: &[[f32; 2]]| {
            let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
            for p in ps {
                b = [
                    b[0].min(p[0]),
                    b[1].min(p[1]),
                    b[2].max(p[0]),
                    b[3].max(p[1]),
                ];
            }
            b
        };
        let (mut shown, mut more, mut off) = (Vec::new(), 0usize, 0usize);
        for (by, d) in draws.iter() {
            let c = d.color;
            let color = if c[3] == 255 {
                format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
            } else {
                format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
            };
            let (o, bounds) = match &d.shape {
                Shape::Line { from, to, arrow } => {
                    let (a, b) = (pt(*from), pt(*to));
                    let kind = if *arrow { "arrow" } else { "line" };
                    (json!({ kind: [a[0], a[1], b[0], b[1]] }), span(&[a, b]))
                }
                Shape::Box { at, size: s, fill } => {
                    let (hw, hh) = (s[0] * 0.5, s[1] * 0.5);
                    let cs = [
                        pt([at[0] - hw, at[1] - hh]),
                        pt([at[0] + hw, at[1] - hh]),
                        pt([at[0] + hw, at[1] + hh]),
                        pt([at[0] - hw, at[1] + hh]),
                    ];
                    let b = span(&cs);
                    let mut o = json!({ "box": b });
                    if *fill {
                        o["fill"] = json!(true);
                    }
                    (o, b)
                }
                Shape::Circle { at, radius, fill } => {
                    let c = pt(*at);
                    let rp = r(radius * k);
                    let mut o = json!({ "circle": [c[0], c[1], rp] });
                    if *fill {
                        o["fill"] = json!(true);
                    }
                    (o, [c[0] - rp, c[1] - rp, c[0] + rp, c[1] + rp])
                }
                Shape::Text { at, text } => {
                    let p = pt(*at);
                    (json!({ "text": text, "at": p }), [p[0], p[1], p[0], p[1]])
                }
            };
            if !inside(bounds) {
                off += 1;
                continue;
            }
            if shown.len() >= LISTED {
                more += 1;
                continue;
            }
            let mut o = o;
            o["by"] = json!(by);
            o["color"] = json!(color);
            shown.push(o);
        }
        let mut out = json!({ "shapes": shown });
        if more > 0 {
            out["more"] = json!(more);
        }
        if off > 0 {
            out["off_frame"] = json!(off);
        }
        let dropped: usize = draws.dropped().values().sum();
        if dropped > 0 {
            out["dropped"] = json!(dropped);
        }
        self.debug = Some(out);
    }
}
