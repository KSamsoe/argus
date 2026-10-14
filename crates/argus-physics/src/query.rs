//! Queries a script asks of the world's shapes (2D staples item 0): what a
//! line meets (`raycast`), where a shape moving along a line would first
//! touch something (`shape_cast`), and what a shape placed somewhere
//! overlaps (`overlap`). They meet every active `Collider` whose layer
//! shares a bit with the filter's mask and every Tilemap's collision cells,
//! as the physics tiers see them: solid cells, slopes (solid under their
//! line), and one-way platforms, which only stop what comes down onto
//! their top. The geometry is `geom`'s: everything is a rounded convex
//! polygon, so the answers are exact and the same on every platform.

use crate::geom::{ray, Contact, Round, V};
use crate::Collider;
use argus_ecs::{Entity, GlobalTransform, Result, Transform, World};
use argus_tiles::{Collide, MapStore, TileGrid, Tilemap};

/// Cells a query looks at in one map before it stops looking: a long
/// shape cast across a large map, which would otherwise visit every cell
/// in the box it sweeps.
pub const MAX_CELLS: usize = 65_536;

/// What a query meets.
#[derive(Clone, Debug, PartialEq)]
pub struct Filter {
    /// Colliders whose layer shares a bit with this.
    pub mask: u32,
    /// Whether colliders count.
    pub colliders: bool,
    /// Whether tile cells count.
    pub tiles: bool,
    /// Whether one-way platforms stop what comes down onto their top
    /// (false: they are not there).
    pub one_way: bool,
    /// Entities left out (the asker itself).
    pub ignore: Vec<Entity>,
}

impl Default for Filter {
    fn default() -> Self {
        Filter {
            mask: u32::MAX,
            colliders: true,
            tiles: true,
            one_way: true,
            ignore: Vec::new(),
        }
    }
}

/// What was met.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Target {
    /// An entity's collider.
    Entity(Entity),
    /// A tile cell of a Tilemap entity: its column and row (rows go down).
    Tile {
        /// The Tilemap entity.
        map: Entity,
        /// Column.
        col: i64,
        /// Row.
        row: i64,
    },
}

/// One hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    /// What it met.
    pub target: Target,
    /// Where they touch.
    pub point: V,
    /// The surface's outward normal there (zero when the query started
    /// inside it).
    pub normal: V,
    /// How far along the line, 0 to 1.
    pub fraction: f32,
    /// How far along the line, in world units.
    pub distance: f32,
    /// Where the moving shape is when it touches (a ray's: the point).
    pub position: V,
}

/// A collider's shape where it is.
pub fn collider_shape(c: &Collider, at: V) -> Round {
    let centre = at + V::new(c.offset.x, c.offset.y);
    if c.radius > 0.0 {
        Round::circle(centre, c.radius)
    } else {
        Round::rect(centre, c.size.x.abs() * 0.5, c.size.y.abs() * 0.5)
    }
}

/// Where an entity is in the world: its GlobalTransform, else its
/// Transform.
pub fn world_position(world: &World, e: Entity) -> Option<V> {
    if let Ok(g) = world.get::<GlobalTransform>(e) {
        return Some(V::new(g.pos.x, g.pos.y));
    }
    world
        .get::<Transform>(e)
        .ok()
        .map(|t| V::new(t.pos.x, t.pos.y))
}

