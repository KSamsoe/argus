//! A Tiled map as the engine's map (phase 5 item 4, §12 "a Tiled map
//! becomes the engine's map and its layers").
//!
//! `argus_assets::tiled::read_map` reads the `.tmx` or `.tmj` (the importer
//! reads it the same way for its events and `asset.get`); this puts its
//! layers and cells into a [`MapFile`] held in memory
//! ([`MapFile::in_memory`]), so drawing, collision, navigation, lighting,
//! and the semantic frame read it as they read any map. [`MapStore::open`]
//! reads it whenever the file's modification time moves, as it reopens an
//! index another process saved. A Tiled map is Tiled's to edit: `set`
//! refuses it, and so does the daemon's `edit.tiles`, with the hint to edit
//! it in Tiled.
//!
//! [`MapStore::open`]: crate::MapStore::open

use crate::map::{fnv1a64, LayerDef, MapFile, MapIndex, Rect};
use argus_ecs::{ArgusError, Result};
use std::collections::BTreeMap;
use std::path::Path;

/// Whether `rel` is a Tiled map (`.tmx` or `.tmj`).
pub fn is_tiled(rel: &str) -> bool {
    argus_assets::tiled::is_map_file(rel)
}

/// What a Tiled map's file said beyond its cells.
#[derive(Clone, Debug, PartialEq)]
pub struct TiledSource {
    /// Per layer: drawn. A layer Tiled hides is not drawn; it still
    /// collides when it is a collision layer (Tiled's usual hidden
    /// collision layer).
    pub visible: Vec<bool>,
    /// Its structure, as `scene.tiles.info` shows it
    /// (`argus_assets::tiled::TiledMap::info`).
    pub info: serde_json::Value,
    /// What did not carry over, each with where.
    pub warnings: Vec<(String, String)>,
}

/// The refusal for an edit of a Tiled map.
pub fn read_only(file: &str) -> ArgusError {
    ArgusError::new(
        crate::codes::BAD_MAP,
        format!("{file} is a Tiled map: the engine reads it and never writes it"),
    )
    .hint("This map is Tiled's: edit it in Tiled, and the engine reads it again when it is saved.")
    .at(Some(file.to_string()))
}

/// Read the Tiled map at `rel` (project-relative) into the engine's form.
pub fn open(root: &Path, rel: &str) -> Result<MapFile> {
    let bytes = argus_os::fs::read(root.join(rel)).map_err(|x| {
        ArgusError::new(crate::codes::BAD_MAP, format!("cannot read {rel}: {x}"))
            .at(Some(rel.to_string()))
    })?;
    let mut load = |p: &str| argus_os::fs::read(root.join(p)).map_err(|x| x.to_string());
    let map = argus_assets::tiled::read_map(rel, rel, &bytes, &mut load).map_err(|x| {
        let mut x = x;
        x.code = crate::codes::BAD_MAP;
        x
    })?;
    if map.layers.is_empty() {
        return Err(ArgusError::new(
            crate::codes::BAD_MAP,
            format!("{rel} has no tile layers; a Tilemap draws a map's tile layers"),
        )
        .hint("Its objects are data: asset.get on the map (under assets/) or scene.tiles.info shows them.")
        .at(Some(rel.to_string())));
    }
    let max = map
        .layers
        .iter()
        .flat_map(|l| l.cells.iter().map(|c| c[2]))
        .max()
        .unwrap_or(0);
    let index = MapIndex {
        width: map.width,
        height: map.height,
        tile_size: map.tile_size,
        chunk: 64,
        cell: if max <= u16::MAX as u32 { 2 } else { 4 },
        chunks: String::new(),
        hash: format!("fnv1a64:{:016x}", fnv1a64(&bytes)),
        layers: map
            .layers
            .iter()
            .map(|l| LayerDef {
                name: l.name.clone(),
                tileset: l.tileset.clone(),
                z: l.z,
                collision: l.collision,
                parallax: l.parallax,
            })
            .collect(),
        runtime: false,
        parallax_origin: map.parallax_origin,
    };
    let mut m = MapFile::in_memory(root, rel, index);
    let c = m.index.chunk;
    let (w, h) = (m.index.width, m.index.height);
    for (li, l) in map.layers.iter().enumerate() {
        // A chunk's worth of cells at a time.
        let mut chunks: BTreeMap<(u32, u32), Vec<u32>> = BTreeMap::new();
        for &[x, y, t] in &l.cells {
            let (cx, cy) = (x / c, y / c);
            let cw = c.min(w - cx * c);
            let buf = chunks
                .entry((cx, cy))
                .or_insert_with(|| vec![0; (cw * c.min(h - cy * c)) as usize]);
            buf[((y % c) * cw + x % c) as usize] = t;
        }
        for ((cx, cy), cells) in chunks {
            let rect = Rect::new(cx * c, cy * c, c.min(w - cx * c), c.min(h - cy * c));
            m.set(li, rect, &cells)?;
        }
    }
    let info = map.info();
    m.set_tiled(TiledSource {
        visible: map.layers.iter().map(|l| l.visible).collect(),
        info,
        warnings: map
            .warnings
            .into_iter()
            .map(|w| (w.message, w.at))
            .collect(),
    });
    Ok(m)
}
