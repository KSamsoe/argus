//! A tilemap's collision as fixed colliders: solid and one-way tiles
//! merged into as few boxes as rows and columns allow, each slope its own
//! polygon. The kinematic tier reads the same cells (argus-physics' env);
//! a one-way tile is solid here, since a rigid body has no "from above".

use argus_tiles::{Collide, MapStore, TileGrid};
use rapier2d::prelude::*;

const EPS: f32 = 1e-4;

fn normal(k: Collide) -> Collide {
    match k {
        Collide::Slope { left, right } if left >= 1.0 - EPS && right >= 1.0 - EPS => Collide::Solid,
        Collide::Slope { left, right } if left <= EPS && right <= EPS => Collide::Empty,
        k => k,
    }
}

/// The colliders for a grid, in row then column order.
pub(crate) fn colliders(store: &mut MapStore, g: &TileGrid) -> Vec<ColliderBuilder> {
    let (w, h) = (g.width as i64, g.height as i64);
    let tile = g.tile;
    let left = |c: i64| g.origin[0] + c as f32 * tile;
    let top = |r: i64| g.origin[1] - r as f32 * tile;
    let mut out = Vec::new();
    let boxed = |out: &mut Vec<ColliderBuilder>, c0: i64, c1: i64, r0: i64, r1: i64| {
        let (x0, x1) = (left(c0), left(c1 + 1));
        let (y1, y0) = (top(r0), top(r1 + 1));
        out.push(
            ColliderBuilder::cuboid((x1 - x0) * 0.5, (y1 - y0) * 0.5)
                .translation(Vector::new((x0 + x1) * 0.5, (y0 + y1) * 0.5)),
        );
    };
    // Runs of solid cells open in the rows above: (first column, last
    // column, first row).
    let mut open: Vec<(i64, i64, i64)> = Vec::new();
    for r in 0..h {
        let mut runs: Vec<(i64, i64)> = Vec::new();
        let mut c = 0;
        while c < w {
            match normal(store.collide(g, c, r)) {
                Collide::Solid | Collide::OneWay => {
                    let c0 = c;
                    while c + 1 < w
                        && matches!(
                            normal(store.collide(g, c + 1, r)),
                            Collide::Solid | Collide::OneWay
                        )
                    {
                        c += 1;
                    }
                    runs.push((c0, c));
                }
                Collide::Slope { left: l, right: rt } => {
                    let (x0, x1, yb) = (left(c), left(c + 1), top(r + 1));
                    let pts = [
                        Vector::new(x0, yb),
                        Vector::new(x1, yb),
                        Vector::new(x1, yb + rt.clamp(0.0, 1.0) * tile),
                        Vector::new(x0, yb + l.clamp(0.0, 1.0) * tile),
                    ];
                    if let Some(b) = ColliderBuilder::convex_hull(&pts) {
                        out.push(b);
                    }
                }
                Collide::Empty => {}
            }
            c += 1;
        }
        // A run the same as one open above extends it; an open one not
        // continued is closed.
        let mut still = Vec::new();
        for &(c0, c1, r0) in &open {
            if let Some(i) = runs.iter().position(|&(a, b)| a == c0 && b == c1) {
                runs.remove(i);
                still.push((c0, c1, r0));
            } else {
                boxed(&mut out, c0, c1, r0, r - 1);
            }
        }
        for (c0, c1) in runs {
            still.push((c0, c1, r));
        }
        still.sort();
        open = still;
    }
    for (c0, c1, r0) in open {
        boxed(&mut out, c0, c1, r0, h - 1);
    }
    out
}
