//! Chunked storage for a tilemap (§4 "bulk authored data", §17 "4,096 ×
//! 4,096 tiles, 8 layers: chunked storage, rect edits, and auto-tiling at
//! that size without pauses").
//!
//! ```toml
//! # scenes/hollow/rooms/a/tiles.tilemap.toml
//! kind      = "tilemap"
//! width     = 4096            # tiles
//! height    = 4096
//! tile_size = 1.0             # world units per tile
//! chunk     = 64              # tiles per chunk side
//! cell      = 2               # bytes per cell: 1, 2, or 4
//! chunks    = "tiles.chunks"  # folder beside the index
//! hash      = "fnv1a64:..."   # of the chunk manifest; checked on open
//!
//! [[layers]]
//! name      = "ground"
//! tileset   = "tiles/cave"    # a sprite asset: tile n is frame n - 1, 0 is empty
//! z         = 0.0
//! collision = true            # its tiles' solid / one_way properties collide
//! ```
//!
//! The folder holds one file per non-empty chunk, `<layer>/<cx>_<cy>.bin`
//! (little-endian cells, row-major, `chunk * chunk` of them), and
//! `manifest.txt` with every chunk's hash. A chunk loads the first time a
//! cell in it is read; an empty chunk is never written; a save rewrites
//! only the chunks that changed. Rows go down (row 0 is the top), as a
//! tile editor shows them.

use argus_ecs::{ArgusError, Result};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// A rectangle of cells, `x`/`y` at the top-left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    /// Left column.
    pub x: u32,
    /// Top row.
    pub y: u32,
    /// Width in cells.
    pub w: u32,
    /// Height in cells.
    pub h: u32,
}

impl Rect {
    /// Construct.
    pub fn new(x: u32, y: u32, w: u32, h: u32) -> Rect {
        Rect { x, y, w, h }
    }
    /// Cells covered.
    pub fn area(&self) -> usize {
        self.w as usize * self.h as usize
    }
    /// Grown by `n` on every side, clipped to a `width` x `height` grid.
    pub fn grow(&self, n: u32, width: u32, height: u32) -> Rect {
        let x0 = self.x.saturating_sub(n);
        let y0 = self.y.saturating_sub(n);
        let x1 = (self.x + self.w + n).min(width);
        let y1 = (self.y + self.h + n).min(height);
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }
}

/// One layer.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerDef {
    /// Name.
    pub name: String,
    /// The tileset: a sprite asset name.
    pub tileset: Option<String>,
    /// Draw order.
    pub z: f32,
    /// Whether its tiles collide.
    pub collision: bool,
}

/// The text index.
#[derive(Clone, Debug, PartialEq)]
pub struct MapIndex {
    /// Columns.
    pub width: u32,
    /// Rows.
    pub height: u32,
    /// World units per tile.
    pub tile_size: f32,
    /// Tiles per chunk side.
    pub chunk: u32,
    /// Bytes per cell.
    pub cell: u8,
    /// Chunk folder, relative to the index's folder.
    pub chunks: String,
    /// `fnv1a64:<hex>` of the manifest.
    pub hash: String,
    /// Layers, drawn in `z` order.
    pub layers: Vec<LayerDef>,
}

/// 64-bit FNV-1a.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

type Key = (usize, u32, u32);

#[derive(Clone, Debug)]
struct Chunk {
    data: Vec<u8>,
    dirty: bool,
}

/// What a save wrote.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SaveReport {
    /// Chunk files written.
    pub written: usize,
    /// Chunk files removed (they became empty).
    pub removed: usize,
    /// Bytes written, chunks plus manifest and index.
    pub bytes: usize,
    /// Files written or removed, project-relative.
    pub files: Vec<String>,
}

fn e(code: u32, m: impl Into<String>) -> ArgusError {
    ArgusError::new(code, m.into())
}

/// An open tilemap: its index, its manifest, and the chunks read so far.
#[derive(Clone, Debug)]
pub struct MapFile {
    root: PathBuf,
    rel: String,
    /// The index.
    pub index: MapIndex,
    manifest: BTreeMap<Key, u64>,
    loaded: HashMap<Key, Chunk>,
    /// See [`MapFile::revision`].
    revision: u64,
}

