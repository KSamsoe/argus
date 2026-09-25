//! Sprite sidecars: `assets/<name>.toml` beside `assets/<name>.png`, or
//! alone when the sprite is a described shape (§14).
//!
//! ```toml
//! # assets/sprites/hero.toml
//! grid = [4, 2]                  # or [[frames]] with rect = [x, y, w, h]
//! pivot = [0.5, 0.0]             # fraction of the frame, y up: feet
//! duration_ms = 100              # per frame, unless a frame says otherwise
//! pixels_per_unit = 16
//!
//! [[frames]]                     # overrides for single frames (by index)
//! index = 3
//! duration_ms = 200
//! properties = { hitbox = "on" } # the Tiled tile-property shape
//!
//! [clips.run]                    # an animation clip (the Aseprite tag shape)
//! frames = [0, 1, 2, 3]          # or from = 0, to = 3
//! fps = 12                       # or each frame's duration_ms
//! loop = true
//! direction = "forward"          # forward, reverse, pingpong
//!
//! # A tileset is a sprite sheet: tile n is frame n - 1, 0 is empty, and a
//! # frame's properties are the tile's (solid = true, one_way = true).
//! [autotile.rock]                # auto-tiling rules for one terrain (section 12)
//! mode = "4bit"                  # 16 tiles by N=1, E=2, S=4, W=8 neighbours;
//! tiles = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
//!                                # "blob47": 47 tiles by the 8-neighbour blob;
//!                                # "single": one tile
//! edges = "connect"              # the map's edge counts as the same terrain
//!
//! # With no hero.png, the sprite is a described shape the importer draws:
//! kind = "rect"                  # rect or circle
//! size = [8, 8]                  # pixels
//! color = "#E8703F"
//! ```

use argus_ecs::{ArgusError, Result};
use std::collections::BTreeMap;

type Doc = toml_edit::ImDocument<String>;

/// How a clip plays its frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// First to last.
    Forward,
    /// Last to first.
    Reverse,
    /// There and back.
    PingPong,
}

impl Direction {
    /// Name as written.
    pub fn name(self) -> &'static str {
        match self {
            Direction::Forward => "forward",
            Direction::Reverse => "reverse",
            Direction::PingPong => "pingpong",
        }
    }
}

/// A clip as the sidecar declares it.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipSpec {
    /// Name.
    pub name: String,
    /// Frame indices in order.
    pub frames: Vec<u32>,
    /// Frames per second; `None` uses each frame's duration.
    pub fps: Option<f32>,
    /// Loops.
    pub looping: bool,
    /// Play order.
    pub direction: Direction,
    /// `file:line`.
    pub at: String,
}

/// An explicit frame rectangle or per-frame override.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameSpec {
    /// Index this entry describes (explicit frames are indexed in order).
    pub index: Option<u32>,
    /// Pixel rect, when this entry defines a frame.
    pub rect: Option<[i64; 4]>,
    /// Pivot override.
    pub pivot: Option<[f32; 2]>,
    /// Duration override.
    pub duration_ms: Option<u32>,
    /// Properties (strings).
    pub properties: BTreeMap<String, String>,
    /// `file:line`.
    pub at: String,
}

/// How a terrain picks its tile from its neighbours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoMode {
    /// One tile, whatever the neighbours.
    Single,
    /// 16 tiles indexed by the four edge neighbours (N=1, E=2, S=4, W=8).
    FourBit,
    /// 47 tiles indexed by the 8-neighbour blob (corners count only when
    /// both edges beside them do), in ascending order of the reduced mask.
    Blob47,
}

impl AutoMode {
    /// Name as written.
    pub fn name(self) -> &'static str {
        match self {
            AutoMode::Single => "single",
            AutoMode::FourBit => "4bit",
            AutoMode::Blob47 => "blob47",
        }
    }

    /// Tiles the mode needs.
    pub fn tiles(self) -> usize {
        match self {
            AutoMode::Single => 1,
            AutoMode::FourBit => 16,
            AutoMode::Blob47 => 47,
        }
    }
}

/// An auto-tiling rule as the sidecar declares it.
#[derive(Clone, Debug, PartialEq)]
pub struct AutoTileSpec {
    /// Terrain name.
    pub terrain: String,
    /// Mode.
    pub mode: AutoMode,
    /// Tile ids (1-based frames), in the mode's order.
    pub tiles: Vec<u32>,
    /// The map's edge counts as this terrain.
    pub connect_edges: bool,
    /// `file:line`.
    pub at: String,
}

