//! Navigation through the protocol (§12 "Pathfinding and jobs", phase 3
//! item 2): `nav.grids` lists the grids, `nav.grid` draws one as rows of
//! characters, `nav.path` asks for a way from one place to another,
//! `nav.field` draws a flow field, and `nav.bake` writes a grid out as a
//! text index and a binary chunk (§4). Each checks the grid against its
//! sources first, so an answer between ticks sees the last edit.

use super::*;
use argus_nav::grid::STRAIGHT;
use argus_nav::Grid;
use std::rc::Rc;
use std::time::Instant;

/// Rows and columns a drawing shows before it takes every nth cell.
const DRAW_MAX: usize = 120;

impl Daemon {
    fn nav_store(&self) -> Result<Rc<RefCell<argus_nav::NavStore>>> {
        Ok(self.require_session()?.nav.clone())
    }

    fn id_of(&self, e: Entity) -> String {
        match self.world.string_id(e) {
            Some(s) => format!("@{s}"),
            None => e.to_string(),
        }
    }

    /// A place: an entity (`@player`, `e:12`: where it is) or `[x, y]`.
    fn nav_place(&self, params: &Json, key: &str) -> Result<[f32; 2]> {
        match params.get(key) {
            Some(Json::String(r)) => {
                let e = self.world.entity(r)?;
                argus_nav::position(&self.world, e).ok_or_else(|| {
                    bad_params(format!("{key}: {r} has no Transform to stand anywhere"))
                })
            }
            Some(Json::Array(a)) if a.len() == 2 => {
                let n = |v: &Json| v.as_f64().map(|f| f as f32);
                match (n(&a[0]), n(&a[1])) {
                    (Some(x), Some(y)) => Ok([x, y]),
                    _ => Err(bad_params(format!("{key} is [x, y] numbers"))),
                }
            }
            _ => Err(bad_params(format!(
                "{key} is an entity (\"@player\") or a place ([x, y])"
            ))),
        }
    }

    fn nav_grid_param(&self, params: &Json) -> Result<Entity> {
        Ok(match json::opt_str(params, "grid")? {
            Some(r) => self.world.entity(r)?,
            None => Entity::NULL,
        })
    }

    /// The cells of `g` a world rect covers (all of it without one), and
    /// the step that keeps a drawing under [`DRAW_MAX`] a side.
    fn nav_rect(params: &Json, g: &Grid) -> Result<(i64, i64, i64, i64, usize)> {
        let (mut c0, mut r0, mut c1, mut r1) = (0, 0, g.width as i64 - 1, g.height as i64 - 1);
        if let Some(r) = params.get("rect").and_then(Json::as_array) {
            let v: Vec<f32> = r
                .iter()
                .filter_map(|x| x.as_f64().map(|f| f as f32))
                .collect();
            if v.len() != 4 {
                return Err(bad_params("rect is [x0, y0, x1, y1] in world units"));
            }
            let (a, b) = (g.cell_of([v[0], v[1]]), g.cell_of([v[2], v[3]]));
            c0 = a.0.min(b.0).max(0);
            r0 = a.1.min(b.1).max(0);
            c1 = a.0.max(b.0).min(g.width as i64 - 1);
            r1 = a.1.max(b.1).min(g.height as i64 - 1);
        }
        let max = json::u64_param(params, "max", DRAW_MAX as u64)?.max(1) as usize;
        let (w, h) = ((c1 - c0 + 1).max(1) as usize, (r1 - r0 + 1).max(1) as usize);
        let step = w.div_ceil(max).max(h.div_ceil(max)).max(1);
        Ok((c0, r0, c1, r1, step))
    }

    /// `nav.grids`: every grid, what it covers, and what it is built from.
    pub(super) fn nav_grids(&mut self) -> Result<Json> {
        let store = self.nav_store()?;
        let mut st = store.borrow_mut();
        let mut rows = Vec::new();
        for e in st.grids(&self.world) {
            let nav = self.world.get::<argus_nav::NavGrid>(e)?;
            let source = if nav.baked != argus_ecs::StrId::EMPTY {
                format!("baked {}", self.world.strings().resolve(nav.baked))
            } else if !nav.tilemap.is_null() {
                format!("tilemap {}", self.id_of(nav.tilemap))
            } else {
                "rect".to_string()
            };
            match st.grid(&self.world, e, true) {
                Ok(g) => rows.push(json!([
                    self.id_of(e),
                    g.origin,
                    g.cell,
                    [g.width, g.height],
                    g.open_cells(),
                    g.len() - g.open_cells(),
                    source,
                    format!("{:016x}", g.hash),
                ])),
                Err(err) => rows.push(json!([
                    self.id_of(e),
                    null,
                    null,
                    null,
                    null,
                    null,
                    source,
                    err.message
                ])),
            }
        }
        Ok(json!({
            "columns": ["grid", "origin", "cell", "size", "open", "blocked", "from", "hash"],
            "rows": rows,
            "total": rows.len(),
        }))
    }

