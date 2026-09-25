//! Baked grids (§4: "a baked navigation grid" is bulk authored data, "a
//! text index file plus a binary chunk: the index carries everything a
//! reader needs to reason about the data (dimensions, tileset, layer names,
//! a content hash), and the chunk carries the numbers").
//!
//! `<name>.navgrid.toml` is the index an agent reads; `<name>.navgrid.bin`
//! holds one byte a cell, the step cost (0 blocked), row by row from row 0
//! at the bottom. A `NavGrid` whose `baked` names the index starts from it
//! instead of deriving its tilemap; `NavCost` entities still go over it.

use crate::codes;
use crate::grid::Grid;
use argus_ecs::{ArgusError, Result};
use std::path::Path;

/// Whether a project-relative path is a baked grid's index (which a scene
/// loader leaves alone, as it does a tilemap's).
pub fn is_index(rel: &str) -> bool {
    rel.ends_with(".navgrid.toml")
}

/// The chunk beside an index: `a/b.navgrid.toml` -> `a/b.navgrid.bin`.
pub fn chunk_of(index: &str) -> String {
    format!("{}.bin", index.trim_end_matches(".toml"))
}

fn bad(file: &str, why: impl Into<String>) -> ArgusError {
    ArgusError::new(codes::BAD_BAKE, format!("{file}: {}", why.into()))
        .hint("Bake it again with nav.bake, or clear NavGrid.baked to derive the grid.")
        .at(Some(file.to_string()))
}

/// What a bake wrote.
#[derive(Clone, Debug, PartialEq)]
pub struct Written {
    /// The index, project-relative.
    pub index: String,
    /// The chunk, project-relative.
    pub chunk: String,
    /// Chunk bytes.
    pub bytes: usize,
    /// Open cells.
    pub open: usize,
}

/// Write `grid` as `index` (project-relative, `*.navgrid.toml`) and its
/// chunk under `root`. `from` says what it was baked from.
pub fn write(root: &Path, index: &str, grid: &Grid, from: &str) -> Result<Written> {
    if !is_index(index) {
        return Err(ArgusError::new(
            codes::BAD_BAKE,
            format!("{index} does not end in .navgrid.toml"),
        ));
    }
    let chunk = chunk_of(index);
    let open = grid.open_cells();
    let chunk_name = Path::new(&chunk)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let text = format!(
        "# A baked navigation grid (nav.bake, phase 3 item 2): this index, and\n\
         # the step costs in {chunk_name}, one byte a cell (0 blocked), row by\n\
         # row from row 0 at the bottom. NavGrid.baked names this file.\n\
         width = {}\n\
         height = {}\n\
         cell = {}\n\
         origin = [{}, {}]\n\
         diagonal = {}\n\
         open = {open}\n\
         blocked = {}\n\
         chunk = \"{chunk_name}\"\n\
         hash = \"{:016x}\"\n\
         from = \"{}\"\n",
        grid.width,
        grid.height,
        argus_ecs::schema::shortest_f64(grid.cell),
        argus_ecs::schema::shortest_f64(grid.origin[0]),
        argus_ecs::schema::shortest_f64(grid.origin[1]),
        grid.diagonal,
        grid.len() - open,
        grid.hash,
        from.replace('"', "'"),
    );
    let ip = root.join(index);
    if let Some(dir) = ip.parent() {
        argus_os::fs::create_dir_all(dir).map_err(|e| bad(index, e.to_string()))?;
    }
    argus_os::fs::write(root.join(&chunk), &grid.cost).map_err(|e| bad(&chunk, e.to_string()))?;
    argus_os::fs::write(&ip, text.as_bytes()).map_err(|e| bad(index, e.to_string()))?;
    Ok(Written {
        index: index.to_string(),
        chunk,
        bytes: grid.cost.len(),
        open,
    })
}

/// Read a baked grid: its index and chunk, checked against each other.
pub fn read(root: &Path, index: &str) -> Result<Grid> {
    let text =
        argus_os::fs::read_to_string(root.join(index)).map_err(|e| bad(index, e.to_string()))?;
    let doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| bad(index, e.to_string()))?;
    let int = |k: &str| -> Result<u32> {
        doc.get(k)
            .and_then(|v| v.as_integer())
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| bad(index, format!("no {k}")))
    };
    let num = |v: &toml_edit::Item| v.as_float().or_else(|| v.as_integer().map(|i| i as f64));
    let (width, height) = (int("width")?, int("height")?);
    let cell = doc
        .get("cell")
        .and_then(num)
        .ok_or_else(|| bad(index, "no cell"))? as f32;
    let origin = doc
        .get("origin")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_float().or_else(|| x.as_integer().map(|i| i as f64)))
                .collect::<Vec<_>>()
        })
        .filter(|a| a.len() == 2)
        .ok_or_else(|| bad(index, "origin is two numbers"))?;
    let diagonal = doc
        .get("diagonal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let hash = doc
        .get("hash")
        .and_then(|v| v.as_str())
        .and_then(|s| u64::from_str_radix(s, 16).ok());
    let chunk = chunk_of(index);
    let cost = argus_os::fs::read(root.join(&chunk)).map_err(|e| bad(&chunk, e.to_string()))?;
    if cost.len() != width as usize * height as usize {
        return Err(bad(
            index,
            format!(
                "the chunk has {} cells, the index says {width} x {height}",
                cost.len()
            ),
        ));
    }
    let mut g = Grid::new(
        [origin[0] as f32, origin[1] as f32],
        cell,
        width,
        height,
        diagonal,
        0,
    );
    g.cost = cost;
    g.seal();
    if hash.is_some_and(|h| h != g.hash) {
        return Err(bad(index, "the chunk does not match the index's hash"));
    }
    Ok(g)
}
