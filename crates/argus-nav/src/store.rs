//! The session's navigation: the built grids and the searches made on
//! them. A grid is built from its sources when first asked for, and checked
//! again when the frame or the world's structure has moved since (a
//! signature over its `NavGrid`, the tilemap's revision, the baked files,
//! and every `NavCost`); it is rebuilt only when the signature changed.
//! Searches are cached by the grid's hash and the cells, so a cached answer
//! is the answer a fresh search would give.

use crate::grid::{fnv, Grid, FNV, STRAIGHT};
use crate::search::{self, Field, Route, Scratch};
use crate::{bake, codes, NavCost, NavField, NavGrid};
use argus_ecs::{ArgusError, Entity, GlobalTransform, Parent, Result, Transform, World};
use argus_tiles::{Collide, MapStore, Tilemap};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

struct Built {
    frame: u64,
    structure: u64,
    signature: u64,
    grid: Rc<Grid>,
}

/// Cached searches kept, at most, before the cache starts again.
const PATHS_MAX: usize = 16384;
/// Cached flow fields kept, at most.
const FIELDS_MAX: usize = 64;

/// What the store has done (for tests and profiles).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Counts {
    /// Grids built.
    pub builds: u64,
    /// A* searches run (not answered from the cache).
    pub searches: u64,
    /// Cells those searches expanded.
    pub expanded: u64,
    /// Flow fields made.
    pub fields: u64,
}

/// A way from one point to another on a grid.
#[derive(Clone, Debug, PartialEq)]
pub struct Walk {
    /// The grid.
    pub grid: Entity,
    /// The cells it was found between.
    pub from: u32,
    /// The goal's cell.
    pub to: u32,
    /// The path, when there is one.
    pub route: Option<Rc<Route>>,
    /// The waypoints after the start: each cell's centre, and the goal
    /// itself last.
    pub points: Vec<[f32; 2]>,
}

/// The session's navigation grids and searches.
pub struct NavStore {
    root: PathBuf,
    tiles: Option<Rc<RefCell<MapStore>>>,
    built: HashMap<Entity, Built>,
    paths: HashMap<(u64, u32, u32), Option<Rc<Route>>>,
    fields: HashMap<(u64, u32, u32), Rc<Field>>,
    scratch: Scratch,
    /// What it has done.
    pub counts: Counts,
}

/// Where an entity is: a child's world position from its `GlobalTransform`,
/// a root's from its `Transform`.
pub fn position(world: &World, e: Entity) -> Option<[f32; 2]> {
    // Asked first, so no error is made for an entity without them.
    let has = |id: Option<argus_ecs::ComponentId>| id.is_some_and(|c| world.has(e, c));
    if has(world.component_id_of::<Parent>()) {
        if let Ok(g) = world.get::<GlobalTransform>(e) {
            return Some([g.pos.x, g.pos.y]);
        }
    }
    if !has(world.component_id_of::<Transform>()) {
        return None;
    }
    world.get::<Transform>(e).ok().map(|t| [t.pos.x, t.pos.y])
}

fn no_grid(msg: String) -> ArgusError {
    ArgusError::new(codes::NO_GRID, msg)
        .hint("A NavGrid entity makes a grid; nav.grids lists them.")
}

impl NavStore {
    /// A store for the project at `root`, reading tilemaps through `tiles`.
    pub fn new(root: &Path, tiles: Option<Rc<RefCell<MapStore>>>) -> NavStore {
        NavStore {
            root: root.to_path_buf(),
            tiles,
            built: HashMap::new(),
            paths: HashMap::new(),
            fields: HashMap::new(),
            scratch: Scratch::default(),
            counts: Counts::default(),
        }
    }

    /// The `NavGrid` entities, in entity order.
    pub fn grids(&self, world: &World) -> Vec<Entity> {
        let Some(id) = world.component_id_of::<NavGrid>() else {
            return Vec::new();
        };
        let mut v = world.query().has(id).entities();
        v.sort();
        v
    }

