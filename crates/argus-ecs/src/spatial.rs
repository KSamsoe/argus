//! The shared broadphase (§12 "one broadphase: a spatial hash over the live
//! world, rebuilt each tick"). Every entity with a position lands in a
//! cell; `within` visits the cells a circle covers and checks distance
//! exactly, so a query over 100k entities touches a few cells rather than
//! every row (§17 "spatial predicates hit the broadphase hash, not a
//! scan"). `world.query`'s `within`, Luau's `q:within`, and the physics
//! tiers of phase 1 all read the same hash.
//!
//! The hash is kept up to date in place (§12 "rebuilt incrementally",
//! phase 5 item 5). A pass over the world's positions writes each entity's
//! position into its slot; only the entities that changed cell (or arrived,
//! or left) touch the cells, and when many did, the cells are refilled in
//! one sweep instead. Each cell lists its entities by index, so the hash is
//! a function of where things are, not of the order they arrived in: a
//! world restored from a snapshot and built from nothing visits candidates
//! in the same order as the run it came from.

use crate::entity::Entity;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// One indexed entity, by entity index.
#[derive(Clone, Copy, Debug)]
struct Slot {
    /// The entity there, `Entity::NULL` when the slot is free.
    entity: Entity,
    x: f32,
    y: f32,
    key: (i32, i32),
    /// The pass that last saw it.
    stamp: u32,
}

const FREE: Slot = Slot {
    entity: Entity::NULL,
    x: 0.0,
    y: 0.0,
    key: (0, 0),
    stamp: 0,
};

/// A cell key's hash: a rotate and a multiply per coordinate, where the
/// standard hasher spent most of a whole rebuild.
#[derive(Default)]
struct CellHasher(u64);