/// A described shape the importer draws when there is no image.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    /// `rect` or `circle`.
    pub kind: String,
    /// Pixels.
    pub size: [u32; 2],
    /// `#RRGGBB` or `#RRGGBBAA`.
    pub color: String,
}

/// A parsed sprite sidecar.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct SpriteSidecar {
    /// `grid = [cols, rows]`.
    pub grid: Option<[u32; 2]>,
    /// `[[frames]]` entries.
    pub frames: Vec<FrameSpec>,
    /// Default pivot.
    pub pivot: Option<[f32; 2]>,
    /// Default frame duration.
    pub duration_ms: Option<u32>,
    /// World scale.
    pub pixels_per_unit: Option<f32>,
    /// Clips.
    pub clips: Vec<ClipSpec>,
    /// A described shape.
    pub shape: Option<Shape>,
    /// Auto-tiling rules by terrain.
    pub autotile: Vec<AutoTileSpec>,
    /// Keys the importer did not recognise (reported as warnings).
    pub unknown: Vec<String>,
}

/// What a sidecar says it describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidecarKind {
    /// Frames, clips, or a shape.
    Sprite,
    /// A sound: bus, gain, variations.
    Sound,
    /// An animator state machine (item 8).
    Animator,
    /// A particle effect (item 10).
    Emitter,
    /// A material: a shader and its parameters (item 11).
    Material,
    /// A font's settings (phase 3 item 1), which need the font beside them.
    Font,
    /// It does not say, and nothing in it tells.
    Unknown,
}

pub(crate) fn err(file: &str, line: u32, msg: impl Into<String>) -> ArgusError {
    ArgusError::new(crate::codes::BAD_SIDECAR, msg.into())
        .hint("A sprite sidecar has grid or [[frames]], pivot, [clips.name], or kind/size/color for a shape; a sound sidecar has variations, tone, or file. `type = \"sprite\"` or `\"sound\"` says which.")
        .at(Some(format!("{file}:{line}")))
}

pub(crate) fn line_of(text: &str, item: &toml_edit::Item) -> u32 {
    let at = item.span().map(|s| s.start).unwrap_or(0).min(text.len());
    text[..at].matches('\n').count() as u32 + 1
}

fn nums(v: &toml_edit::Value) -> Vec<f64> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_float().or_else(|| x.as_integer().map(|i| i as f64)))
                .collect()
        })
        .unwrap_or_default()
}

fn num(v: Option<&toml_edit::Item>) -> Option<f64> {
    let v = v?.as_value()?;
    v.as_float().or_else(|| v.as_integer().map(|i| i as f64))
}

pub(crate) fn parse(file: &str, text: &str) -> Result<Doc> {
    Doc::parse(text.to_string()).map_err(|e| {
        let line = e
            .span()
            .map(|s| text[..s.start.min(text.len())].matches('\n').count() as u32 + 1)
            .unwrap_or(1);
        err(file, line, format!("not valid TOML: {}", e.message()))
    })
}

/// Which kind a sidecar describes, from `type` or its keys.
pub fn kind_of(file: &str, text: &str) -> Result<SidecarKind> {
    let doc = parse(file, text)?;
    if let Some(t) = doc.get("type").and_then(|i| i.as_str()) {
        return match t {
            "sprite" => Ok(SidecarKind::Sprite),
            "sound" => Ok(SidecarKind::Sound),
            "animator" => Ok(SidecarKind::Animator),
            "emitter" => Ok(SidecarKind::Emitter),
            "material" => Ok(SidecarKind::Material),
            "font" => Ok(SidecarKind::Font),
            other => Err(err(
                file,
                1,
                format!(
                    "type = {other:?}; types are sprite, sound, animator, emitter, material, and font"
                ),
            )),
        };
    }
    let has = |k: &str| doc.get(k).is_some();
    if has("states") || has("initial") {
        return Ok(SidecarKind::Animator);
    }
    if has("rate") || has("burst") || has("lifetime") {
        return Ok(SidecarKind::Emitter);
    }
    if has("shader") || has("params") || has("blend") {
        return Ok(SidecarKind::Material);
    }
    if has("variations") || has("tone") || has("file") || has("bus") {
        return Ok(SidecarKind::Sound);
    }
    if has("kind") || has("grid") || has("frames") || has("clips") || has("pivot") || has("color") {
        return Ok(SidecarKind::Sprite);
    }
    Ok(SidecarKind::Unknown)
}