/// The next [`MapFile::revision`]: never repeats in a process.
static REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub(crate) fn next_revision() -> u64 {
    REVISION.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl MapFile {
    fn index_path(&self) -> PathBuf {
        self.root.join(&self.rel)
    }

    fn chunk_dir(&self) -> PathBuf {
        self.index_path()
            .parent()
            .unwrap_or(Path::new("."))
            .join(&self.index.chunks)
    }

    fn chunk_rel(&self, k: Key) -> String {
        let dir = Path::new(&self.rel)
            .parent()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let base = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        format!(
            "{base}{}/{}/{}_{}.bin",
            self.index.chunks, self.index.layers[k.0].name, k.1, k.2
        )
    }

    /// Refuse a layer or chunk-folder name that is not one plain path
    /// segment: both become paths on disk, and a hand-edited index must not
    /// send writes outside the map's folder.
    fn check_names(&self) -> std::result::Result<(), String> {
        let plain = |s: &str| {
            !s.is_empty()
                && s.len() <= 128
                && !s.starts_with('.')
                && !s
                    .chars()
                    .any(|c| c.is_control() || "/\\:<>\"|?*".contains(c))
        };
        if !plain(&self.index.chunks) {
            return Err(format!(
                "chunks = {:?} must be one folder name beside the index",
                self.index.chunks
            ));
        }
        for l in &self.index.layers {
            if !plain(&l.name) {
                return Err(format!(
                    "layer name {:?} must be a plain folder name (no slashes, no leading dot)",
                    l.name
                ));
            }
        }
        Ok(())
    }

    /// Whether `rel` is a tilemap index (`kind = "tilemap"`).
    pub fn is_tilemap(root: &Path, rel: &str) -> bool {
        argus_os::fs::read_to_string(root.join(rel))
            .ok()
            .and_then(|t| t.parse::<toml_edit::DocumentMut>().ok())
            .and_then(|d| {
                d.get("kind")
                    .and_then(|k| k.as_str())
                    .map(|k| k == "tilemap")
            })
            .unwrap_or(false)
    }

    /// Create a new, empty tilemap and write its index and manifest.
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        root: &Path,
        rel: &str,
        width: u32,
        height: u32,
        tile_size: f32,
        chunk: u32,
        cell: u8,
        layers: Vec<LayerDef>,
    ) -> Result<MapFile> {
        if !matches!(cell, 1 | 2 | 4) {
            return Err(e(crate::codes::BAD_MAP, "cell must be 1, 2, or 4 bytes"));
        }
        if width == 0 || height == 0 || chunk == 0 {
            return Err(e(
                crate::codes::BAD_MAP,
                "width, height, and chunk must be at least 1",
            ));
        }
        if layers.is_empty() {
            return Err(e(
                crate::codes::BAD_MAP,
                "a tilemap needs at least one layer",
            ));
        }
        let mut names: Vec<&str> = layers.iter().map(|l| l.name.as_str()).collect();
        names.sort();
        names.dedup();
        if names.len() != layers.len() {
            return Err(e(crate::codes::BAD_MAP, "layer names must be unique"));
        }
        let stem = Path::new(rel)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("map.tilemap.toml")
            .trim_end_matches(".toml")
            .trim_end_matches(".tilemap")
            .to_string();
        let mut m = MapFile {
            root: root.to_path_buf(),
            rel: rel.to_string(),
            index: MapIndex {
                width,
                height,
                tile_size: if tile_size > 0.0 { tile_size } else { 1.0 },
                chunk,
                cell,
                chunks: format!("{stem}.chunks"),
                hash: String::new(),
                layers,
            },
            manifest: BTreeMap::new(),
            loaded: HashMap::new(),
            revision: next_revision(),
        };
        m.check_names().map_err(|x| e(crate::codes::BAD_MAP, x))?;
        m.save()?;
        Ok(m)
    }

    /// Open a tilemap. The manifest must hash to the index's `hash`.
    pub fn open(root: &Path, rel: &str) -> Result<MapFile> {
        let path = root.join(rel);
        let text = argus_os::fs::read_to_string(&path).map_err(|x| {
            e(crate::codes::BAD_MAP, format!("cannot read {rel}: {x}")).at(Some(rel.to_string()))
        })?;
        let doc: toml_edit::ImDocument<String> = toml_edit::ImDocument::parse(text.clone())
            .map_err(|x| {
                e(
                    crate::codes::BAD_MAP,
                    format!("{rel}: not valid TOML: {}", x.message()),
                )
                .at(Some(rel.to_string()))
            })?;
        let bad =
            |m: String| e(crate::codes::BAD_MAP, format!("{rel}: {m}")).at(Some(rel.to_string()));
        let int = |k: &str, d: i64| doc.get(k).and_then(|v| v.as_integer()).unwrap_or(d);
        if doc.get("kind").and_then(|k| k.as_str()) != Some("tilemap") {
            return Err(bad("kind must be \"tilemap\"".into()));
        }
        let mut layers = Vec::new();
        if let Some(arr) = doc.get("layers").and_then(|l| l.as_array_of_tables()) {
            for t in arr.iter() {
                layers.push(LayerDef {
                    name: t
                        .get("name")
                        .and_then(|n| n.as_str())
                        .ok_or_else(|| bad("every [[layers]] needs a name".into()))?
                        .to_string(),
                    tileset: t
                        .get("tileset")
                        .and_then(|n| n.as_str())
                        .map(str::to_string),
                    z: t.get("z")
                        .and_then(|v| v.as_float().or(v.as_integer().map(|i| i as f64)))
                        .unwrap_or(0.0) as f32,
                    collision: t
                        .get("collision")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                });
            }
        }
        if layers.is_empty() {
            return Err(bad("no [[layers]]".into()));
        }
        let dim = |k: &str, d: i64| {
            let v = int(k, d);
            u32::try_from(v)
                .ok()
                .filter(|&n| n >= 1)
                .ok_or_else(|| bad(format!("{k} = {v}; it must be at least 1")))
        };
        let index = MapIndex {
            width: dim("width", 0)?,
            height: dim("height", 0)?,
            tile_size: doc
                .get("tile_size")
                .and_then(|v| v.as_float().or(v.as_integer().map(|i| i as f64)))
                .unwrap_or(1.0) as f32,
            chunk: dim("chunk", 64)?,
            cell: match int("cell", 2) {
                c @ (1 | 2 | 4) => c as u8,
                c => return Err(bad(format!("cell = {c}; cells are 1, 2, or 4 bytes"))),
            },
            chunks: doc
                .get("chunks")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string(),
            hash: doc
                .get("hash")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string(),
            layers,
        };
        if !matches!(index.cell, 1 | 2 | 4) {
            return Err(bad(format!(
                "cell = {}; cells are 1, 2, or 4 bytes",
                index.cell
            )));
        }
        let mut m = MapFile {
            root: root.to_path_buf(),
            rel: rel.to_string(),
            index,
            manifest: BTreeMap::new(),
            loaded: HashMap::new(),
            revision: next_revision(),
        };
        m.check_names().map_err(bad)?;
        let mpath = m.chunk_dir().join("manifest.txt");
        let mtext = argus_os::fs::read_to_string(&mpath).unwrap_or_default();
        let actual = format!("fnv1a64:{:016x}", fnv1a64(mtext.as_bytes()));
        if actual != m.index.hash {
            return Err(e(
                crate::codes::MAP_MISMATCH,
                format!("{rel}: the chunk manifest hashes to {actual}, the index says {}", m.index.hash),
            )
            .hint("The chunks changed outside the tool. If that was intended, set hash in the index to the actual value.")
            .at(Some(rel.to_string())));
        }
        for line in mtext
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        {
            let p: Vec<&str> = line.split('\t').collect();
            if p.len() != 4 {
                continue;
            }
            let Some(li) = m.index.layers.iter().position(|l| l.name == p[0]) else {
                continue;
            };
            let (Ok(cx), Ok(cy), Ok(h)) =
                (p[1].parse(), p[2].parse(), u64::from_str_radix(p[3], 16))
            else {
                continue;
            };
            m.manifest.insert((li, cx, cy), h);
        }
        Ok(m)
    }

    /// Project-relative path of the index.
    pub fn rel(&self) -> &str {
        &self.rel
    }

    /// Layer index by name.
    pub fn layer(&self, name: &str) -> Result<usize> {
        self.index
            .layers
            .iter()
            .position(|l| l.name == name)
            .ok_or_else(|| {
                e(
                    argus_ecs::codes::UNKNOWN_FIELD,
                    format!("{} has no layer {name:?}", self.rel),
                )
                .hint(format!(
                    "Layers: {}",
                    self.index
                        .layers
                        .iter()
                        .map(|l| l.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })
    }

    fn check(&self, r: Rect) -> Result<()> {
        let (w, h) = (self.index.width, self.index.height);
        if r.w == 0
            || r.h == 0
            || r.x.checked_add(r.w).is_none_or(|e| e > w)
            || r.y.checked_add(r.h).is_none_or(|e| e > h)
        {
            return Err(e(
                argus_ecs::codes::OUT_OF_RANGE,
                format!(
                    "rect {}x{} at ({}, {}) is outside the {w}x{h} map",
                    r.w, r.h, r.x, r.y
                ),
            )
            .hint("Clip the rect to the map's width and height; scene.tiles.info shows them."));
        }
        Ok(())
    }

    fn chunk_bytes(&self) -> usize {
        (self.index.chunk * self.index.chunk) as usize * self.index.cell as usize
    }

    fn load(&mut self, k: Key) -> Result<()> {
        if self.loaded.contains_key(&k) {
            return Ok(());
        }
        let data = match self.manifest.get(&k) {
            None => vec![0u8; self.chunk_bytes()],
            Some(&h) => {
                let rel = self.chunk_rel(k);
                let bytes = argus_os::fs::read(self.root.join(&rel)).map_err(|x| {
                    e(crate::codes::MAP_MISMATCH, format!("{rel}: {x}")).at(Some(rel.clone()))
                })?;
                if bytes.len() != self.chunk_bytes() || fnv1a64(&bytes) != h {
                    return Err(e(
                        crate::codes::MAP_MISMATCH,
                        format!(
                            "{rel} does not match the manifest (size {} of {}, or its hash)",
                            bytes.len(),
                            self.chunk_bytes()
                        ),
                    )
                    .hint("The chunk changed outside the tool.")
                    .at(Some(rel)));
                }
                bytes
            }
        };
        self.loaded.insert(k, Chunk { data, dirty: false });
        Ok(())
    }

    fn read_cell(data: &[u8], i: usize, cell: usize) -> u32 {
        let o = i * cell;
        match cell {
            1 => data[o] as u32,
            2 => u16::from_le_bytes([data[o], data[o + 1]]) as u32,
            _ => u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]),
        }
    }

    fn write_cell(data: &mut [u8], i: usize, cell: usize, v: u32) {
        let o = i * cell;
        match cell {
            1 => data[o] = v as u8,
            2 => data[o..o + 2].copy_from_slice(&(v as u16).to_le_bytes()),
            _ => data[o..o + 4].copy_from_slice(&v.to_le_bytes()),
        }
    }

    /// Largest value a cell holds.
    pub fn max_tile(&self) -> u32 {
        match self.index.cell {
            1 => u8::MAX as u32,
            2 => u16::MAX as u32,
            _ => u32::MAX,
        }
    }

    /// One cell. Reading an empty chunk allocates nothing.
    pub fn cell(&mut self, layer: usize, x: u32, y: u32) -> Result<u32> {
        let c = self.index.chunk;
        let k = (layer, x / c, y / c);
        if !self.loaded.contains_key(&k) && !self.manifest.contains_key(&k) {
            return Ok(0);
        }
        self.load(k)?;
        let i = ((y % c) * c + x % c) as usize;
        Ok(Self::read_cell(
            &self.loaded[&k].data,
            i,
            self.index.cell as usize,
        ))
    }

    /// Cells of a layer inside `rect`, row-major.
    pub fn get(&mut self, layer: usize, rect: Rect) -> Result<Vec<u32>> {
        self.check(rect)?;
        let (c, cell) = (self.index.chunk, self.index.cell as usize);
        let mut out = vec![0u32; rect.area()];
        // Chunk by chunk, so each chunk is looked up once.
        for cy in rect.y / c..=(rect.y + rect.h - 1) / c {
            for cx in rect.x / c..=(rect.x + rect.w - 1) / c {
                let k = (layer, cx, cy);
                if !self.loaded.contains_key(&k) && !self.manifest.contains_key(&k) {
                    continue;
                }
                self.load(k)?;
                let data = &self.loaded[&k].data;
                let (x0, x1) = ((cx * c).max(rect.x), ((cx + 1) * c).min(rect.x + rect.w));
                let (y0, y1) = ((cy * c).max(rect.y), ((cy + 1) * c).min(rect.y + rect.h));
                for y in y0..y1 {
                    for x in x0..x1 {
                        let i = ((y % c) * c + x % c) as usize;
                        out[((y - rect.y) * rect.w + (x - rect.x)) as usize] =
                            Self::read_cell(data, i, cell);
                    }
                }
            }
        }
        Ok(out)
    }

    /// Write cells of a layer inside `rect` from a row-major slice.
    pub fn set(&mut self, layer: usize, rect: Rect, values: &[u32]) -> Result<()> {
        self.check(rect)?;
        if values.len() != rect.area() {
            return Err(e(
                crate::codes::BAD_MAP,
                format!(
                    "{} values for a {}x{} rect of {} cells",
                    values.len(),
                    rect.w,
                    rect.h,
                    rect.area()
                ),
            ));
        }
        let max = self.max_tile();
        if let Some(v) = values.iter().find(|&&v| v > max) {
            return Err(e(
                argus_ecs::codes::OUT_OF_RANGE,
                format!(
                    "tile {v} does not fit a {}-byte cell (at most {max})",
                    self.index.cell
                ),
            ));
        }
        let (c, cell) = (self.index.chunk, self.index.cell as usize);
        for cy in rect.y / c..=(rect.y + rect.h - 1) / c {
            for cx in rect.x / c..=(rect.x + rect.w - 1) / c {
                let k = (layer, cx, cy);
                let (x0, x1) = ((cx * c).max(rect.x), ((cx + 1) * c).min(rect.x + rect.w));
                let (y0, y1) = ((cy * c).max(rect.y), ((cy + 1) * c).min(rect.y + rect.h));
                // Writing zeros into a chunk that does not exist changes nothing.
                if !self.loaded.contains_key(&k) && !self.manifest.contains_key(&k) {
                    let any = (y0..y1).any(|y| {
                        (x0..x1)
                            .any(|x| values[((y - rect.y) * rect.w + (x - rect.x)) as usize] != 0)
                    });
                    if !any {
                        continue;
                    }
                }
                self.load(k)?;
                let ch = self.loaded.get_mut(&k).expect("loaded");
                for y in y0..y1 {
                    for x in x0..x1 {
                        let i = ((y % c) * c + x % c) as usize;
                        let v = values[((y - rect.y) * rect.w + (x - rect.x)) as usize];
                        if Self::read_cell(&ch.data, i, cell) != v {
                            Self::write_cell(&mut ch.data, i, cell, v);
                            ch.dirty = true;
                            self.revision = next_revision();
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Changes whenever a cell does (and when the map is read or made), and
    /// never repeats in a process: what data derived from the map (a
    /// navigation grid, phase 3 item 2) checks.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Fill `rect` with one tile.
    pub fn fill(&mut self, layer: usize, rect: Rect, tile: u32) -> Result<()> {
        let v = vec![tile; rect.area()];
        self.set(layer, rect, &v)
    }

    /// Chunks with changes not yet saved.
    pub fn dirty(&self) -> usize {
        self.loaded.values().filter(|c| c.dirty).count()
    }

    /// Chunks that hold tiles (on disk or in memory).
    pub fn chunk_count(&self) -> usize {
        let mut keys: std::collections::BTreeSet<Key> = self.manifest.keys().copied().collect();
        for (k, c) in &self.loaded {
            if c.data.iter().any(|&b| b != 0) {
                keys.insert(*k);
            } else {
                keys.remove(k);
            }
        }
        keys.len()
    }

    /// Chunks read into memory.
    pub fn loaded(&self) -> usize {
        self.loaded.len()
    }

    /// Write changed chunks, the manifest, and the index. An empty chunk
    /// is deleted rather than written.
    pub fn save(&mut self) -> Result<SaveReport> {
        let mut rep = SaveReport::default();
        let dir = self.chunk_dir();
        let io = |x: std::io::Error, what: &Path| {
            e(
                crate::codes::BAD_MAP,
                format!("cannot write {}: {x}", what.display()),
            )
        };
        argus_os::fs::create_dir_all(&dir).map_err(|x| io(x, &dir))?;
        let dirty: Vec<Key> = self
            .loaded
            .iter()
            .filter(|(_, c)| c.dirty)
            .map(|(k, _)| *k)
            .collect();
        for k in dirty {
            let rel = self.chunk_rel(k);
            let path = self.root.join(&rel);
            let ch = self.loaded.get_mut(&k).expect("loaded");
            ch.dirty = false;
            if ch.data.iter().all(|&b| b == 0) {
                if self.manifest.remove(&k).is_some() {
                    let _ = argus_os::fs::remove_file(&path);
                    rep.removed += 1;
                    rep.files.push(rel);
                }
                continue;
            }
            if let Some(p) = path.parent() {
                argus_os::fs::create_dir_all(p).map_err(|x| io(x, p))?;
            }
            argus_os::fs::write(&path, &ch.data).map_err(|x| io(x, &path))?;
            self.manifest.insert(k, fnv1a64(&ch.data));
            rep.written += 1;
            rep.bytes += ch.data.len();
            rep.files.push(rel);
        }
        let mut mtext = String::from("# argus tilemap chunks: layer\tcx\tcy\tfnv1a64\n");
        for ((li, cx, cy), h) in &self.manifest {
            mtext.push_str(&format!(
                "{}\t{cx}\t{cy}\t{h:016x}\n",
                self.index.layers[*li].name
            ));
        }
        let mpath = dir.join("manifest.txt");
        argus_os::fs::write(&mpath, &mtext).map_err(|x| io(x, &mpath))?;
        self.index.hash = format!("fnv1a64:{:016x}", fnv1a64(mtext.as_bytes()));
        // Only the hash changes after create, so an existing index is kept
        // as the person left it (comments, order) and only its hash is set.
        let ipath = self.index_path();
        let itext = argus_os::fs::read_to_string(&ipath)
            .ok()
            .and_then(|t| t.parse::<toml_edit::DocumentMut>().ok())
            .filter(|d| d.get("kind").and_then(|k| k.as_str()) == Some("tilemap"))
            .map(|mut d| {
                d["hash"] = toml_edit::value(self.index.hash.clone());
                d.to_string()
            })
            .unwrap_or_else(|| self.index_text());
        argus_os::fs::write(&ipath, &itext).map_err(|x| io(x, &ipath))?;
        rep.bytes += mtext.len() + itext.len();
        rep.files.push(self.rel.clone());
        Ok(rep)
    }

    fn index_text(&self) -> String {
        use toml_edit::{value, ArrayOfTables, DocumentMut, Item, Table};
        // f32 through its shortest text, so 0.1 is written as 0.1.
        let float = |x: f32| value(x.to_string().parse::<f64>().unwrap_or(0.0));
        let i = &self.index;
        let mut d = DocumentMut::new();
        d["kind"] = value("tilemap");
        d["width"] = value(i64::from(i.width));
        d["height"] = value(i64::from(i.height));
        d["tile_size"] = float(i.tile_size);
        d["chunk"] = value(i64::from(i.chunk));
        d["cell"] = value(i64::from(i.cell));
        d["chunks"] = value(i.chunks.clone());
        d["hash"] = value(i.hash.clone());
        let mut layers = ArrayOfTables::new();
        for l in &i.layers {
            let mut t = Table::new();
            t["name"] = value(l.name.clone());
            if let Some(ts) = &l.tileset {
                t["tileset"] = value(ts.clone());
            }
            t["z"] = float(l.z);
            t["collision"] = value(l.collision);
            layers.push(t);
        }
        d["layers"] = Item::ArrayOfTables(layers);
        format!(
            "# A tilemap: this index is text, the tiles are in {}/ (see argus-tiles).\n{d}",
            i.chunks
        )
    }
}
