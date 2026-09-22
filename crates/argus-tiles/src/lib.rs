//! Argus tilemaps, phase 1 item 5 (§12 "Tilemaps", §4 bulk data, §17):
//! layers of tiles over a grid, chunked storage, auto-tiling rules
//! declared in TOML, collision derived from tile properties, and rect
//! edits the daemon runs as transactions.
//!
//! A tileset is a sprite asset (item 4): tile `n` is frame `n - 1` and
//! `0` is empty; a frame's properties are the tile's (`solid = true`,
//! `one_way = true`, the Tiled shape); `[autotile.<terrain>]` in the same
//! sidecar declares how a terrain picks its tile from its neighbours. A
//! `Tilemap` component on an entity places a map in the world: its
//! `Transform` is the top-left corner of tile (0, 0), rows go down.

#![warn(missing_docs)]

pub mod map;
pub mod store;

pub use map::{fnv1a64, LayerDef, MapFile, MapIndex, Rect, SaveReport};
pub use store::{MapStore, TileGrid};

use argus_assets::{AutoMode, AutoTileSpec, Importer, SpriteAsset};
use argus_ecs::{ArgusError, Component, Result, StrId};
use std::collections::HashMap;

/// Error codes this crate adds (53xx).
pub mod codes {
    /// A tilemap index is malformed, or an edit does not fit it.
    pub const BAD_MAP: u32 = 5301;
    /// Chunks do not match the index or the manifest.
    pub const MAP_MISMATCH: u32 = 5302;
    /// A layer's tileset is not a sprite asset, or has no such terrain.
    pub const NO_TILESET: u32 = 5303;
}

/// Places a tilemap in the world. The entity's `Transform` is the
/// top-left corner of tile (0, 0); rows go down, as a tile editor shows.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Places a tilemap (a *.tilemap.toml) in the world; the Transform is the top-left corner of tile (0, 0)"
)]
pub struct Tilemap {
    /// Project-relative path of the tilemap index.
    #[argus(doc = "Project-relative *.tilemap.toml")]
    pub map: StrId,
    /// Draw it.
    #[argus(doc = "Draw it")]
    pub visible: bool,
}

impl Default for Tilemap {
    fn default() -> Self {
        Tilemap {
            map: StrId::EMPTY,
            visible: true,
        }
    }
}

/// Register the component.
pub fn register(world: &mut argus_ecs::World) -> Result<()> {
    world.register::<Tilemap>()?;
    Ok(())
}

// ----- auto-tiling -----------------------------------------------------------

const N: u32 = 1;
const NE: u32 = 2;
const E: u32 = 4;
const SE: u32 = 8;
const S: u32 = 16;
const SW: u32 = 32;
const W: u32 = 64;
const NW: u32 = 128;
/// The eight neighbours as (dx, dy, bit). Rows go down: north is y - 1.
const AROUND: [(i64, i64, u32); 8] = [
    (0, -1, N),
    (1, -1, NE),
    (1, 0, E),
    (1, 1, SE),
    (0, 1, S),
    (-1, 1, SW),
    (-1, 0, W),
    (-1, -1, NW),
];

/// A corner counts only when both edges beside it do: 256 raw masks
/// reduce to the 47 of the blob set.
pub fn blob_reduce(m: u32) -> u32 {
    let mut r = m & (N | E | S | W);
    if m & NE != 0 && m & N != 0 && m & E != 0 {
        r |= NE;
    }
    if m & SE != 0 && m & S != 0 && m & E != 0 {
        r |= SE;
    }
    if m & SW != 0 && m & S != 0 && m & W != 0 {
        r |= SW;
    }
    if m & NW != 0 && m & N != 0 && m & W != 0 {
        r |= NW;
    }
    r
}

/// The 47 reduced blob masks in ascending order: a `blob47` rule's
/// `tiles` list is indexed by position in this list.
pub fn blob_masks() -> Vec<u32> {
    let mut v: Vec<u32> = (0..256).map(blob_reduce).collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// A tileset's rules, compiled: tile id to terrain, and per terrain the
/// rule and a mask-to-tile table.
pub struct Rules {
    terrain_of: HashMap<u32, usize>,
    rules: Vec<(AutoTileSpec, Vec<u32>)>,
    names: Vec<String>,
}

impl Rules {
    /// Compile a tileset's `[autotile]` rules.
    pub fn new(ts: &SpriteAsset) -> Rules {
        let masks = blob_masks();
        let mut terrain_of = HashMap::new();
        let mut rules = Vec::new();
        let mut names = Vec::new();
        for (i, (name, spec)) in ts.autotile.iter().enumerate() {
            for &t in &spec.tiles {
                terrain_of.entry(t).or_insert(i);
            }
            // table[raw mask] -> tile, for the mode's mask space.
            let table: Vec<u32> = match spec.mode {
                AutoMode::Single => vec![spec.tiles[0]; 256],
                AutoMode::FourBit => (0..256u32)
                    .map(|m| {
                        let b = (m & N != 0) as usize
                            | ((m & E != 0) as usize) << 1
                            | ((m & S != 0) as usize) << 2
                            | ((m & W != 0) as usize) << 3;
                        spec.tiles[b]
                    })
                    .collect(),
                AutoMode::Blob47 => (0..256u32)
                    .map(|m| {
                        let r = blob_reduce(m);
                        spec.tiles[masks
                            .binary_search(&r)
                            .expect("reduced mask is a blob mask")]
                    })
                    .collect(),
            };
            rules.push((spec.clone(), table));
            names.push(name.clone());
        }
        Rules {
            terrain_of,
            rules,
            names,
        }
    }

    /// Terrains, in rule order.
    pub fn terrains(&self) -> &[String] {
        &self.names
    }

    fn terrain(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n == name)
    }
}