const SPRITE_KEYS: &[&str] = &[
    "type",
    "grid",
    "frames",
    "pivot",
    "duration_ms",
    "pixels_per_unit",
    "clips",
    "kind",
    "size",
    "color",
    "autotile",
];

fn pivot_of(v: &toml_edit::Value) -> Option<[f32; 2]> {
    let p = nums(v);
    (p.len() == 2).then(|| [p[0] as f32, p[1] as f32])
}

/// Parse a sprite sidecar.
pub fn parse_sprite(file: &str, text: &str) -> Result<SpriteSidecar> {
    let doc = parse(file, text)?;
    let mut out = SpriteSidecar::default();
    for (k, _) in doc.as_table().iter() {
        if !SPRITE_KEYS.contains(&k) {
            out.unknown.push(k.to_string());
        }
    }
    if let Some(g) = doc.get("grid") {
        let v = g.as_value().map(nums).unwrap_or_default();
        if v.len() != 2 || v[0] < 1.0 || v[1] < 1.0 {
            return Err(err(
                file,
                line_of(text, g),
                "grid must be [columns, rows], both at least 1",
            ));
        }
        out.grid = Some([v[0] as u32, v[1] as u32]);
    }
    if let Some(p) = doc.get("pivot") {
        out.pivot = Some(p.as_value().and_then(pivot_of).ok_or_else(|| {
            err(
                file,
                line_of(text, p),
                "pivot must be [x, y], fractions of the frame",
            )
        })?);
    }
    out.duration_ms = num(doc.get("duration_ms")).map(|d| d.max(1.0) as u32);
    out.pixels_per_unit = num(doc.get("pixels_per_unit")).map(|d| d.max(0.001) as f32);
    if let Some(item) = doc.get("frames") {
        let line = line_of(text, item);
        let arr = item.as_array_of_tables().ok_or_else(|| {
            err(
                file,
                line,
                "frames must be [[frames]] tables with rect = [x, y, w, h] or index = n",
            )
        })?;
        for t in arr.iter() {
            let fl = t
                .span()
                .map(|s| text[..s.start.min(text.len())].matches('\n').count() as u32 + 1)
                .unwrap_or(line);
            let rect = match t.get("rect").and_then(|r| r.as_value()) {
                Some(v) => {
                    let r = nums(v);
                    if r.len() != 4 {
                        return Err(err(file, fl, "a frame rect is [x, y, w, h] in pixels"));
                    }
                    Some([r[0] as i64, r[1] as i64, r[2] as i64, r[3] as i64])
                }
                None => None,
            };
            let mut properties = BTreeMap::new();
            if let Some(p) = t.get("properties").and_then(|p| p.as_table_like()) {
                for (k, v) in p.iter() {
                    let s = match v.as_value() {
                        Some(v) if v.as_str().is_some() => v.as_str().unwrap().to_string(),
                        Some(v) => v.to_string().trim().to_string(),
                        None => String::new(),
                    };
                    properties.insert(k.to_string(), s);
                }
            }
            out.frames.push(FrameSpec {
                index: num(t.get("index")).map(|i| i.max(0.0) as u32),
                rect,
                pivot: t.get("pivot").and_then(|p| p.as_value()).and_then(pivot_of),
                duration_ms: num(t.get("duration_ms")).map(|d| d.max(1.0) as u32),
                properties,
                at: format!("{file}:{fl}"),
            });
        }
    }
    if let Some(item) = doc.get("clips") {
        let t = item.as_table_like().ok_or_else(|| {
            err(
                file,
                line_of(text, item),
                "clips must be [clips.name] tables",
            )
        })?;
        for (name, c) in t.iter() {
            let cl = line_of(text, c);
            let ct = c
                .as_table_like()
                .ok_or_else(|| err(file, cl, format!("clips.{name} must be a table")))?;
            let frames: Vec<u32> = if let Some(f) = ct.get("frames").and_then(|f| f.as_value()) {
                nums(f).into_iter().map(|x| x.max(0.0) as u32).collect()
            } else if let (Some(a), Some(b)) = (num(ct.get("from")), num(ct.get("to"))) {
                let (a, b) = (a.max(0.0) as u32, b.max(0.0) as u32);
                if a <= b {
                    (a..=b).collect()
                } else {
                    (b..=a).rev().collect()
                }
            } else {
                return Err(err(
                    file,
                    cl,
                    format!("clips.{name} needs frames = [...] or from and to"),
                ));
            };
            let direction = match ct.get("direction").and_then(|d| d.as_str()).unwrap_or("forward") {
                "forward" => Direction::Forward,
                "reverse" => Direction::Reverse,
                "pingpong" | "ping-pong" => Direction::PingPong,
                other => {
                    return Err(err(file, cl, format!("clips.{name}.direction = {other:?}; directions are forward, reverse, pingpong")))
                }
            };
            out.clips.push(ClipSpec {
                name: name.to_string(),
                frames,
                fps: num(ct.get("fps")).map(|f| f.max(0.001) as f32),
                looping: ct.get("loop").and_then(|l| l.as_bool()).unwrap_or(true),
                direction,
                at: format!("{file}:{cl}"),
            });
        }
    }
    if let Some(item) = doc.get("autotile") {
        let t = item.as_table_like().ok_or_else(|| {
            err(
                file,
                line_of(text, item),
                "autotile must be [autotile.terrain] tables",
            )
        })?;
        for (terrain, a) in t.iter() {
            let al = line_of(text, a);
            let at = a
                .as_table_like()
                .ok_or_else(|| err(file, al, format!("autotile.{terrain} must be a table")))?;
            let mode = match at.get("mode").and_then(|m| m.as_str()).unwrap_or("4bit") {
                "single" => AutoMode::Single,
                "4bit" => AutoMode::FourBit,
                "blob47" | "blob" => AutoMode::Blob47,
                other => {
                    return Err(err(
                        file,
                        al,
                        format!(
                            "autotile.{terrain}.mode = {other:?}; modes are single, 4bit, blob47"
                        ),
                    ))
                }
            };
            let tiles: Vec<u32> = at
                .get("tiles")
                .and_then(|v| v.as_value())
                .map(nums)
                .unwrap_or_default()
                .into_iter()
                .map(|x| x.max(0.0) as u32)
                .collect();
            if tiles.len() != mode.tiles() {
                return Err(err(
                    file,
                    al,
                    format!(
                        "autotile.{terrain} is {} and needs {} tiles; it lists {}",
                        mode.name(),
                        mode.tiles(),
                        tiles.len()
                    ),
                ));
            }
            if tiles.contains(&0) {
                return Err(err(
                    file,
                    al,
                    format!("autotile.{terrain}: tile 0 is empty; tiles start at 1 (frame 0)"),
                ));
            }
            let connect_edges = match at
                .get("edges")
                .and_then(|e| e.as_str())
                .unwrap_or("connect")
            {
                "connect" => true,
                "open" => false,
                other => {
                    return Err(err(
                        file,
                        al,
                        format!("autotile.{terrain}.edges = {other:?}; edges are connect or open"),
                    ))
                }
            };
            out.autotile.push(AutoTileSpec {
                terrain: terrain.to_string(),
                mode,
                tiles,
                connect_edges,
                at: format!("{file}:{al}"),
            });
        }
    }
    if doc.get("kind").is_some() || doc.get("size").is_some() || doc.get("color").is_some() {
        let kind = doc
            .get("kind")
            .and_then(|k| k.as_str())
            .unwrap_or("rect")
            .to_string();
        if kind != "rect" && kind != "circle" {
            let l = doc.get("kind").map(|k| line_of(text, k)).unwrap_or(1);
            return Err(err(
                file,
                l,
                format!("kind = {kind:?}; shapes are rect and circle"),
            ));
        }
        let size = doc
            .get("size")
            .and_then(|s| s.as_value())
            .map(nums)
            .filter(|v| v.len() == 2)
            .map(|v| {
                [
                    v[0].clamp(1.0, 4096.0) as u32,
                    v[1].clamp(1.0, 4096.0) as u32,
                ]
            })
            .unwrap_or([16, 16]);
        let color = doc
            .get("color")
            .and_then(|c| c.as_str())
            .unwrap_or("#FFFFFF")
            .to_string();
        if parse_color(&color).is_none() {
            let l = doc.get("color").map(|k| line_of(text, k)).unwrap_or(1);
            return Err(err(
                file,
                l,
                format!("color = {color:?}; colours are #RRGGBB or #RRGGBBAA"),
            ));
        }
        out.shape = Some(Shape { kind, size, color });
    }
    Ok(out)
}

/// `#RRGGBB` or `#RRGGBBAA` to RGBA bytes.
pub fn parse_color(s: &str) -> Option<[u8; 4]> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 && h.len() != 8 {
        return None;
    }
    let v = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    Some([v(0)?, v(2)?, v(4)?, if h.len() == 8 { v(6)? } else { 255 }])
}
