//! The open tilemaps of a session, shared by everything that touches them:
//! the daemon edits and draws through it, and the physics step reads
//! collision from it during the tick, so an edit is solid ground on the next
//! tick (§12 "a human can paint in the editor and both land in the same
//! chunk").
//!
//! Each map's collision layers are bound to a table per tileset (tile id to
//! [`Collide`]), rebuilt when the tilesets re-import, so a cell's collision
//! is two lookups and no string comparison.

use crate::map::{MapFile, SaveReport};
use crate::{table_of, Collide};
use argus_assets::Importer;
use argus_ecs::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

struct Entry {
    map: Option<MapFile>,
    /// The index's modification time when it was read or last saved by us.
    mtime: Option<SystemTime>,
    /// A read failed and nothing asked again since: the tick does not retry.
    failed: bool,
    /// Collision layers: index and table (`None`: every tile is solid).
    layers: Vec<(usize, Option<Rc<Vec<Collide>>>)>,
}

/// A map placed in the world for one tick: its tile (0, 0)'s top-left
/// corner, tile size, and size in tiles. Rows go down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileGrid {
    /// Store handle.
    pub handle: usize,
    /// World position of the top-left corner of tile (0, 0).
    pub origin: [f32; 2],
    /// World units per tile.
    pub tile: f32,
    /// Width in tiles.
    pub width: u32,
    /// Height in tiles.
    pub height: u32,
}

impl TileGrid {
    /// Column holding world `x`.
    pub fn col(&self, x: f32) -> i64 {
        ((x - self.origin[0]) / self.tile).floor() as i64
    }
    /// Row holding world `y`.
    pub fn row(&self, y: f32) -> i64 {
        ((self.origin[1] - y) / self.tile).floor() as i64
    }
    /// World x of column `c`'s left edge.
    pub fn left(&self, c: i64) -> f32 {
        self.origin[0] + c as f32 * self.tile
    }
    /// World y of row `r`'s top edge.
    pub fn top(&self, r: i64) -> f32 {
        self.origin[1] - r as f32 * self.tile
    }
}

/// Open tilemaps by project-relative file.
pub struct MapStore {
    root: PathBuf,
    entries: Vec<Entry>,
    by_file: HashMap<String, usize>,
    tables: HashMap<String, Rc<Vec<Collide>>>,
    /// Moves when the tileset tables are rebuilt.
    tables_revision: u64,
}

fn mtime(p: &Path) -> Option<SystemTime> {
    argus_os::fs::metadata(p).map(|m| m.modified).ok()
}

impl MapStore {
    /// An empty store for the project at `root`.
    pub fn new(root: &Path) -> MapStore {
        MapStore {
            root: root.to_path_buf(),
            entries: Vec::new(),
            by_file: HashMap::new(),
            tables: HashMap::new(),
            tables_revision: crate::map::next_revision(),
        }
    }