    /// The built grid of `e`, current: built now, or checked against its
    /// sources when the frame or the world's structure moved since (always,
    /// with `force`: a protocol call between ticks, after an edit).
    pub fn grid(&mut self, world: &World, e: Entity, force: bool) -> Result<Rc<Grid>> {
        if !force {
            if let Some(b) = self.built.get(&e) {
                if b.frame == world.frame() && b.structure == world.structure() {
                    return Ok(b.grid.clone());
                }
            }
        }
        let nav = world
            .get::<NavGrid>(e)
            .map_err(|_| no_grid(format!("{e} has no NavGrid")))?;
        let signature = self.signature(world, &nav);
        if let Some(b) = self.built.get_mut(&e) {
            if b.signature == signature {
                b.frame = world.frame();
                b.structure = world.structure();
                return Ok(b.grid.clone());
            }
        }
        let grid = Rc::new(self.build(world, &nav)?);
        self.counts.builds += 1;
        self.built.insert(
            e,
            Built {
                frame: world.frame(),
                structure: world.structure(),
                signature,
                grid: grid.clone(),
            },
        );
        Ok(grid)
    }

    /// The grid `explicit` names, else the first grid (in entity order)
    /// that `at` is on; none when no grid covers it.
    pub fn grid_for(
        &mut self,
        world: &World,
        at: [f32; 2],
        explicit: Entity,
        force: bool,
    ) -> Result<Option<(Entity, Rc<Grid>)>> {
        if !explicit.is_null() {
            return Ok(Some((explicit, self.grid(world, explicit, force)?)));
        }
        for e in self.grids(world) {
            let g = self.grid(world, e, force)?;
            if g.contains(at) {
                return Ok(Some((e, g)));
            }
        }
        Ok(None)
    }

    /// What a grid is built from, as one number: equal, the same grid.
    fn signature(&mut self, world: &World, nav: &NavGrid) -> u64 {
        let mut h = FNV;
        for v in [nav.origin.x, nav.origin.y, nav.cell] {
            h = fnv(h, &v.to_le_bytes());
        }
        h = fnv(h, &nav.width.to_le_bytes());
        h = fnv(h, &nav.height.to_le_bytes());
        h = fnv(h, &[nav.fill, nav.diagonal as u8]);
        h = fnv(h, &nav.tilemap.to_bits().to_le_bytes());
        if !nav.tilemap.is_null() {
            if let (Some((file, origin)), Some(tiles)) =
                (tilemap_of(world, nav.tilemap), &self.tiles)
            {
                let mut t = tiles.borrow_mut();
                let _ = t.grid(&file, origin);
                h = fnv(h, file.as_bytes());
                h = fnv(h, &origin[0].to_le_bytes());
                h = fnv(h, &origin[1].to_le_bytes());
                for r in t.revision(&file).unwrap_or([0, 0]) {
                    h = fnv(h, &r.to_le_bytes());
                }
            }
        }
        let baked = world.strings().resolve(nav.baked).to_string();
        if !baked.is_empty() {
            h = fnv(h, baked.as_bytes());
            for f in [baked.clone(), bake::chunk_of(&baked)] {
                if let Ok(m) = argus_os::fs::metadata(self.root.join(f)) {
                    h = fnv(h, &m.len.to_le_bytes());
                    h = fnv(h, format!("{:?}", m.modified).as_bytes());
                }
            }
        }
        if let Some(id) = world.component_id_of::<NavCost>() {
            let _ = world.query().has(id).for_each_view(|v| {
                let costs = v.column::<NavCost>()?;
                let places = v.column::<Transform>().ok();
                for (k, e) in v.entities().iter().enumerate() {
                    let Some(p) = places.as_ref().map(|t| t[k].pos) else {
                        continue;
                    };
                    let c = costs[k];
                    h = fnv(h, &e.to_bits().to_le_bytes());
                    for f in [p.x, p.y, c.size.x, c.size.y] {
                        h = fnv(h, &f.to_le_bytes());
                    }
                    h = fnv(h, &[c.cost]);
                }
                Ok(())
            });
        }
        h
    }

