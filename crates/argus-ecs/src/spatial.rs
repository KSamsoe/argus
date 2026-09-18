//! The shared broadphase (§12 "one broadphase: a spatial hash over the live
//! world, rebuilt each tick"). Every entity with a position lands in a
//! cell; `within` visits the cells a circle covers and checks distance
//! exactly, so a query over 100k entities touches a few cells rather than
//! every row (§17 "spatial predicates hit the broadphase hash, not a
//! scan"). `world.query`'s `within`, Luau's `q:within`, and the physics
//! tiers of phase 1 all read the same hash.

use crate::entity::Entity;
use std::collections::HashMap;

/// A uniform grid of cells over entity positions.
#[derive(Clone, Debug)]
pub struct SpatialHash {
    cell: f32,
    cells: HashMap<(i32, i32), Vec<u32>>,
    /// Entity, x, y for every indexed entity, by insertion order.
    entries: Vec<(Entity, f32, f32)>,
    /// Frame the hash was built for.
    frame: u64,
}

impl Default for SpatialHash {
    fn default() -> Self {
        SpatialHash::new(4.0)
    }
}

impl SpatialHash {
    /// Empty hash with the given cell size (world units).
    pub fn new(cell: f32) -> SpatialHash {
        SpatialHash {
            cell: if cell > 0.0 { cell } else { 4.0 },
            cells: HashMap::new(),
            entries: Vec::new(),
            frame: 0,
        }
    }

    /// Cell size.
    pub fn cell(&self) -> f32 {
        self.cell
    }

    /// Frame the hash was built for.
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// Entities indexed.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when nothing is indexed.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn key(&self, x: f32, y: f32) -> (i32, i32) {
        (
            (x / self.cell).floor() as i32,
            (y / self.cell).floor() as i32,
        )
    }

    /// Forget everything and mark the frame.
    pub fn clear(&mut self, frame: u64) {
        for v in self.cells.values_mut() {
            v.clear();
        }
        self.entries.clear();
        self.frame = frame;
    }

    /// Index one entity at a position.
    pub fn insert(&mut self, e: Entity, x: f32, y: f32) {
        let i = self.entries.len() as u32;
        self.entries.push((e, x, y));
        self.cells.entry(self.key(x, y)).or_default().push(i);
    }

    /// Every entity within `r` of `(x, y)`, with its distance, nearest first.
    pub fn within(&self, x: f32, y: f32, r: f32) -> Vec<(Entity, f32)> {
        let mut out = Vec::new();
        if r < 0.0 {
            return out;
        }
        let (x0, y0) = self.key(x - r, y - r);
        let (x1, y1) = self.key(x + r, y + r);
        let r2 = r * r;
        for cx in x0..=x1 {
            for cy in y0..=y1 {
                if let Some(list) = self.cells.get(&(cx, cy)) {
                    for &i in list {
                        let (e, ex, ey) = self.entries[i as usize];
                        let d2 = (ex - x) * (ex - x) + (ey - y) * (ey - y);
                        if d2 <= r2 {
                            out.push((e, d2.sqrt()));
                        }
                    }
                }
            }
        }
        out.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap().then(a.0.cmp(&b.0)));
        out
    }

    /// The nearest indexed entity accepted by `keep`, searching rings of
    /// cells outward up to `max_r`. `None` if nothing qualifies.
    pub fn nearest(
        &self,
        x: f32,
        y: f32,
        max_r: f32,
        mut keep: impl FnMut(Entity) -> bool,
    ) -> Option<(Entity, f32)> {
        let (cx, cy) = self.key(x, y);
        let max_ring = ((max_r / self.cell).ceil() as i32).max(0);
        let mut best: Option<(Entity, f32)> = None;
        for ring in 0..=max_ring {
            // Anything found in an inner ring is closer than anything the
            // next ring can hold once the ring's inner edge exceeds it.
            if let Some((_, d)) = best {
                if (ring as f32 - 1.0) * self.cell > d {
                    break;
                }
            }
            for cx2 in (cx - ring)..=(cx + ring) {
                for cy2 in (cy - ring)..=(cy + ring) {
                    let on_ring = cx2 == cx - ring
                        || cx2 == cx + ring
                        || cy2 == cy - ring
                        || cy2 == cy + ring;
                    if !on_ring {
                        continue;
                    }
                    if let Some(list) = self.cells.get(&(cx2, cy2)) {
                        for &i in list {
                            let (e, ex, ey) = self.entries[i as usize];
                            let d = ((ex - x) * (ex - x) + (ey - y) * (ey - y)).sqrt();
                            if d <= max_r
                                && best.is_none_or(|(be, bd)| d < bd || (d == bd && e < be))
                                && keep(e)
                            {
                                best = Some((e, d));
                            }
                        }
                    }
                }
            }
        }
        best
    }
}
