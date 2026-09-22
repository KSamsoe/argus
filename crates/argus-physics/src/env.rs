//! What a moving body collides with this tick: the tile grids of every
//! Tilemap entity and the solid bodies, and the three questions the mover
//! asks of them: what floor is under me, what wall is ahead, what ceiling
//! is above.
//!
//! World y is up; a tilemap's rows go down from its origin (item 5). Tiles
//! are solid, one-way (blocks only from above), or slopes. A slope is a
//! floor sampled at the body's centre: the body stands where the line is
//! under its middle and walks up and down it, the way tile platformers have
//! always done it. Solid and one-way tiles hold the body across its whole
//! width, so it can stand on a ledge's very edge.

use argus_ecs::Entity;
use argus_tiles::{Collide, MapStore, TileGrid};

/// Distances closer than this are touching, not overlapping.
pub const EPS: f32 = 1e-4;

/// An axis-aligned box, min and max corners.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    /// Left.
    pub x0: f32,
    /// Bottom.
    pub y0: f32,
    /// Right.
    pub x1: f32,
    /// Top.
    pub y1: f32,
}

impl Aabb {
    /// A box from its centre and half extents.
    pub fn around(cx: f32, cy: f32, hw: f32, hh: f32) -> Aabb {
        Aabb {
            x0: cx - hw,
            y0: cy - hh,
            x1: cx + hw,
            y1: cy + hh,
        }
    }
    /// Centre x.
    pub fn cx(&self) -> f32 {
        (self.x0 + self.x1) * 0.5
    }
    /// Centre y.
    pub fn cy(&self) -> f32 {
        (self.y0 + self.y1) * 0.5
    }
    /// Half width.
    pub fn hw(&self) -> f32 {
        (self.x1 - self.x0) * 0.5
    }
    /// Moved by `(dx, dy)`.
    pub fn shift(&self, dx: f32, dy: f32) -> Aabb {
        Aabb {
            x0: self.x0 + dx,
            y0: self.y0 + dy,
            x1: self.x1 + dx,
            y1: self.y1 + dy,
        }
    }
    /// Overlaps `o` by more than touching.
    pub fn overlaps(&self, o: &Aabb) -> bool {
        self.x0 < o.x1 - EPS && o.x0 < self.x1 - EPS && self.y0 < o.y1 - EPS && o.y0 < self.y1 - EPS
    }
    fn x_overlaps(&self, x0: f32, x1: f32) -> bool {
        self.x0 < x1 - EPS && x0 < self.x1 - EPS
    }
}

/// A solid body (a moving platform, a door, a crate) as it stands now.
#[derive(Clone, Copy, Debug)]
pub struct Solid {
    /// The entity.
    pub e: Entity,
    /// Its box.
    pub b: Aabb,
    /// Blocks only from above.
    pub one_way: bool,
    /// Collider layer bits.
    pub layer: u32,
    /// Collider mask bits.
    pub mask: u32,
    /// How far it moved this tick (for riders jumping off it).
    pub moved: [f32; 2],
}

/// What a body stands on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Support {
    /// A tile. `slope` is the floor's rise per unit of x under the centre
    /// (0 on flat ground).
    Tile {
        /// A one-way tile.
        one_way: bool,
        /// Rise per unit x.
        slope: f32,
    },
    /// A solid body, by index into [`Env::solids`].
    Body {
        /// Index.
        idx: usize,
        /// A one-way body.
        one_way: bool,
    },
}

impl Support {
    /// Rise per unit x under the centre.
    pub fn slope(&self) -> f32 {
        match self {
            Support::Tile { slope, .. } => *slope,
            Support::Body { .. } => 0.0,
        }
    }
    /// One-way ground (dropping through it is allowed).
    pub fn one_way(&self) -> bool {
        match self {
            Support::Tile { one_way, .. } | Support::Body { one_way, .. } => *one_way,
        }
    }
}

/// Who is asking: a body's filter and whether it is dropping through
/// one-way platforms.
#[derive(Clone, Copy, Debug)]
pub struct Asker {
    /// The body itself (never collides with itself).
    pub e: Entity,
    /// Collider layer bits.
    pub layer: u32,
    /// Collider mask bits.
    pub mask: u32,
    /// Ignoring one-way platforms.
    pub dropping: bool,
}

impl Asker {
    fn sees(&self, s: &Solid) -> bool {
        s.e != self.e && (self.mask & s.layer) != 0 && (s.mask & self.layer) != 0
    }
}

/// Something in the way of an upward move.
#[derive(Clone, Copy, Debug)]
pub struct Block {
    /// Its underside.
    pub bottom: f32,
    /// Its x extent.
    pub x0: f32,
    /// Its x extent.
    pub x1: f32,
}

/// The world a body moves in this tick.
pub struct Env<'a> {
    /// The tile store.
    pub store: &'a mut MapStore,
    /// Every Tilemap in the world with collision.
    pub grids: &'a [TileGrid],
    /// Every solid body, in entity order.
    pub solids: Vec<Solid>,
}