    fn build(&mut self, world: &World, nav: &NavGrid) -> Result<Grid> {
        let baked = world.strings().resolve(nav.baked).to_string();
        let mut g = if !baked.is_empty() {
            let mut g = bake::read(&self.root, &baked)?;
            g.diagonal = nav.diagonal;
            g
        } else if !nav.tilemap.is_null() {
            let (file, origin) = tilemap_of(world, nav.tilemap).ok_or_else(|| {
                no_grid(format!(
                    "the grid's tilemap {} has no Tilemap and Transform",
                    nav.tilemap
                ))
            })?;
            let tiles = self
                .tiles
                .as_ref()
                .ok_or_else(|| no_grid("no tilemaps here".into()))?;
            let mut t = tiles.borrow_mut();
            let tg = t
                .grid(&file, origin)
                .ok_or_else(|| no_grid(format!("the tilemap {file} did not open")))?;
            let (w, h) = (tg.width, tg.height);
            let mut g = Grid::new(
                [tg.origin[0], tg.origin[1] - h as f32 * tg.tile],
                tg.tile,
                w,
                h,
                nav.diagonal,
                nav.fill,
            );
            // Rows go down in a tilemap and up here.
            for r in 0..h as i64 {
                for c in 0..w as i64 {
                    match t.collide(&tg, c, h as i64 - 1 - r) {
                        Collide::Empty | Collide::OneWay => {}
                        _ => g.cost[(r as u32 * w + c as u32) as usize] = 0,
                    }
                }
            }
            g
        } else {
            Grid::new(
                [nav.origin.x, nav.origin.y],
                nav.cell,
                nav.width,
                nav.height,
                nav.diagonal,
                nav.fill,
            )
        };
        // The entities over it: a block wins, else the highest cost.
        if let Some(id) = world.component_id_of::<NavCost>() {
            let mut over: Vec<i16> = vec![-1; g.len()];
            let mut any = false;
            let _ = world.query().has(id).for_each_view(|v| {
                let costs = v.column::<NavCost>()?;
                let places = v.column::<Transform>().ok();
                for k in 0..v.entities().len() {
                    let Some(p) = places.as_ref().map(|t| t[k].pos) else {
                        continue;
                    };
                    let c = costs[k];
                    let mut set = |i: u32| {
                        let o = &mut over[i as usize];
                        if c.cost == 0 {
                            *o = 0;
                        } else if *o != 0 {
                            *o = (*o).max(c.cost as i16);
                        }
                        any = true;
                    };
                    if c.size.x <= 0.0 && c.size.y <= 0.0 {
                        if let Some(i) = g.locate([p.x, p.y]) {
                            set(i);
                        }
                        continue;
                    }
                    // Centres in [low edge, high edge): a box two cells
                    // wide covers two cells, wherever it sits.
                    let lo = |v: f32, o: f32| ((v - o) / g.cell - 0.5).ceil() as i64;
                    let hi = |v: f32, o: f32| ((v - o) / g.cell - 0.5).ceil() as i64 - 1;
                    let (hx, hy) = (c.size.x.max(0.0) * 0.5, c.size.y.max(0.0) * 0.5);
                    let (c0, c1) = (
                        lo(p.x - hx, g.origin[0]).max(0),
                        hi(p.x + hx, g.origin[0]).min(g.width as i64 - 1),
                    );
                    let (r0, r1) = (
                        lo(p.y - hy, g.origin[1]).max(0),
                        hi(p.y + hy, g.origin[1]).min(g.height as i64 - 1),
                    );
                    for r in r0..=r1 {
                        for cc in c0..=c1 {
                            if let Some(i) = g.index(cc, r) {
                                set(i);
                            }
                        }
                    }
                }
                Ok(())
            });
            if any {
                for (i, o) in over.into_iter().enumerate() {
                    if o >= 0 {
                        g.cost[i] = o as u8;
                    }
                }
            }
        }
        g.seal();
        Ok(g)
    }

