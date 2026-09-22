//! Bulk authored data (§4 "Bulk authored data"): a text index file plus a
//! binary chunk. The index carries everything a reader needs to reason about
//! the data (dimensions, layers, cell width, tileset, a content hash); the
//! chunk carries the numbers. Readers query rectangles through
//! [`Bulk::get`], which is what `scene.tiles.get {rect}` serves; nobody sees
//! a 40,000-line array.
//!
//! ```toml
//! # tiles.map.toml
//! kind    = "tiles"
//! width   = 64
//! height  = 48
//! layers  = ["ground", "props"]
//! cell    = 2                      # bytes per cell: 1, 2, or 4
//! chunk   = "tiles.map.bin"        # next to the index
//! hash    = "fnv1a64:9f0c..."      # of the chunk; checked on open
//! tileset = "assets/tiles.png"     # optional, a path reference
//! ```
//!
//! Chunk layout: layers in index order, each `width * height` cells,
//! row-major, little-endian.

use crate::codes;
use crate::tomlv;
use argus_ecs::{ArgusError, Result};
use std::path::{Path, PathBuf};

/// True for a data index that sits beside entity files but is not one: a
/// bulk index (`*.map.toml`) or a tilemap (`*.tilemap.toml`, argus-tiles).
/// Scene loading, moves, and the index all skip these.
pub fn is_data_index(path: &str) -> bool {
    path.ends_with(".map.toml") || path.ends_with(".tilemap.toml")
}

/// True for a file stem an entity may not have: `scene` and `room` are the
/// scene and room documents, and `.map` / `.tilemap` stems are data indexes.
/// An entity by such a name would be written and then skipped on load.
pub fn reserved_stem(stem: &str) -> bool {
    stem == "scene" || stem == "room" || is_data_index(&format!("{stem}.toml"))
}

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
}

/// The text index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BulkIndex {
    /// What the data is (`tiles`, `props`, `nav`).
    pub kind: String,
    /// Columns.
    pub width: u32,
    /// Rows.
    pub height: u32,
    /// Layer names, in chunk order.
    pub layers: Vec<String>,
    /// Bytes per cell: 1, 2, or 4.
    pub cell: u8,
    /// Chunk file name, relative to the index.
    pub chunk: String,
    /// `fnv1a64:<hex>` of the chunk.
    pub hash: String,
    /// Optional tileset path reference.
    pub tileset: Option<String>,
}

impl BulkIndex {
    /// Expected chunk size in bytes.
    pub fn bytes(&self) -> usize {
        self.width as usize * self.height as usize * self.layers.len() * self.cell as usize
    }
}

/// An open bulk file: index plus chunk in memory.
#[derive(Clone, Debug)]
pub struct Bulk {
    index_path: PathBuf,
    rel: String,
    /// The index.
    pub index: BulkIndex,
    data: Vec<u8>,
    dirty: bool,
}

/// 64-bit FNV-1a, the chunk hash.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn hash_string(data: &[u8]) -> String {
    format!("fnv1a64:{:016x}", fnv1a64(data))
}