fn normal(k: Collide) -> Collide {
    match k {
        Collide::Slope { left, right } if left >= 1.0 - EPS && right >= 1.0 - EPS => Collide::Solid,
        Collide::Slope { left, right } if left <= EPS && right <= EPS => Collide::Empty,
        k => k,
    }
}

impl<'a> Env<'a> {
    fn cell(&mut self, g: usize, c: i64, r: i64) -> Collide {
        let grid = self.grids[g];
        normal(self.store.collide(&grid, c, r))
    }

    /// The highest slope surface under `cx` between `lo` and `hi`, with
    /// its rise per unit x.
    pub fn slope_under(&mut self, cx: f32, lo: f32, hi: f32) -> Option<(f32, f32)> {
        let mut best: Option<(f32, f32)> = None;
        for gi in 0..self.grids.len() {
            let g = self.grids[gi];
            let c = g.col(cx);
            let f = (cx - g.left(c)) / g.tile;
            for r in g.row(hi)..=g.row(lo) {
                if let Collide::Slope { left, right } = self.cell(gi, c, r) {
                    let bottom = g.top(r) - g.tile;
                    let s = bottom + (left + (right - left) * f) * g.tile;
                    if s >= lo - EPS && s <= hi + EPS && best.is_none_or(|(b, _)| s > b) {
                        best = Some((s, right - left));
                    }
                }
            }
        }
        best
    }