    /// A* on `grid` from one cell to another, from the cache when it was
    /// asked before.
    pub fn path(&mut self, grid: &Grid, from: u32, to: u32) -> Option<Rc<Route>> {
        let key = (grid.hash, from, to);
        if let Some(r) = self.paths.get(&key) {
            return r.clone();
        }
        let r = search::astar(grid, from, to, &mut self.scratch).map(Rc::new);
        self.counts.searches += 1;
        self.counts.expanded += r.as_ref().map_or(0, |r| r.expanded as u64);
        if self.paths.len() >= PATHS_MAX {
            self.paths.clear();
        }
        self.paths.insert(key, r.clone());
        r
    }

    /// The flow field toward a cell of `grid`, out to `range` cost units,
    /// from the cache when it was made before.
    pub fn field(&mut self, grid: &Grid, target: u32, range: u32) -> Rc<Field> {
        let key = (grid.hash, target, range);
        if let Some(f) = self.fields.get(&key) {
            return f.clone();
        }
        let f = Rc::new(search::flow(grid, target, range));
        self.counts.fields += 1;
        if self.fields.len() >= FIELDS_MAX {
            self.fields.clear();
        }
        self.fields.insert(key, f.clone());
        f
    }

    /// The flow field of a `NavField` entity: toward the cell it stands in,
    /// on its grid. None when it stands on no grid.
    pub fn field_of(
        &mut self,
        world: &World,
        e: Entity,
        force: bool,
    ) -> Result<Option<(Rc<Grid>, Rc<Field>)>> {
        let nf = world.get::<NavField>(e).map_err(|_| {
            ArgusError::new(codes::NO_GRID, format!("{e} has no NavField"))
                .hint("A NavField on an entity makes a flow field toward it.")
        })?;
        let Some(at) = position(world, e) else {
            return Ok(None);
        };
        let Some((_, g)) = self.grid_for(world, at, nf.grid, force)? else {
            return Ok(None);
        };
        let Some(target) = g.locate(at) else {
            return Ok(None);
        };
        let range = (nf.range.max(0.0) * STRAIGHT as f32).round() as u32;
        let f = self.field(&g, target, range);
        Ok(Some((g, f)))
    }

    /// The way from one point to another: on `grid`, or the grid `from` is
    /// on. None when no grid covers `from`; an error when `to` is off the
    /// grid.
    pub fn walk(
        &mut self,
        world: &World,
        from: [f32; 2],
        to: [f32; 2],
        grid: Entity,
        force: bool,
    ) -> Result<Option<Walk>> {
        let Some((ge, g)) = self.grid_for(world, from, grid, force)? else {
            return Ok(None);
        };
        let off = |what: &str, p: [f32; 2]| {
            ArgusError::new(
                codes::OFF_GRID,
                format!("{what} ({}, {}) is off the grid {ge}", p[0], p[1]),
            )
        };
        let a = g.locate(from).ok_or_else(|| off("the start", from))?;
        let b = g.locate(to).ok_or_else(|| off("the goal", to))?;
        let route = self.path(&g, a, b);
        let points = route
            .as_ref()
            .map(|r| waypoints(&g, &r.cells, to))
            .unwrap_or_default();
        Ok(Some(Walk {
            grid: ge,
            from: a,
            to: b,
            route,
            points,
        }))
    }
}

/// The waypoints of a path's cells: each cell's centre after the start,
/// the goal point itself last (the start's own cell alone: just the goal).
pub fn waypoints(g: &Grid, cells: &[u32], goal: [f32; 2]) -> Vec<[f32; 2]> {
    let mut pts: Vec<[f32; 2]> = cells.iter().skip(1).map(|&c| g.centre(c)).collect();
    match pts.last_mut() {
        Some(last) => *last = goal,
        None => pts.push(goal),
    }
    pts
}

/// A `Tilemap` entity's file and where tile (0, 0)'s top-left corner is.
fn tilemap_of(world: &World, e: Entity) -> Option<(String, [f32; 2])> {
    let tm = world.get::<Tilemap>(e).ok()?;
    let at = position(world, e)?;
    Some((world.strings().resolve(tm.map).to_string(), at))
}