/// Every active collider the filter lets in, in entity order.
fn colliders(world: &World, f: &Filter) -> Vec<(Entity, Round)> {
    let (Some(col), Some(tr)) = (
        world.component_id_of::<Collider>(),
        world.component_id_of::<Transform>(),
    ) else {
        return Vec::new();
    };
    if !f.colliders {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut ents = world.query().has(col).has(tr).entities();
    ents.sort();
    for e in ents {
        if f.ignore.contains(&e) {
            continue;
        }
        let (Ok(c), Some(at)) = (world.get::<Collider>(e), world_position(world, e)) else {
            continue;
        };
        if c.layer & f.mask == 0 {
            continue;
        }
        out.push((e, collider_shape(&c, at)));
    }
    out
}

/// Every active Tilemap with collision, in entity order.
fn grids(world: &World, store: &mut MapStore, f: &Filter) -> Vec<(Entity, TileGrid)> {
    if !f.tiles {
        return Vec::new();
    }
    let Some(tm) = world.component_id_of::<Tilemap>() else {
        return Vec::new();
    };
    let mut ents = world.query().has(tm).entities();
    ents.sort();
    let mut out = Vec::new();
    for e in ents {
        if f.ignore.contains(&e) {
            continue;
        }
        let Ok(t) = world.get::<Tilemap>(e) else {
            continue;
        };
        let file = world.strings().resolve(t.map).to_string();
        if file.is_empty() {
            continue;
        }
        let at = world_position(world, e).unwrap_or_default();
        if let Some(g) = store.grid(&file, [at.x, at.y]) {
            out.push((e, g));
        }
    }
    out
}

/// A cell's shape, and whether it is one-way; none for an empty cell.
fn cell_shape(store: &mut MapStore, g: &TileGrid, c: i64, r: i64) -> Option<(Round, bool)> {
    let (x0, top) = (g.left(c), g.top(r));
    let (x1, bottom) = (x0 + g.tile, top - g.tile);
    let mid = V::new((x0 + x1) * 0.5, (top + bottom) * 0.5);
    let half = g.tile * 0.5;
    match store.collide(g, c, r) {
        Collide::Empty => None,
        Collide::Solid => Some((Round::rect(mid, half, half), false)),
        Collide::OneWay => Some((Round::rect(mid, half, half), true)),
        Collide::Slope { left, right } => {
            if left <= 1e-4 && right <= 1e-4 {
                return None;
            }
            let pts = [
                V::new(x0, bottom),
                V::new(x1, bottom),
                V::new(x1, bottom + right.clamp(0.0, 1.0) * g.tile),
                V::new(x0, bottom + left.clamp(0.0, 1.0) * g.tile),
            ];
            Some((Round::hull(&pts, 0.0), false))
        }
    }
}

/// A one-way platform stops only what comes down onto its top.
fn one_way_ok(c: &Contact, d: V) -> bool {
    d.y < 0.0 && c.normal.y > 0.99
}

/// Every collider and tile cell a line from `from` to `to` meets: the
/// nearest only, unless `all`; nearest first, ties in entity, then map,
/// row, and column order.
pub fn raycast(
    world: &World,
    store: &mut MapStore,
    from: V,
    to: V,
    f: &Filter,
    all: bool,
) -> Result<Vec<Hit>> {
    let d = to - from;
    let len = d.len();
    let mut hits: Vec<Hit> = Vec::new();
    let hit = |target: Target, c: Contact| {
        let p = from + d * c.t;
        Hit {
            target,
            point: p,
            normal: c.normal,
            fraction: c.t,
            distance: c.t * len,
            position: p,
        }
    };
    for (e, s) in colliders(world, f) {
        if let Some(c) = ray(&s, from, d) {
            hits.push(hit(Target::Entity(e), c));
        }
    }
    for (map, g) in grids(world, store, f) {
        walk_cells(&g, from, to, |col, row| {
            let Some((s, one_way)) = cell_shape(store, &g, col, row) else {
                return true;
            };
            if one_way && !f.one_way {
                return true;
            }
            let Some(c) = ray(&s, from, d) else {
                return true;
            };
            if one_way && !one_way_ok(&c, d) {
                return true;
            }
            hits.push(hit(Target::Tile { map, col, row }, c));
            // Cells come nearest first: the first that meets the line is
            // this map's nearest.
            all
        });
    }
    sort(&mut hits);
    if !all {
        hits.truncate(1);
    }
    Ok(hits)
}

/// Where `shape` (around the origin) moving from `from` to `to` first
/// touches a collider or a tile cell; none when it gets there clear.
pub fn shape_cast(
    world: &World,
    store: &mut MapStore,
    shape: &Round,
    from: V,
    to: V,
    f: &Filter,
) -> Result<Option<Hit>> {
    let d = to - from;
    let len = d.len();
    let back = shape.neg();
    let mut hits: Vec<Hit> = Vec::new();
    let hit = |target: Target, c: Contact| {
        let at = from + d * c.t;
        Hit {
            target,
            point: at + support(shape, -c.normal),
            normal: c.normal,
            fraction: c.t,
            distance: c.t * len,
            position: at,
        }
    };
    for (e, s) in colliders(world, f) {
        if let Some(c) = ray(&s.sum(&back), from, d) {
            hits.push(hit(Target::Entity(e), c));
        }
    }
    let (lo0, hi0) = shape.shift(from).bounds();
    let (lo1, hi1) = shape.shift(to).bounds();
    let (lo, hi) = (
        V::new(lo0.x.min(lo1.x), lo0.y.min(lo1.y)),
        V::new(hi0.x.max(hi1.x), hi0.y.max(hi1.y)),
    );
    for (map, g) in grids(world, store, f) {
        for (col, row) in cells_in(&g, lo, hi) {
            let Some((s, one_way)) = cell_shape(store, &g, col, row) else {
                continue;
            };
            if one_way && !f.one_way {
                continue;
            }
            let Some(c) = ray(&s.sum(&back), from, d) else {
                continue;
            };
            if one_way && (!one_way_ok(&c, d) || c.t == 0.0) {
                continue;
            }
            hits.push(hit(Target::Tile { map, col, row }, c));
        }
    }
    sort(&mut hits);
    Ok(hits.into_iter().next())
}

/// Every collider and tile cell `shape` (around the origin) overlaps when
/// it is at `at`. One-way platforms are not solid to a shape that is not
/// moving, so they are left out.
pub fn overlap(
    world: &World,
    store: &mut MapStore,
    shape: &Round,
    at: V,
    f: &Filter,
) -> Result<Vec<Hit>> {
    let placed = shape.shift(at);
    let mut hits: Vec<Hit> = Vec::new();
    let hit = |target: Target| Hit {
        target,
        point: at,
        normal: V::default(),
        fraction: 0.0,
        distance: 0.0,
        position: at,
    };
    for (e, s) in colliders(world, f) {
        if s.overlaps(&placed) {
            hits.push(hit(Target::Entity(e)));
        }
    }
    let (lo, hi) = placed.bounds();
    for (map, g) in grids(world, store, f) {
        for (col, row) in cells_in(&g, lo, hi) {
            if let Some((s, false)) = cell_shape(store, &g, col, row) {
                if s.overlaps(&placed) {
                    hits.push(hit(Target::Tile { map, col, row }));
                }
            }
        }
    }
    sort(&mut hits);
    Ok(hits)
}

/// The point of `s` (around the origin) farthest along `dir`, its radius
/// included.
fn support(s: &Round, dir: V) -> V {
    let mut best = s.pts.first().copied().unwrap_or_default();
    for &p in &s.pts {
        if p.dot(dir) > best.dot(dir) {
            best = p;
        }
    }
    best + dir.unit() * s.r
}

fn sort(hits: &mut [Hit]) {
    hits.sort_by(|a, b| {
        a.fraction
            .total_cmp(&b.fraction)
            .then(a.target.cmp(&b.target))
    });
}

/// The cells of `g` a box from `lo` to `hi` covers, row by row, at most
/// [`MAX_CELLS`].
fn cells_in(g: &TileGrid, lo: V, hi: V) -> Vec<(i64, i64)> {
    let (c0, c1) = (g.col(lo.x).max(0), g.col(hi.x).min(g.width as i64 - 1));
    let (r0, r1) = (g.row(hi.y).max(0), g.row(lo.y).min(g.height as i64 - 1));
    let mut out = Vec::new();
    for r in r0..=r1 {
        for c in c0..=c1 {
            if out.len() >= MAX_CELLS {
                return out;
            }
            out.push((c, r));
        }
    }
    out
}

/// The cells of `g` the line from `from` to `to` passes through, in order
/// from `from` (Amanatides and Woo's walk), each handed to `visit` until it
/// says stop.
fn walk_cells(g: &TileGrid, from: V, to: V, mut visit: impl FnMut(i64, i64) -> bool) {
    // In cells: x right, y down.
    let fx = (from.x - g.origin[0]) / g.tile;
    let fy = (g.origin[1] - from.y) / g.tile;
    let dx = (to.x - from.x) / g.tile;
    let dy = (from.y - to.y) / g.tile;
    let (mut c, mut r) = (fx.floor() as i64, fy.floor() as i64);
    let (sc, sr) = (dx.signum() as i64, dy.signum() as i64);
    let next = |p: f32, cell: i64, step: i64, delta: f32| -> f32 {
        if delta == 0.0 {
            f32::INFINITY
        } else {
            let edge = if step > 0 {
                (cell + 1) as f32
            } else {
                cell as f32
            };
            (edge - p) / delta
        }
    };
    let mut tx = next(fx, c, sc, dx);
    let mut ty = next(fy, r, sr, dy);
    let step_x = if dx == 0.0 {
        f32::INFINITY
    } else {
        1.0 / dx.abs()
    };
    let step_y = if dy == 0.0 {
        f32::INFINITY
    } else {
        1.0 / dy.abs()
    };
    let (w, h) = (g.width as i64, g.height as i64);
    for _ in 0..MAX_CELLS {
        if c >= 0 && r >= 0 && c < w && r < h && !visit(c, r) {
            return;
        }
        if tx > 1.0 && ty > 1.0 {
            return;
        }
        if tx < ty {
            c += sc;
            tx += step_x;
        } else {
            r += sr;
            ty += step_y;
        }
    }
}