    /// The highest floor across the box's width between `lo` and `hi`:
    /// solid and one-way tiles and solid bodies. One-way floors count only
    /// when the feet were at or above them before the move (`feet`) and the
    /// asker is not dropping.
    pub fn flat_floor(
        &mut self,
        b: &Aabb,
        lo: f32,
        hi: f32,
        feet: f32,
        who: &Asker,
    ) -> Option<(f32, Support)> {
        let mut best: Option<(f32, Support)> = None;
        let take = |h: f32, s: Support, best: &mut Option<(f32, Support)>| {
            if best.is_none_or(|(bh, _)| h > bh) {
                *best = Some((h, s));
            }
        };
        let one_way_ok = |top: f32| !who.dropping && top <= feet + EPS;
        for gi in 0..self.grids.len() {
            let g = self.grids[gi];
            for c in g.col(b.x0 + EPS)..=g.col(b.x1 - EPS) {
                for r in g.row(hi)..=g.row(lo) {
                    let top = g.top(r);
                    if top < lo - EPS || top > hi + EPS {
                        continue;
                    }
                    match self.cell(gi, c, r) {
                        Collide::Solid => {
                            take(
                                top,
                                Support::Tile {
                                    one_way: false,
                                    slope: 0.0,
                                },
                                &mut best,
                            );
                            break;
                        }
                        Collide::OneWay if one_way_ok(top) => {
                            take(
                                top,
                                Support::Tile {
                                    one_way: true,
                                    slope: 0.0,
                                },
                                &mut best,
                            );
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
        for (i, s) in self.solids.iter().enumerate() {
            if !who.sees(s) || !b.x_overlaps(s.b.x0, s.b.x1) {
                continue;
            }
            let top = s.b.y1;
            if top < lo - EPS || top > hi + EPS || (s.one_way && !one_way_ok(top)) {
                continue;
            }
            take(
                top,
                Support::Body {
                    idx: i,
                    one_way: s.one_way,
                },
                &mut best,
            );
        }
        best
    }

    /// The floor a body stands on if its feet may be anywhere from `lo` to
    /// `hi` (slopes may also be found up to `slope_hi`, which recovers a
    /// body that moved sideways into a slope in the air). Where the centre
    /// is over a slope, the slope is the floor unless a flat floor under
    /// the box rises above it by more than the slope rises across half the
    /// box: that is a real step, not the slope meeting level ground.
    pub fn ground(
        &mut self,
        b: &Aabb,
        lo: f32,
        hi: f32,
        slope_hi: f32,
        feet: f32,
        who: &Asker,
    ) -> Option<(f32, Support)> {
        let s = self.slope_under(b.cx(), lo, slope_hi.max(hi));
        let f = self.flat_floor(b, lo, hi, feet, who);
        match (s, f) {
            (Some((sh, k)), Some((fh, sup))) => {
                if fh > sh + k.abs() * b.hw() + EPS {
                    Some((fh, sup))
                } else {
                    Some((
                        sh,
                        Support::Tile {
                            one_way: false,
                            slope: k,
                        },
                    ))
                }
            }
            (Some((sh, k)), None) => Some((
                sh,
                Support::Tile {
                    one_way: false,
                    slope: k,
                },
            )),
            (None, f) => f,
        }
    }

    /// The lowest ceiling a box rising by `dy` meets, and every block at
    /// that height (for corner correction). Solid and slope tiles block
    /// from below; one-way tiles and bodies do not.
    pub fn ceiling(&mut self, b: &Aabb, dy: f32, who: &Asker) -> Option<(f32, Vec<Block>)> {
        let mut lowest = f32::INFINITY;
        let mut blocks: Vec<Block> = Vec::new();
        let note = |bottom: f32, x0: f32, x1: f32, lowest: &mut f32, blocks: &mut Vec<Block>| {
            if bottom < *lowest - EPS {
                *lowest = bottom;
                blocks.clear();
            }
            if (bottom - *lowest).abs() <= EPS {
                blocks.push(Block { bottom, x0, x1 });
            }
        };
        let (from, to) = (b.y1 - EPS, b.y1 + dy);
        for gi in 0..self.grids.len() {
            let g = self.grids[gi];
            for c in g.col(b.x0 + EPS)..=g.col(b.x1 - EPS) {
                // Bottom-up, so the first block in a column is its lowest.
                for r in (g.row(to)..=g.row(from)).rev() {
                    let bottom = g.top(r) - g.tile;
                    if bottom < from || bottom >= to {
                        continue;
                    }
                    if matches!(self.cell(gi, c, r), Collide::Solid | Collide::Slope { .. }) {
                        note(
                            bottom,
                            g.left(c),
                            g.left(c) + g.tile,
                            &mut lowest,
                            &mut blocks,
                        );
                        break;
                    }
                }
            }
        }
        for s in &self.solids {
            if s.one_way || !who.sees(s) || !b.x_overlaps(s.b.x0, s.b.x1) {
                continue;
            }
            if s.b.y0 >= from && s.b.y0 < to {
                note(s.b.y0, s.b.x0, s.b.x1, &mut lowest, &mut blocks);
            }
        }
        (lowest.is_finite()).then_some((lowest, blocks))
    }

    /// The nearest wall a box moving `dx` sideways meets: the edge it stops
    /// at and how high that wall rises. Anything whose side rises no more
    /// than `tol` above the feet is not a wall (a step, or level ground
    /// meeting a slope the body stands on). A slope's side is as high as
    /// the slope at that edge, so its low end is walked onto, not into.
    pub fn wall(&mut self, b: &Aabb, dx: f32, tol: f32, who: &Asker) -> Option<(f32, f32)> {
        if dx == 0.0 {
            return None;
        }
        let right = dx > 0.0;
        let lead = if right { b.x1 } else { b.x0 };
        let reach = lead + dx;
        let mut best: Option<(f32, f32)> = None;
        // Keep the nearest edge; at the same edge, the highest wall.
        let take = |edge: f32, top: f32, best: &mut Option<(f32, f32)>| {
            *best = match *best {
                Some((be, bt)) if (edge - be).abs() <= EPS => Some((be, bt.max(top))),
                Some(b) if (right && edge > b.0) || (!right && edge < b.0) => Some(b),
                _ => Some((edge, top)),
            };
        };
        for gi in 0..self.grids.len() {
            let g = self.grids[gi];
            // Columns nearest first.
            let (first, last, step) = if right {
                (g.col(lead - EPS), g.col(reach), 1)
            } else {
                (g.col(lead + EPS), g.col(reach), -1)
            };
            let mut c = first - step;
            'cols: while c != last {
                c += step;
                // The edge a box moving this way meets in column c.
                let edge = if right { g.left(c) } else { g.left(c) + g.tile };
                let ahead = if right {
                    edge >= lead - EPS && edge < reach
                } else {
                    edge <= lead + EPS && edge > reach
                };
                if !ahead {
                    continue;
                }
                let mut top_here: Option<f32> = None;
                for r in g.row(b.y1 - EPS)..=g.row(b.y0 + EPS) {
                    let bottom = g.top(r) - g.tile;
                    if bottom >= b.y1 - EPS {
                        continue;
                    }
                    let side = match self.cell(gi, c, r) {
                        Collide::Solid => g.top(r),
                        Collide::Slope { left, right: rr } => {
                            bottom + if right { left } else { rr } * g.tile
                        }
                        _ => continue,
                    };
                    if side > b.y0 + tol + EPS {
                        top_here = Some(top_here.map_or(side, |t: f32| t.max(side)));
                    }
                }
                if let Some(t) = top_here {
                    take(edge, t, &mut best);
                    break 'cols;
                }
            }
        }
        for s in &self.solids {
            if s.one_way || !who.sees(s) {
                continue;
            }
            if s.b.y1 <= b.y0 + tol + EPS || s.b.y0 >= b.y1 - EPS {
                continue;
            }
            let edge = if right { s.b.x0 } else { s.b.x1 };
            let ahead = if right {
                edge >= lead - EPS && edge < reach
            } else {
                edge <= lead + EPS && edge > reach
            };
            if ahead {
                take(edge, s.b.y1, &mut best);
            }
        }
        best
    }
}