    /// The project root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn slot(&mut self, file: &str) -> usize {
        if let Some(&i) = self.by_file.get(file) {
            return i;
        }
        self.entries.push(Entry {
            map: None,
            mtime: None,
            failed: false,
            layers: Vec::new(),
        });
        let i = self.entries.len() - 1;
        self.by_file.insert(file.to_string(), i);
        i
    }

    fn bind(&mut self, i: usize) {
        let tables = &self.tables;
        let e = &mut self.entries[i];
        e.layers = match &e.map {
            Some(m) => m
                .index
                .layers
                .iter()
                .enumerate()
                .filter(|(_, l)| l.collision)
                .map(|(li, l)| (li, l.tileset.as_ref().and_then(|t| tables.get(t).cloned())))
                .collect(),
            None => Vec::new(),
        };
    }

    /// The map, opened now or reopened when its index changed on disk
    /// since it was read (another process saved it).
    pub fn open(&mut self, file: &str) -> Result<&mut MapFile> {
        let now = mtime(&self.root.join(file));
        let i = self.slot(file);
        let e = &self.entries[i];
        if e.map.is_none() || e.mtime != now {
            let read = if crate::tiled::is_tiled(file) {
                crate::tiled::open(&self.root, file)
            } else {
                MapFile::open(&self.root, file)
            };
            match read {
                Ok(m) => {
                    let e = &mut self.entries[i];
                    e.map = Some(m);
                    e.mtime = now;
                    e.failed = false;
                    self.bind(i);
                }
                Err(err) => {
                    let e = &mut self.entries[i];
                    e.map = None;
                    e.failed = true;
                    return Err(err);
                }
            }
        }
        Ok(self.entries[i].map.as_mut().expect("opened"))
    }

    /// A map for use during a tick, as [`MapStore::grid`] has it: opened
    /// on first use, else the one in memory, with no look at the disk. A
    /// script reading a cell at a time went through [`MapStore::open`],
    /// whose look at the file's time took 19 us a call, 15 ms for 800
    /// cells against Delve's 2 ms turn.
    pub fn loaded(&mut self, file: &str) -> Result<&mut MapFile> {
        match self.by_file.get(file) {
            Some(&i) if self.entries[i].map.is_some() => {
                Ok(self.entries[i].map.as_mut().expect("loaded"))
            }
            _ => self.open(file),
        }
    }

    /// Put a map made in this process into the store.
    pub fn insert(&mut self, file: &str, map: MapFile) {
        let i = self.slot(file);
        let now = mtime(&self.root.join(file));
        let e = &mut self.entries[i];
        e.map = Some(map);
        e.mtime = now;
        e.failed = false;
        self.bind(i);
    }

    /// Save a map and remember the index's new modification time, so our
    /// own write is not mistaken for a change from outside.
    pub fn save(&mut self, file: &str) -> Result<SaveReport> {
        let rep = self.open(file)?.save()?;
        let now = mtime(&self.root.join(file));
        if let Some(&i) = self.by_file.get(file) {
            self.entries[i].mtime = now;
        }
        Ok(rep)
    }

    /// Rebuild the tileset tables from the importer's sprites and rebind
    /// every map's collision layers (after an import or a re-import).
    pub fn set_tilesets(&mut self, assets: &Importer) {
        self.tables_revision = crate::map::next_revision();
        self.tables = assets
            .sprites()
            .map(|(name, s)| (name.to_string(), Rc::new(table_of(s))))
            .collect();
        for i in 0..self.entries.len() {
            self.bind(i);
        }
    }

    /// What a map's collision was derived from: its cells' revision and the
    /// tileset tables'. Equal twice, the collision is the same; `None` for a
    /// map not open.
    pub fn revision(&self, file: &str) -> Option<[u64; 2]> {
        let e = &self.entries[*self.by_file.get(file)?];
        Some([e.map.as_ref()?.revision(), self.tables_revision])
    }

    /// A map placed at `origin` for this tick. Opened on first use and
    /// never re-read from disk here (the daemon's [`MapStore::open`] does
    /// that between ticks); `None` if it cannot be opened.
    pub fn grid(&mut self, file: &str, origin: [f32; 2]) -> Option<TileGrid> {
        let i = match self.by_file.get(file) {
            Some(&i) => i,
            None => {
                self.open(file).ok()?;
                self.by_file[file]
            }
        };
        let e = &self.entries[i];
        if e.failed {
            return None;
        }
        let m = e.map.as_ref()?;
        Some(TileGrid {
            handle: i,
            origin,
            tile: m.index.tile_size,
            width: m.index.width,
            height: m.index.height,
        })
    }

    /// The collision of a map's cells inside a world rect (`x0, y0, x1,
    /// y1`, y up), one character a cell ([`Collide::ch`]), top row first.
    /// Past `max` columns or rows it takes every `step`th cell. Returns the
    /// rows, the first cell (column, row), and the step; `None` when the
    /// rect misses the map.
    pub fn collision_rows(
        &mut self,
        g: &TileGrid,
        rect: [f32; 4],
        max: [usize; 2],
    ) -> Option<(Vec<String>, [i64; 2], usize)> {
        let c0 = g.col(rect[0]).max(0);
        let c1 = g.col(rect[2]).min(g.width as i64 - 1);
        let r0 = g.row(rect[3]).max(0);
        let r1 = g.row(rect[1]).min(g.height as i64 - 1);
        if c1 < c0 || r1 < r0 {
            return None;
        }
        let (cols, rows) = ((c1 - c0 + 1) as usize, (r1 - r0 + 1) as usize);
        let step = cols
            .div_ceil(max[0].max(1))
            .max(rows.div_ceil(max[1].max(1)))
            .max(1);
        let mut out = Vec::new();
        let mut r = r0;
        while r <= r1 {
            let mut line = String::new();
            let mut c = c0;
            while c <= c1 {
                line.push(self.collide(g, c, r).ch());
                c += step as i64;
            }
            out.push(line);
            r += step as i64;
        }
        Some((out, [c0, r0], step))
    }

    /// The collision of cell `(c, r)` of a grid: every collision layer
    /// combined by [`Collide::rank`]. Outside the map is empty.
    pub fn collide(&mut self, g: &TileGrid, c: i64, r: i64) -> Collide {
        if c < 0 || r < 0 || c >= g.width as i64 || r >= g.height as i64 {
            return Collide::Empty;
        }
        let Some(Entry {
            map: Some(map),
            layers,
            ..
        }) = self.entries.get_mut(g.handle)
        else {
            return Collide::Empty;
        };
        let mut out = Collide::Empty;
        for (li, table) in layers.iter() {
            let t = map.cell(*li, c as u32, r as u32).unwrap_or(0);
            if t == 0 {
                continue;
            }
            let k = match table {
                Some(tb) => tb.get(t as usize).copied().unwrap_or(Collide::Empty),
                None => Collide::Solid,
            };
            if k == Collide::Solid {
                return k;
            }
            if k.rank() > out.rank() {
                out = k;
            }
        }
        out
    }

    /// The cells of every open runtime map (phase 5 item 4), by file: what
    /// a keyframe, a replay, and a save keep of them.
    pub fn runtime_cells(&self) -> RuntimeCells {
        let mut out: RuntimeCells = Vec::new();
        for (file, &i) in &self.by_file {
            let Some(m) = self.entries[i].map.as_ref().filter(|m| m.index.runtime) else {
                continue;
            };
            let cells = m
                .cells()
                .into_iter()
                .map(|((li, cx, cy), d)| (m.index.layers[li].name.clone(), cx, cy, d))
                .collect();
            out.push((file.clone(), cells));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Put runtime maps' cells back as [`MapStore::runtime_cells`] gave them
    /// (a seek, a load): every open runtime map gets its own, or none.
    pub fn set_runtime_cells(&mut self, cells: &RuntimeCells) {
        for (file, _) in cells {
            let _ = self.open(file);
        }
        for (file, &i) in &self.by_file {
            let Some(m) = self.entries[i].map.as_mut().filter(|m| m.index.runtime) else {
                continue;
            };
            let chunks: crate::map::Chunks = cells
                .iter()
                .find(|(f, _)| f == file)
                .map(|(_, c)| {
                    c.iter()
                        .filter_map(|(name, cx, cy, d)| {
                            m.layer(name).ok().map(|li| ((li, *cx, *cy), d.clone()))
                        })
                        .collect()
                })
                .unwrap_or_default();
            m.set_cells(&chunks);
        }
    }
}

/// Runtime maps' cells: by map file, each chunk by layer name, chunk
/// column, chunk row, and its bytes.
pub type RuntimeCells = Vec<(String, Vec<(String, u32, u32, Vec<u8>)>)>;

/// Write [`RuntimeCells`] (a keyframe's, a save's).
pub fn encode_cells(cells: &RuntimeCells, w: &mut argus_ecs::codec::Writer) {
    w.count(cells.len());
    for (file, chunks) in cells {
        w.str(file);
        w.count(chunks.len());
        for (layer, cx, cy, d) in chunks {
            w.str(layer);
            w.u64(u64::from(*cx));
            w.u64(u64::from(*cy));
            w.bytes(d);
        }
    }
}

/// A digest of runtime maps' cells, or `None` when none holds a tile: a
/// game without them digests as it always did. Part of a run's digest
/// (`argus_player::run_digest`, phase 5 item 4).
pub fn cells_digest(cells: &RuntimeCells) -> Option<u64> {
    let held: RuntimeCells = cells
        .iter()
        .filter(|(_, c)| !c.is_empty())
        .cloned()
        .collect();
    if held.is_empty() {
        return None;
    }
    let mut w = argus_ecs::codec::Writer::new();
    encode_cells(&held, &mut w);
    Some(crate::map::fnv1a64(&w.into_bytes()))
}

/// Read what [`encode_cells`] wrote.
pub fn decode_cells(r: &mut argus_ecs::codec::Reader) -> Result<RuntimeCells> {
    let mut out = Vec::new();
    for _ in 0..r.count()? {
        let file = r.str()?;
        let mut chunks = Vec::new();
        for _ in 0..r.count()? {
            let layer = r.str()?;
            let cx = r.u64()? as u32;
            let cy = r.u64()? as u32;
            chunks.push((layer, cx, cy, r.bytes()?.to_vec()));
        }
        out.push((file, chunks));
    }
    Ok(out)
}