impl Bulk {
    /// Open `rel` (a project-relative `*.map.toml`) under `root`. The chunk
    /// must exist, have exactly the size the index implies, and hash to
    /// the index's `hash`; otherwise error 4608 names what disagrees.
    pub fn open(root: &Path, rel: &str) -> Result<Bulk> {
        let index_path = root.join(rel);
        let text = tomlv::read(&index_path)?;
        let im = tomlv::parse(rel, &text)?;
        let t = im.as_table();
        let at = |k: &str| {
            t.get(k)
                .map(|i| tomlv::item_ref(rel, &text, i))
                .unwrap_or_else(|| format!("{rel}:1"))
        };
        let need_str = |k: &str| -> Result<String> {
            t.get(k)
                .and_then(|i| i.as_str())
                .map(str::to_string)
                .ok_or_else(|| {
                    ArgusError::new(codes::BAD_SCENE, format!("{rel}: {k} must be a string"))
                        .at(Some(at(k)))
                })
        };
        let need_u32 = |k: &str| -> Result<u32> {
            t.get(k)
                .and_then(|i| i.as_integer())
                .and_then(|i| u32::try_from(i).ok())
                .ok_or_else(|| {
                    ArgusError::new(
                        codes::BAD_SCENE,
                        format!("{rel}: {k} must be a non-negative integer"),
                    )
                    .at(Some(at(k)))
                })
        };
        let cell = need_u32("cell")?;
        if !matches!(cell, 1 | 2 | 4) {
            return Err(ArgusError::new(
                codes::BAD_SCENE,
                format!("{rel}: cell must be 1, 2, or 4 bytes"),
            )
            .at(Some(at("cell"))));
        }
        let layers = t
            .get("layers")
            .map(|i| tomlv::string_list(i, "layers", &at("layers")))
            .transpose()?
            .unwrap_or_else(|| vec!["default".to_string()]);
        let index = BulkIndex {
            kind: need_str("kind")?,
            width: need_u32("width")?,
            height: need_u32("height")?,
            layers,
            cell: cell as u8,
            chunk: need_str("chunk")?,
            hash: need_str("hash")?,
            tileset: t
                .get("tileset")
                .and_then(|i| i.as_str())
                .map(str::to_string),
        };
        let chunk_path = index_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(&index.chunk);
        let data = std::fs::read(&chunk_path).map_err(|e| {
            ArgusError::new(
                codes::MISSING_FILE,
                format!("{rel}: chunk {} cannot be read: {e}", index.chunk),
            )
            .hint("The chunk lives next to its index. If it was deleted, recreate the bulk data through the protocol.")
            .at(Some(at("chunk")))
        })?;
        if data.len() != index.bytes() {
            return Err(ArgusError::new(
                codes::BULK_MISMATCH,
                format!(
                    "{rel}: chunk {} is {} bytes; the index implies {} ({}x{}x{} layers x {} bytes)",
                    index.chunk,
                    data.len(),
                    index.bytes(),
                    index.width,
                    index.height,
                    index.layers.len(),
                    index.cell
                ),
            )
            .hint("Either the index or the chunk was edited by hand; regenerate the chunk or fix the dimensions.")
            .at(Some(at("chunk"))));
        }
        let actual = hash_string(&data);
        if actual != index.hash {
            return Err(ArgusError::new(
                codes::BULK_MISMATCH,
                format!(
                    "{rel}: chunk hash is {actual}; the index says {}",
                    index.hash
                ),
            )
            .hint("The chunk changed outside the tool. If that was intended, update hash in the index to the actual value.")
            .at(Some(at("hash"))));
        }
        Ok(Bulk {
            index_path,
            rel: rel.to_string(),
            index,
            data,
            dirty: false,
        })
    }

    /// Create a new zeroed bulk file pair and write both files.
    pub fn create(
        root: &Path,
        rel: &str,
        kind: &str,
        width: u32,
        height: u32,
        layers: &[&str],
        cell: u8,
    ) -> Result<Bulk> {
        if !matches!(cell, 1 | 2 | 4) {
            return Err(ArgusError::new(
                codes::BAD_SCENE,
                "cell must be 1, 2, or 4 bytes",
            ));
        }
        let index_path = root.join(rel);
        let stem = Path::new(rel)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("bulk.map.toml")
            .trim_end_matches(".toml")
            .to_string();
        let index = BulkIndex {
            kind: kind.to_string(),
            width,
            height,
            layers: layers.iter().map(|s| s.to_string()).collect(),
            cell,
            chunk: format!("{stem}.bin"),
            hash: String::new(),
            tileset: None,
        };
        let data = vec![0u8; index.bytes()];
        let mut b = Bulk {
            index_path,
            rel: rel.to_string(),
            index,
            data,
            dirty: true,
        };
        b.write_index_fresh()?;
        b.save()?;
        Ok(b)
    }