    /// `nav.grid {grid?, rect?, max?}`: a grid as rows of characters.
    pub(super) fn nav_grid(&mut self, params: &Json) -> Result<Json> {
        let store = self.nav_store()?;
        let mut st = store.borrow_mut();
        let ge = match self.nav_grid_param(params)? {
            e if !e.is_null() => e,
            _ => *st.grids(&self.world).first().ok_or_else(|| {
                ArgusError::new(argus_nav::codes::NO_GRID, "the scene has no NavGrid")
                    .hint("A NavGrid entity makes a grid over a tilemap or a rectangle.")
            })?,
        };
        let g = st.grid(&self.world, ge, true)?;
        let (c0, r0, c1, r1, step) = Self::nav_rect(params, &g)?;
        Ok(json!({
            "grid": self.id_of(ge),
            "origin": g.origin,
            "cell": g.cell,
            "size": [g.width, g.height],
            "first": [c0, r1],
            "step": step,
            "rows": g.rows(c0, r0, c1, r1, step),
            "legend": "top row first; # blocked, . cost 1, 2-9 that cost, + above 9",
        }))
    }

    /// `nav.path {from, to, grid?}`: the way from one place to another.
    pub(super) fn nav_path(&mut self, params: &Json) -> Result<Json> {
        let from = self.nav_place(params, "from")?;
        let to = self.nav_place(params, "to")?;
        let grid = self.nav_grid_param(params)?;
        let store = self.nav_store()?;
        let t = Instant::now();
        let before = store.borrow().counts;
        let walk = store.borrow_mut().walk(&self.world, from, to, grid, true)?;
        let ms = t.elapsed().as_secs_f64() * 1e3;
        let Some(w) = walk else {
            return Ok(json!({ "found": false, "grid": null, "why": "no grid covers from" }));
        };
        let g = store.borrow_mut().grid(&self.world, w.grid, false)?;
        let after = store.borrow().counts;
        let base = json!({
            "grid": self.id_of(w.grid),
            "from_cell": g.col_row(w.from),
            "to_cell": g.col_row(w.to),
            "ms": (ms * 1000.0).round() / 1000.0,
            "expanded": after.expanded - before.expanded,
            "cached": after.searches == before.searches,
        });
        let mut out = base;
        match w.route {
            Some(r) => {
                out["found"] = json!(true);
                out["steps"] = json!(r.cells.len() - 1);
                out["cost"] = json!(r.cost as f64 / STRAIGHT as f64);
                out["points"] = json!(w.points);
            }
            None => {
                out["found"] = json!(false);
                out["why"] = json!(if g.open(w.to) {
                    "the goal cannot be reached from the start"
                } else {
                    "the goal's cell is blocked"
                });
            }
        }
        Ok(out)
    }

    /// `nav.field {of, rect?, max?}`: a NavField entity's flow field as
    /// rows of steps.
    pub(super) fn nav_field(&mut self, params: &Json) -> Result<Json> {
        let of = self.world.entity(json::str_param(params, "of")?)?;
        let store = self.nav_store()?;
        let mut st = store.borrow_mut();
        let Some((g, f)) = st.field_of(&self.world, of, true)? else {
            return Ok(
                json!({ "of": self.id_of(of), "grid": null, "why": "it stands on no grid" }),
            );
        };
        let (c0, r0, c1, r1, step) = Self::nav_rect(params, &g)?;
        let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
        let mut rows = Vec::new();
        let mut r = r1;
        while r >= r0 {
            let mut line = String::new();
            let mut c = c0;
            while c <= c1 {
                line.push(match g.index(c, r) {
                    Some(i) if !g.open(i) && i != f.target => '#',
                    Some(i) => match f.cost(i) {
                        Some(d) => {
                            let s = (d as f32 / STRAIGHT as f32).round() as usize;
                            digits.get(s).map(|&b| b as char).unwrap_or('+')
                        }
                        None => ' ',
                    },
                    None => ' ',
                });
                c += step as i64;
            }
            rows.push(line);
            r -= step as i64;
        }
        Ok(json!({
            "of": self.id_of(of),
            "target": g.col_row(f.target),
            "reached": f.reached,
            "first": [c0, r1],
            "step": step,
            "rows": rows,
            "legend": "top row first; steps to the target 0-9 then a-z, + beyond 35, # blocked, blank unreached",
        }))
    }

    /// `nav.bake {grid, file?}`: write a grid as a text index and a binary
    /// chunk (§4); `NavGrid.baked` naming the index starts from it.
    pub(super) fn nav_bake(&mut self, params: &Json) -> Result<Json> {
        let ge = self.world.entity(json::str_param(params, "grid")?)?;
        let file = match json::opt_str(params, "file")? {
            Some(f) => f.to_string(),
            None => {
                let id = self.world.string_id(ge).ok_or_else(|| {
                    bad_params("the grid has no string id to name the file by; give file")
                })?;
                let s = self.require_session()?;
                let dir = s.project.scene_dir(&s.scene_name);
                let rel = dir
                    .strip_prefix(&s.root)
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|_| format!("scenes/{}", s.scene_name));
                format!("{rel}/{}.navgrid.toml", id.replace('/', "."))
            }
        };
        let root = self.require_session()?.root.clone();
        let store = self.nav_store()?;
        let g = store.borrow_mut().grid(&self.world, ge, true)?;
        let w = argus_nav::bake::write(&root, &file, &g, &self.id_of(ge))?;
        Ok(json!({
            "grid": self.id_of(ge),
            "index": w.index,
            "chunk": w.chunk,
            "bytes": w.bytes,
            "open": w.open,
            "blocked": g.len() - w.open,
            "hash": format!("{:016x}", g.hash),
            "use": format!("set NavGrid.baked = \"{}\" on the grid to start from it", w.index),
        }))
    }
}