/// Paint a terrain over `rect` (or erase with `None`), then re-pick every
/// terrain tile in the rect and one cell around it from its neighbours.
/// Returns the rect that may have changed; read it before painting to
/// keep the inverse.
pub fn paint(
    map: &mut MapFile,
    layer: usize,
    rect: Rect,
    terrain: Option<&str>,
    rules: &Rules,
) -> Result<Rect> {
    let (w, h) = (map.index.width, map.index.height);
    let ti = match terrain {
        Some(t) => Some(rules.terrain(t).ok_or_else(|| {
            ArgusError::new(
                codes::NO_TILESET,
                format!("the tileset has no terrain {t:?}"),
            )
            .hint(format!("Terrains: {}", rules.terrains().join(", ")))
        })?),
        None => None,
    };
    let fill = ti.map(|i| rules.rules[i].0.tiles[0]).unwrap_or(0);
    map.fill(layer, rect, fill)?;
    let area = rect.grow(1, w, h);
    // Read the area plus one more ring, so every neighbour is known.
    let ctx = area.grow(1, w, h);
    let cells = map.get(layer, ctx)?;
    let at = |x: i64, y: i64| -> Option<u32> {
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            return None;
        }
        let (cx, cy) = (x as u32 - ctx.x, y as u32 - ctx.y);
        Some(cells[(cy * ctx.w + cx) as usize])
    };
    let mut out = map.get(layer, area)?;
    for y in area.y..area.y + area.h {
        for x in area.x..area.x + area.w {
            let t = at(x as i64, y as i64).unwrap_or(0);
            let Some(&r) = rules.terrain_of.get(&t) else {
                continue;
            };
            let (spec, table) = &rules.rules[r];
            let same = |dx: i64, dy: i64| match at(x as i64 + dx, y as i64 + dy) {
                Some(n) => rules.terrain_of.get(&n) == Some(&r),
                None => spec.connect_edges,
            };
            let mask = AROUND
                .iter()
                .filter(|&&(dx, dy, _)| same(dx, dy))
                .fold(0, |m, &(_, _, bit)| m | bit);
            out[((y - area.y) * area.w + (x - area.x)) as usize] = table[mask as usize];
        }
    }
    map.set(layer, area, &out)?;
    Ok(area)
}

// ----- collision --------------------------------------------------------------

/// What a cell does to a body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Collide {
    /// Nothing.
    Empty,
    /// Blocks from every side.
    Solid,
    /// Blocks only from above (a one-way platform).
    OneWay,
    /// A floor slope (the `slope = [left, right]` property): solid below
    /// the line from the left edge's height to the right edge's, heights as
    /// fractions of the tile from its bottom. A body stands on it at its
    /// centre and walks up and down it; from below it is a solid tile.
    Slope {
        /// Height at the left edge, 0 to 1.
        left: f32,
        /// Height at the right edge, 0 to 1.
        right: f32,
    },
}

impl Collide {
    /// One character: `#` solid, `-` one-way, `/` and `\` slopes rising
    /// right and left, `_` a flat partial block, `.` empty.
    pub fn ch(self) -> char {
        match self {
            Collide::Empty => '.',
            Collide::Solid => '#',
            Collide::OneWay => '-',
            Collide::Slope { left, right } if right > left => '/',
            Collide::Slope { left, right } if left > right => '\\',
            Collide::Slope { left, .. } if left >= 1.0 => '#',
            Collide::Slope { .. } => '_',
        }
    }

    /// Which wins when two collision layers disagree: solid, then slope,
    /// then one-way, then empty.
    pub fn rank(self) -> u8 {
        match self {
            Collide::Empty => 0,
            Collide::OneWay => 1,
            Collide::Slope { .. } => 2,
            Collide::Solid => 3,
        }
    }

    /// The floor height at `f` (0 to 1 across the tile, left to right) as
    /// a fraction of the tile: 1 for a solid tile, the line for a slope.
    pub fn height_at(self, f: f32) -> f32 {
        match self {
            Collide::Solid | Collide::OneWay => 1.0,
            Collide::Slope { left, right } => left + (right - left) * f.clamp(0.0, 1.0),
            Collide::Empty => 0.0,
        }
    }
}