    fn write_index_fresh(&self) -> Result<()> {
        let mut doc = toml_edit::DocumentMut::new();
        doc["kind"] = toml_edit::value(self.index.kind.as_str());
        doc["width"] = toml_edit::value(self.index.width as i64);
        doc["height"] = toml_edit::value(self.index.height as i64);
        doc["layers"] = toml_edit::value(toml_edit::Array::from_iter(
            self.index.layers.iter().map(String::as_str),
        ));
        doc["cell"] = toml_edit::value(self.index.cell as i64);
        doc["chunk"] = toml_edit::value(self.index.chunk.as_str());
        doc["hash"] = toml_edit::value(hash_string(&self.data));
        if let Some(t) = &self.index.tileset {
            doc["tileset"] = toml_edit::value(t.as_str());
        }
        if let Some(dir) = self.index_path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                ArgusError::new(
                    codes::MISSING_FILE,
                    format!("cannot create {}: {e}", dir.display()),
                )
            })?;
        }
        std::fs::write(&self.index_path, doc.to_string()).map_err(|e| {
            ArgusError::new(
                codes::MISSING_FILE,
                format!("cannot write {}: {e}", self.rel),
            )
        })
    }

    fn chunk_path(&self) -> PathBuf {
        self.index_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(&self.index.chunk)
    }

    fn layer_offset(&self, layer: &str) -> Result<usize> {
        let i = self
            .index
            .layers
            .iter()
            .position(|l| l == layer)
            .ok_or_else(|| {
                ArgusError::new(
                    argus_ecs::codes::UNKNOWN_FIELD,
                    format!("{} has no layer {layer:?}", self.rel),
                )
                .hint(format!("Layers: {}", self.index.layers.join(", ")))
            })?;
        Ok(i * self.index.width as usize * self.index.height as usize * self.index.cell as usize)
    }

    fn check_rect(&self, r: Rect) -> Result<()> {
        let (w, h) = (self.index.width, self.index.height);
        if r.x.checked_add(r.w).map(|e| e > w).unwrap_or(true)
            || r.y.checked_add(r.h).map(|e| e > h).unwrap_or(true)
        {
            return Err(ArgusError::new(
                argus_ecs::codes::OUT_OF_RANGE,
                format!(
                    "rect {}x{} at ({}, {}) is outside the {w}x{h} grid",
                    r.w, r.h, r.x, r.y
                ),
            )
            .hint("Clip the rect to the index's width and height."));
        }
        Ok(())
    }

    /// Cells of `layer` inside `rect`, row-major.
    pub fn get(&self, layer: &str, rect: Rect) -> Result<Vec<u64>> {
        self.check_rect(rect)?;
        let base = self.layer_offset(layer)?;
        let cell = self.index.cell as usize;
        let width = self.index.width as usize;
        let mut out = Vec::with_capacity(rect.w as usize * rect.h as usize);
        for row in rect.y..rect.y + rect.h {
            for col in rect.x..rect.x + rect.w {
                let off = base + (row as usize * width + col as usize) * cell;
                let bytes = &self.data[off..off + cell];
                out.push(match cell {
                    1 => bytes[0] as u64,
                    2 => u16::from_le_bytes([bytes[0], bytes[1]]) as u64,
                    _ => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64,
                });
            }
        }
        Ok(out)
    }

    /// Write cells of `layer` inside `rect` from a row-major slice of
    /// exactly `w * h` values. Values must fit the cell width.
    pub fn set(&mut self, layer: &str, rect: Rect, values: &[u64]) -> Result<()> {
        self.check_rect(rect)?;
        let base = self.layer_offset(layer)?;
        let cell = self.index.cell as usize;
        let width = self.index.width as usize;
        let n = rect.w as usize * rect.h as usize;
        if values.len() != n {
            return Err(ArgusError::new(
                argus_ecs::codes::TYPE_MISMATCH,
                format!("rect holds {n} cells; got {} values", values.len()),
            ));
        }
        let max = match cell {
            1 => u8::MAX as u64,
            2 => u16::MAX as u64,
            _ => u32::MAX as u64,
        };
        if let Some(v) = values.iter().find(|v| **v > max) {
            return Err(ArgusError::new(
                argus_ecs::codes::OUT_OF_RANGE,
                format!("{v} does not fit in a {cell}-byte cell"),
            ));
        }
        let mut i = 0;
        for row in rect.y..rect.y + rect.h {
            for col in rect.x..rect.x + rect.w {
                let off = base + (row as usize * width + col as usize) * cell;
                let v = values[i];
                i += 1;
                match cell {
                    1 => self.data[off] = v as u8,
                    2 => self.data[off..off + 2].copy_from_slice(&(v as u16).to_le_bytes()),
                    _ => self.data[off..off + 4].copy_from_slice(&(v as u32).to_le_bytes()),
                }
            }
        }
        self.dirty = true;
        Ok(())
    }

    /// Current hash of the in-memory chunk.
    pub fn hash(&self) -> String {
        hash_string(&self.data)
    }

    /// True if there are unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Write the chunk and update `hash` in the index, keeping the rest of
    /// the index file as it was.
    pub fn save(&mut self) -> Result<()> {
        let chunk = self.chunk_path();
        std::fs::write(&chunk, &self.data).map_err(|e| {
            ArgusError::new(
                codes::MISSING_FILE,
                format!("cannot write {}: {e}", chunk.display()),
            )
        })?;
        let text = tomlv::read(&self.index_path)?;
        let mut doc = tomlv::parse_mut(&self.rel, &text)?;
        let h = self.hash();
        doc["hash"] = toml_edit::value(h.as_str());
        std::fs::write(&self.index_path, doc.to_string()).map_err(|e| {
            ArgusError::new(
                codes::MISSING_FILE,
                format!("cannot write {}: {e}", self.rel),
            )
        })?;
        self.index.hash = h;
        self.dirty = false;
        Ok(())
    }
}