impl Hasher for CellHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }
    fn write_i32(&mut self, n: i32) {
        self.write_u64(n as u32 as u64);
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

type Cells = HashMap<(i32, i32), CellList, BuildHasherDefault<CellHasher>>;

/// Entities a cell holds in place before it allocates.
const INLINE: usize = 6;

/// A cell's entity indices, ascending. Most cells hold a few, so they live
/// in the map's entry: building a hash from nothing, or cloning it with a
/// snapshot, allocated once per cell when every cell was a `Vec`.
#[derive(Clone, Debug)]
enum CellList {
    Inline(u8, [u32; INLINE]),
    Heap(Vec<u32>),
}

impl Default for CellList {
    fn default() -> Self {
        CellList::Inline(0, [0; INLINE])
    }
}

impl std::ops::Deref for CellList {
    type Target = [u32];
    fn deref(&self) -> &[u32] {
        match self {
            CellList::Inline(n, items) => &items[..*n as usize],
            CellList::Heap(v) => v,
        }
    }
}

impl CellList {
    fn clear(&mut self) {
        match self {
            CellList::Inline(n, _) => *n = 0,
            CellList::Heap(v) => v.clear(),
        }
    }

    /// The list on the heap, with room for one more.
    fn spill(&mut self) -> &mut Vec<u32> {
        if let CellList::Inline(n, items) = self {
            let mut v = Vec::with_capacity(INLINE * 2);
            v.extend_from_slice(&items[..*n as usize]);
            *self = CellList::Heap(v);
        }
        match self {
            CellList::Heap(v) => v,
            CellList::Inline(..) => unreachable!(),
        }
    }

    fn push(&mut self, ix: u32) {
        self.insert(self.len(), ix);
    }

    fn insert(&mut self, at: usize, ix: u32) {
        match self {
            CellList::Inline(n, items) if (*n as usize) < INLINE => {
                let len = *n as usize;
                items.copy_within(at..len, at + 1);
                items[at] = ix;
                *n += 1;
            }
            _ => self.spill().insert(at, ix),
        }
    }

    fn remove(&mut self, at: usize) {
        match self {
            CellList::Inline(n, items) => {
                let len = *n as usize;
                items.copy_within(at + 1..len, at);
                *n -= 1;
            }
            CellList::Heap(v) => {
                v.remove(at);
            }
        }
    }
}

/// A change a pass found, applied by [`SpatialHash::finish`].
#[derive(Clone, Copy, Debug)]
struct Move {
    ix: u32,
    /// The cell it left, if it was in one.
    from: Option<(i32, i32)>,
    /// The cell it entered, if it is still indexed.
    to: Option<(i32, i32)>,
}

/// A uniform grid of cells over entity positions.
#[derive(Clone, Debug)]
pub struct SpatialHash {
    cell: f32,
    inv: f32,
    /// Each cell's entity indices, ascending.
    cells: Cells,
    /// Where each indexed entity is, by entity index.
    slots: Vec<Slot>,
    len: usize,
    /// The current pass ([`SpatialHash::begin`]).
    stamp: u32,
    /// Entities this pass placed.
    placed: usize,
    /// Cell changes this pass found.
    moves: Vec<Move>,
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
        let cell = if cell > 0.0 { cell } else { 4.0 };
        SpatialHash {
            cell,
            inv: 1.0 / cell,
            cells: Cells::default(),
            slots: Vec::new(),
            len: 0,
            stamp: 0,
            placed: 0,
            moves: Vec::new(),
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
        self.len
    }

    /// True when nothing is indexed.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The cell a point is in. Monotonic in each coordinate, which is what
    /// a query's box of cells relies on; `floor` without SSE4.1 is a call.
    #[inline]
    fn key(&self, x: f32, y: f32) -> (i32, i32) {
        #[inline]
        fn floor(q: f32) -> i32 {
            let i = q as i32;
            if (i as f32) > q {
                i.saturating_sub(1)
            } else {
                i
            }
        }
        (floor(x * self.inv), floor(y * self.inv))
    }

    /// Make room for `n` entities spread over about as many cells: a pass
    /// over a world that was never indexed.
    pub fn reserve(&mut self, n: usize) {
        self.slots.reserve(n.saturating_sub(self.slots.len()));
        self.cells.reserve(n.saturating_sub(self.cells.len()));
    }

    /// Forget everything and mark the frame.
    pub fn clear(&mut self, frame: u64) {
        for v in self.cells.values_mut() {
            v.clear();
        }
        self.slots.clear();
        self.moves.clear();
        self.len = 0;
        self.placed = 0;
        self.frame = frame;
    }

    /// Start a pass that will [`place`](SpatialHash::place) every entity
    /// with a position; [`finish`](SpatialHash::finish) then applies what
    /// changed and drops the ones it did not see.
    pub fn begin(&mut self, frame: u64) {
        self.stamp = self.stamp.wrapping_add(1);
        if self.stamp == 0 {
            // Every slot's stamp is from an old pass; none may look current.
            for s in &mut self.slots {
                s.stamp = 0;
            }
            self.stamp = 1;
        }
        self.placed = 0;
        self.moves.clear();
        self.frame = frame;
    }

    /// Record one entity's position in this pass: in place when it stays in
    /// its cell, otherwise a move [`finish`](SpatialHash::finish) applies.
    #[inline]
    pub fn place(&mut self, e: Entity, x: f32, y: f32) {
        let ix = e.index() as usize;
        if self.slots.len() <= ix {
            self.slots.resize(ix + 1, FREE);
        }
        let key = self.key(x, y);
        let stamp = self.stamp;
        let s = &mut self.slots[ix];
        let first = s.stamp != stamp;
        if s.entity == Entity::NULL {
            self.len += 1;
            self.moves.push(Move {
                ix: ix as u32,
                from: None,
                to: Some(key),
            });
        } else if s.key != key {
            // A move, or the index reused by a new entity: either way the
            // index leaves one cell for another.
            self.moves.push(Move {
                ix: ix as u32,
                from: Some(s.key),
                to: Some(key),
            });
        }
        *s = Slot {
            entity: e,
            x,
            y,
            key,
            stamp,
        };
        if first {
            self.placed += 1;
        }
    }

    /// End the pass: drop every entity it did not place, and bring the
    /// cells up to date. Returns how many were dropped.
    pub fn finish(&mut self) -> usize {
        let mut dropped = 0;
        if self.placed < self.len {
            for ix in 0..self.slots.len() {
                let s = self.slots[ix];
                if s.entity != Entity::NULL && s.stamp != self.stamp {
                    self.moves.push(Move {
                        ix: ix as u32,
                        from: Some(s.key),
                        to: None,
                    });
                    self.slots[ix] = FREE;
                    dropped += 1;
                }
            }
            self.len -= dropped;
        }
        if self.moves.len() > self.len / 8 {
            // Many changed: refill every cell from the slots, in index
            // order, which is each cell's order.
            for v in self.cells.values_mut() {
                v.clear();
            }
            for (ix, s) in self.slots.iter().enumerate() {
                if s.entity != Entity::NULL {
                    self.cells.entry(s.key).or_default().push(ix as u32);
                }
            }
        } else {
            for m in std::mem::take(&mut self.moves) {
                if let Some(from) = m.from {
                    if let Some(list) = self.cells.get_mut(&from) {
                        if let Ok(at) = list.binary_search(&m.ix) {
                            list.remove(at);
                        }
                    }
                }
                if let Some(to) = m.to {
                    let list = self.cells.entry(to).or_default();
                    let at = list.partition_point(|&o| o < m.ix);
                    list.insert(at, m.ix);
                }
            }
        }
        self.moves.clear();
        dropped
    }

    /// Index one entity at a position, or move it there: a pass of one
    /// that keeps everything else.
    pub fn insert(&mut self, e: Entity, x: f32, y: f32) {
        let placed = self.placed;
        self.place(e, x, y);
        // No sweep: the rest stay.
        self.placed = self.len;
        self.finish();
        self.placed = placed;
    }

    /// Visit the indexed entities in the cells a circle's box covers, with
    /// their positions, cell by cell and by index within a cell.
    #[inline]
    fn scan(&self, x: f32, y: f32, r: f32, mut f: impl FnMut(Entity, f32, f32)) {
        let (x0, y0) = self.key(x - r, y - r);
        let (x1, y1) = self.key(x + r, y + r);
        for cx in x0..=x1 {
            for cy in y0..=y1 {
                if let Some(list) = self.cells.get(&(cx, cy)) {
                    for &ix in list.iter() {
                        let s = &self.slots[ix as usize];
                        f(s.entity, s.x, s.y);
                    }
                }
            }
        }
    }

    /// Every entity within `r` of `(x, y)`, with its distance, nearest first.
    pub fn within(&self, x: f32, y: f32, r: f32) -> Vec<(Entity, f32)> {
        let mut out = Vec::new();
        if r < 0.0 {
            return out;
        }
        let r2 = r * r;
        self.scan(x, y, r, |e, ex, ey| {
            let d2 = (ex - x) * (ex - x) + (ey - y) * (ey - y);
            if d2 <= r2 {
                out.push((e, d2.sqrt()));
            }
        });
        out.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap().then(a.0.cmp(&b.0)));
        out
    }

    /// Call `f` with every entity within `r` of `(x, y)` and its position,
    /// without allocating: the physics tiers' contact pass, which runs over
    /// every collider every tick. Cells in order, entities by index within
    /// a cell.
    pub fn for_each_within(&self, x: f32, y: f32, r: f32, mut f: impl FnMut(Entity, f32, f32)) {
        if r < 0.0 {
            return;
        }
        let r2 = r * r;
        self.scan(x, y, r, |e, ex, ey| {
            if (ex - x) * (ex - x) + (ey - y) * (ey - y) <= r2 {
                f(e, ex, ey);
            }
        });
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
                        for &ix in list.iter() {
                            let s = &self.slots[ix as usize];
                            let e = s.entity;
                            let d = ((s.x - x) * (s.x - x) + (s.y - y) * (s.y - y)).sqrt();
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