fn truthy(v: Option<&str>) -> bool {
    matches!(v, Some("true") | Some("1") | Some("yes"))
}

/// What a tile of a tileset does: its `solid`, `slope`, or `one_way`
/// property. A malformed slope is solid (the importer warns about it).
pub fn collide_of(ts: Option<&SpriteAsset>, tile: u32) -> Collide {
    if tile == 0 {
        return Collide::Empty;
    }
    let Some(ts) = ts else {
        // A collision layer without a tileset: every tile is solid.
        return Collide::Solid;
    };
    if truthy(ts.tile_property(tile, "solid")) {
        Collide::Solid
    } else if let Some(s) = ts.tile_property(tile, "slope") {
        match argus_assets::parse_slope(s) {
            Some((left, right)) => Collide::Slope { left, right },
            None => Collide::Solid,
        }
    } else if truthy(ts.tile_property(tile, "one_way")) {
        Collide::OneWay
    } else {
        Collide::Empty
    }
}

/// Every tile's collision for one tileset, indexed by tile id (0 is empty).
pub fn table_of(ts: &SpriteAsset) -> Vec<Collide> {
    (0..=ts.frames.len() as u32)
        .map(|t| collide_of(Some(ts), t))
        .collect()
}

/// Collision over a rect, from every `collision = true` layer: solid wins,
/// then slope, then one-way ([`Collide::rank`]). Row-major.
pub fn collision(map: &mut MapFile, assets: &Importer, rect: Rect) -> Result<Vec<Collide>> {
    let mut out = vec![Collide::Empty; rect.area()];
    let layers: Vec<(usize, Option<String>)> = map
        .index
        .layers
        .iter()
        .enumerate()
        .filter(|(_, l)| l.collision)
        .map(|(i, l)| (i, l.tileset.clone()))
        .collect();
    for (li, ts) in layers {
        let tileset = ts
            .as_deref()
            .and_then(|n| assets.sprites().find(|(k, _)| *k == n).map(|(_, s)| s));
        let cells = map.get(li, rect)?;
        let mut memo: HashMap<u32, Collide> = HashMap::new();
        for (o, t) in out.iter_mut().zip(cells) {
            let c = *memo.entry(t).or_insert_with(|| collide_of(tileset, t));
            if c.rank() > o.rank() {
                *o = c;
            }
        }
    }
    Ok(out)
}

// ----- drawing -----------------------------------------------------------------

/// One tile to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct TileDraw {
    /// The tileset (a sprite asset name).
    pub tileset: String,
    /// Frame (tile id - 1).
    pub frame: u32,
    /// Centre in world units.
    pub pos: [f32; 2],
    /// Size in world units.
    pub size: f32,
    /// Draw order.
    pub z: f32,
}

/// The tiles of `map` that fall in a view (centre and half extents in
/// world units), placed with the map's top-left corner at `origin`.
pub fn visible(
    map: &mut MapFile,
    origin: [f32; 2],
    view_center: [f32; 2],
    view_half: [f32; 2],
) -> Result<Vec<TileDraw>> {
    let ts = map.index.tile_size;
    let (w, h) = (map.index.width as f32, map.index.height as f32);
    // World x grows with columns; world y shrinks as rows go down.
    let c0 = ((view_center[0] - view_half[0] - origin[0]) / ts)
        .floor()
        .clamp(0.0, w);
    let c1 = ((view_center[0] + view_half[0] - origin[0]) / ts)
        .ceil()
        .clamp(0.0, w);
    let r0 = ((origin[1] - (view_center[1] + view_half[1])) / ts)
        .floor()
        .clamp(0.0, h);
    let r1 = ((origin[1] - (view_center[1] - view_half[1])) / ts)
        .ceil()
        .clamp(0.0, h);
    let mut out = Vec::new();
    if c1 <= c0 || r1 <= r0 {
        return Ok(out);
    }
    let rect = Rect::new(c0 as u32, r0 as u32, (c1 - c0) as u32, (r1 - r0) as u32);
    let layers: Vec<(usize, Option<String>, f32)> = map
        .index
        .layers
        .iter()
        .enumerate()
        .map(|(i, l)| (i, l.tileset.clone(), l.z))
        .collect();
    for (li, tileset, z) in layers {
        let Some(tileset) = tileset else { continue };
        let cells = map.get(li, rect)?;
        for (i, t) in cells.into_iter().enumerate() {
            if t == 0 {
                continue;
            }
            let (col, row) = (rect.x + i as u32 % rect.w, rect.y + i as u32 / rect.w);
            out.push(TileDraw {
                tileset: tileset.clone(),
                frame: t - 1,
                pos: [
                    origin[0] + (col as f32 + 0.5) * ts,
                    origin[1] - (row as f32 + 0.5) * ts,
                ],
                size: ts,
                z,
            });
        }
    }
    Ok(out)
}
