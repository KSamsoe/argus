//! Tiled's files, read natively (phase 5 item 4; §14 "Aseprite and Tiled
//! files import natively with their internal structure exposed", §12 "a
//! Tiled map becomes the engine's map and its layers").
//!
//! A tileset (`.tsx`, or `.tsj` in Tiled's JSON) is a sprite asset like any
//! other: its name is its path under `assets/` without the extension, and
//! tile `n` of a map layer is frame `n - 1`, which is Tiled's tile id
//! `n - 1`. A tile's custom properties are its frame's, so `solid`,
//! `one_way`, and `slope` collide as they do from a sidecar. A sidecar
//! beside it (`<name>.toml`) may add `[autotile.<terrain>]` rules and
//! per-frame overrides; where the tiles are is the tileset's to say.
//!
//! A map (`.tmx`, or `.tmj`) under `assets/` is an asset of kind `tilemap`:
//! its import reports what it holds and what the engine makes of it. A
//! `Tilemap` naming the file (`map = "assets/maps/level1.tmx"`) places it;
//! argus-tiles reads it with [`read_map`] into the engine's form. Its
//! embedded tilesets import as sprites named `<map path>/<tileset name>`
//! (`maps/level1.tmx/terrain`), a name no file under `assets/` can have.
//!
//! What the engine has no place for is a warning, never a silent loss:
//! flipped tiles (the engine draws tiles unflipped), opacity, tint, image
//! layers, Tiled's terrain sets, and a tile's collision shapes. A layer's
//! parallax factors (its groups' multiplied in) and the map's parallax
//! origin are carried (2D staples item 3).

use crate::{
    codes, fnv, Asset, Clip, Direction, Entry, Frame, Group, Importer, SpriteAsset, Warning,
};
use argus_ecs::{ArgusError, Result};
use serde_json::{json, Value as Json};
use std::collections::{BTreeMap, BTreeSet};

/// A custom property's value as text (the engine's frame properties are
/// text: `true`, `3`, `0.5`, `[0, 1]`).
pub type Props = BTreeMap<String, String>;

const FLIP_H: u32 = 0x8000_0000;
const FLIP_V: u32 = 0x4000_0000;
const FLIP_D: u32 = 0x2000_0000;
const ROTATE_HEX: u32 = 0x1000_0000;
/// The bits of a GID that flip or rotate its tile.
const FLAGS: u32 = FLIP_H | FLIP_V | FLIP_D | ROTATE_HEX;

fn ext(rel: &str) -> String {
    rel.rsplit_once('.')
        .filter(|(_, e)| !e.contains('/'))
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// A Tiled tileset file (`.tsx` or `.tsj`).
pub fn is_tileset_file(rel: &str) -> bool {
    matches!(ext(rel).as_str(), "tsx" | "tsj")
}

/// A Tiled map file (`.tmx` or `.tmj`).
pub fn is_map_file(rel: &str) -> bool {
    matches!(ext(rel).as_str(), "tmx" | "tmj")
}

/// The part of a root-relative path under its `assets/`: the game's, or a
/// mod's `mods/<id>/assets/`. `None` outside one.
pub fn under_assets(path: &str) -> Option<&str> {
    if let Some(r) = path.strip_prefix("assets/") {
        return Some(r);
    }
    let (_, rest) = path.strip_prefix("mods/")?.split_once('/')?;
    rest.strip_prefix("assets/")
}

fn strip_ext(p: &str) -> &str {
    match p.rsplit_once('.') {
        Some((stem, e)) if !e.contains('/') && !stem.is_empty() => stem,
        _ => p,
    }
}

/// The asset a root-relative file under an `assets/` is: its path there
/// without the extension.
pub fn asset_of(path: &str) -> Option<String> {
    under_assets(path).map(|r| strip_ext(r).to_string())
}

/// `rel` (as a Tiled file writes a path) from the folder of `base`, both
/// root-relative; `None` when it leaves the root or is absolute.
pub fn resolve(base: &str, rel: &str) -> Option<String> {
    let rel = rel.replace('\\', "/");
    if rel.starts_with('/') || rel.contains(':') {
        return None;
    }
    let mut parts: Vec<&str> = base.split('/').collect();
    parts.pop();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// The sprite asset an embedded tileset of the map at `path` (root-relative)
/// imports as: the map's path under `assets/`, a slash, and the tileset's
/// name with anything but letters, digits, `-`, and `_` made `_`.
pub fn embedded_name(path: &str, tileset: &str, firstgid: u32) -> String {
    let base = under_assets(path).unwrap_or(path);
    let clean: String = tileset
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let clean = clean.trim_matches('_');
    if clean.is_empty() {
        format!("{base}/tileset{firstgid}")
    } else {
        format!("{base}/{clean}")
    }
}

fn fail(m: impl Into<String>) -> ArgusError {
    ArgusError::new(codes::BAD_SOURCE, m.into())
}

/// Warnings as they are found, each with where.
struct Warns {
    out: Vec<Warning>,
}

impl Warns {
    fn add(&mut self, at: &str, m: impl Into<String>) {
        self.out.push(Warning {
            message: m.into(),
            at: at.to_string(),
        });
    }
}

// ----- a small XML tree --------------------------------------------------------

#[derive(Debug, Default)]
struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Node>,
    text: String,
    line: usize,
}

impl Node {
    fn attr(&self, k: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(a, _)| a == k)
            .map(|(_, v)| v.as_str())
    }

    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    fn at(&self, shown: &str) -> String {
        format!("{shown}:{}", self.line)
    }
}

/// Lines counted as the reader moves forward.
struct Lines<'a> {
    text: &'a [u8],
    pos: usize,
    line: usize,
}

impl Lines<'_> {
    fn at(&mut self, pos: usize) -> usize {
        let pos = pos.min(self.text.len());
        if pos < self.pos {
            self.pos = 0;
            self.line = 1;
        }
        self.line += self.text[self.pos..pos]
            .iter()
            .filter(|&&b| b == b'\n')
            .count();
        self.pos = pos;
        self.line
    }
}

fn element(shown: &str, s: &quick_xml::events::BytesStart, line: usize) -> Result<Node> {
    let mut n = Node {
        name: String::from_utf8_lossy(s.name().as_ref()).into_owned(),
        line,
        ..Node::default()
    };
    for a in s.attributes() {
        let a = a.map_err(|e| {
            fail(format!("{shown}:{line}: a malformed attribute: {e}"))
                .at(Some(format!("{shown}:{line}")))
        })?;
        let v = a
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| {
                fail(format!("{shown}:{line}: a malformed attribute: {e}"))
                    .at(Some(format!("{shown}:{line}")))
            })?;
        n.attrs.push((
            String::from_utf8_lossy(a.key.as_ref()).into_owned(),
            v.into_owned(),
        ));
    }
    Ok(n)
}

fn parse_xml(shown: &str, text: &str) -> Result<Node> {
    use quick_xml::events::Event;
    let mut r = quick_xml::Reader::from_str(text);
    let mut lines = Lines {
        text: text.as_bytes(),
        pos: 0,
        line: 1,
    };
    let mut stack: Vec<Node> = vec![Node::default()];
    loop {
        let ev = match r.read_event() {
            Ok(ev) => ev,
            Err(e) => {
                let line = lines.at(r.error_position() as usize);
                return Err(fail(format!("{shown}:{line}: not valid XML: {e}"))
                    .hint("Tiled writes its files whole; save it again from Tiled.")
                    .at(Some(format!("{shown}:{line}"))));
            }
        };
        let line = lines.at(r.buffer_position() as usize);
        match ev {
            Event::Start(s) => stack.push(element(shown, &s, line)?),
            Event::Empty(s) => {
                let n = element(shown, &s, line)?;
                if let Some(top) = stack.last_mut() {
                    top.children.push(n);
                }
            }
            Event::End(_) => {
                if stack.len() < 2 {
                    return Err(
                        fail(format!("{shown}:{line}: a closing tag with nothing open"))
                            .at(Some(format!("{shown}:{line}"))),
                    );
                }
                let n = stack.pop().expect("open element");
                stack.last_mut().expect("document").children.push(n);
            }
            Event::Text(t) => {
                let s = t.xml10_content().map_err(|e| {
                    fail(format!("{shown}:{line}: {e}")).at(Some(format!("{shown}:{line}")))
                })?;
                stack.last_mut().expect("document").text.push_str(&s);
            }
            Event::CData(c) => {
                let s = String::from_utf8_lossy(&c.into_inner()).into_owned();
                stack.last_mut().expect("document").text.push_str(&s);
            }
            Event::GeneralRef(g) => {
                let ch = match g.resolve_char_ref() {
                    Ok(Some(c)) => c.to_string(),
                    _ => match g.decode().as_deref() {
                        Ok("amp") => "&".into(),
                        Ok("lt") => "<".into(),
                        Ok("gt") => ">".into(),
                        Ok("quot") => "\"".into(),
                        Ok("apos") => "'".into(),
                        Ok(other) => format!("&{other};"),
                        Err(_) => String::new(),
                    },
                };
                stack.last_mut().expect("document").text.push_str(&ch);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if stack.len() != 1 {
        let open = stack.last().map(|n| n.name.clone()).unwrap_or_default();
        return Err(
            fail(format!("{shown}: the file ends with <{open}> still open"))
                .hint("The file is cut short; save it again from Tiled.")
                .at(Some(shown.to_string())),
        );
    }
    stack
        .pop()
        .and_then(|d| d.children.into_iter().next())
        .ok_or_else(|| {
            fail(format!("{shown}: no XML element in the file")).at(Some(shown.to_string()))
        })
}

fn num<T: std::str::FromStr>(n: &Node, k: &str, shown: &str) -> Result<Option<T>> {
    match n.attr(k) {
        None => Ok(None),
        Some(v) => v.trim().parse().map(Some).map_err(|_| {
            fail(format!(
                "{}: <{} {k}=\"{v}\"> is not a number of the kind Tiled writes there",
                n.at(shown),
                n.name
            ))
            .at(Some(n.at(shown)))
        }),
    }
}

fn xml_props(n: &Node, shown: &str, w: &mut Warns) -> Props {
    let mut out = Props::new();
    let Some(ps) = n.child("properties") else {
        return out;
    };
    for p in ps.all("property") {
        let name = p.attr("name").unwrap_or_default().to_string();
        let ty = p.attr("type").unwrap_or("string");
        if ty == "class" {
            w.add(
                &p.at(shown),
                format!("property {name:?} is a class; its members are not carried"),
            );
            continue;
        }
        let value = p
            .attr("value")
            .map(str::to_string)
            .unwrap_or_else(|| p.text.clone());
        out.insert(name, value);
    }
    out
}

fn json_props(v: &Json, at: &str, w: &mut Warns) -> Props {
    let mut out = Props::new();
    for p in v["properties"].as_array().into_iter().flatten() {
        let name = p["name"].as_str().unwrap_or_default().to_string();
        let value = match &p["value"] {
            Json::Bool(b) => b.to_string(),
            Json::Number(n) => n.to_string(),
            Json::String(s) => s.clone(),
            Json::Null => String::new(),
            _ => {
                w.add(
                    at,
                    format!("property {name:?} is a class; its members are not carried"),
                );
                continue;
            }
        };
        out.insert(name, value);
    }
    out
}

fn truthy(v: Option<&String>) -> bool {
    matches!(v.map(String::as_str), Some("true" | "1" | "yes"))
}

// ----- tilesets ------------------------------------------------------------------

/// One tile a tileset says something about.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileDef {
    /// Its class (Tiled's `type` before 1.9).
    pub class: String,
    /// Its custom properties.
    pub properties: Props,
    /// Its animation: Tiled's tile id and milliseconds, step by step.
    pub animation: Vec<(u32, u32)>,
    /// It has collision shapes drawn in Tiled's collision editor.
    pub shapes: bool,
    /// It has an image of its own (an image collection).
    pub image: bool,
}

/// A tileset as Tiled wrote it.
#[derive(Clone, Debug, PartialEq)]
pub struct Tileset {
    /// Its name in Tiled.
    pub name: String,
    /// A tile's size in pixels.
    pub tile_width: u32,
    /// A tile's size in pixels.
    pub tile_height: u32,
    /// Pixels around the tiles.
    pub margin: u32,
    /// Pixels between the tiles.
    pub spacing: u32,
    /// Columns of tiles (0: count them from the image).
    pub columns: u32,
    /// Tiles (0: count them from the image).
    pub tile_count: u32,
    /// Its image, as written (relative to the file holding the tileset).
    pub image: Option<String>,
    /// Tiles with something to say, by Tiled's id.
    pub tiles: BTreeMap<u32, TileDef>,
    /// The tileset's own custom properties.
    pub properties: Props,
    /// Where it is, for messages.
    pub at: String,
    /// What did not carry over.
    pub warnings: Vec<Warning>,
}

impl Tileset {
    /// Tiles each with an image of their own: a collection, not a sheet.
    pub fn is_collection(&self) -> bool {
        self.image.is_none() && self.tiles.values().any(|t| t.image)
    }

    /// Each tile's rect in a `w` x `h` image, in Tiled's id order, as many
    /// as the tileset says and the image holds.
    pub fn rects(&self, w: u32, h: u32) -> Vec<[u32; 4]> {
        // In u64: a hand-edited tileset's numbers must not overflow.
        let (tw, th) = (
            self.tile_width.max(1) as u64,
            self.tile_height.max(1) as u64,
        );
        let (m, s) = (self.margin as u64, self.spacing as u64);
        let fit = |size: u32, t: u64| ((size as u64).saturating_sub(2 * m) + s) / (t + s);
        let columns = if self.columns > 0 {
            self.columns as u64
        } else {
            fit(w, tw)
        }
        .max(1);
        let count = if self.tile_count > 0 {
            self.tile_count as u64
        } else {
            columns * fit(h, th)
        };
        let mut out = Vec::new();
        for i in 0..count {
            let (c, r) = (i % columns, i / columns);
            let (x, y) = (m + c * (tw + s), m + r * (th + s));
            if x + tw > w as u64 || y + th > h as u64 {
                break;
            }
            out.push([x as u32, y as u32, tw as u32, th as u32]);
        }
        out
    }
}

fn xml_tileset(n: &Node, shown: &str) -> Result<Tileset> {
    let mut w = Warns { out: Vec::new() };
    let at = n.at(shown);
    let mut ts = Tileset {
        name: n.attr("name").unwrap_or_default().to_string(),
        tile_width: num(n, "tilewidth", shown)?.unwrap_or(0),
        tile_height: num(n, "tileheight", shown)?.unwrap_or(0),
        margin: num(n, "margin", shown)?.unwrap_or(0),
        spacing: num(n, "spacing", shown)?.unwrap_or(0),
        columns: num(n, "columns", shown)?.unwrap_or(0),
        tile_count: num(n, "tilecount", shown)?.unwrap_or(0),
        image: n
            .child("image")
            .and_then(|i| i.attr("source"))
            .map(str::to_string),
        tiles: BTreeMap::new(),
        properties: xml_props(n, shown, &mut w),
        at: at.clone(),
        warnings: Vec::new(),
    };
    if let Some(o) = n.child("tileoffset") {
        let (x, y): (i64, i64) = (
            num(o, "x", shown)?.unwrap_or(0),
            num(o, "y", shown)?.unwrap_or(0),
        );
        if (x, y) != (0, 0) {
            w.add(
                &o.at(shown),
                format!("the tile offset ({x}, {y}) px is not carried; tiles sit on the grid"),
            );
        }
    }
    if n.child("grid")
        .and_then(|g| g.attr("orientation"))
        .is_some_and(|o| o != "orthogonal")
    {
        w.add(
            &at,
            "the tileset's grid is not orthogonal; tiles draw on a square grid",
        );
    }
    if n.child("wangsets").is_some() || n.child("terraintypes").is_some() {
        w.add(&at, "Tiled's terrain sets are not read; declare [autotile.<terrain>] rules in a sidecar beside the tileset");
    }
    for t in n.all("tile") {
        let id: u32 = num(t, "id", shown)?.unwrap_or(0);
        let mut def = TileDef {
            class: t
                .attr("class")
                .or_else(|| t.attr("type"))
                .unwrap_or_default()
                .to_string(),
            properties: xml_props(t, shown, &mut w),
            animation: Vec::new(),
            shapes: t.child("objectgroup").is_some(),
            image: t.child("image").is_some(),
        };
        if let Some(a) = t.child("animation") {
            for f in a.all("frame") {
                def.animation.push((
                    num(f, "tileid", shown)?.unwrap_or(0),
                    num(f, "duration", shown)?.unwrap_or(100),
                ));
            }
        }
        ts.tiles.insert(id, def);
    }
    ts.warnings = w.out;
    Ok(ts)
}

fn json_u32(v: &Json, k: &str, at: &str) -> Result<u32> {
    match &v[k] {
        Json::Null => Ok(0),
        x => x
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| {
                fail(format!("{at}: {k} = {x} is not a count")).at(Some(at.to_string()))
            }),
    }
}

fn json_tileset(v: &Json, at: &str) -> Result<Tileset> {
    let mut w = Warns { out: Vec::new() };
    if !v.is_object() {
        return Err(fail(format!("{at}: a tileset is a JSON object")).at(Some(at.to_string())));
    }
    let mut ts = Tileset {
        name: v["name"].as_str().unwrap_or_default().to_string(),
        tile_width: json_u32(v, "tilewidth", at)?,
        tile_height: json_u32(v, "tileheight", at)?,
        margin: json_u32(v, "margin", at)?,
        spacing: json_u32(v, "spacing", at)?,
        columns: json_u32(v, "columns", at)?,
        tile_count: json_u32(v, "tilecount", at)?,
        image: v["image"].as_str().map(str::to_string),
        tiles: BTreeMap::new(),
        properties: json_props(v, at, &mut w),
        at: at.to_string(),
        warnings: Vec::new(),
    };
    let off = &v["tileoffset"];
    if off["x"].as_i64().unwrap_or(0) != 0 || off["y"].as_i64().unwrap_or(0) != 0 {
        w.add(
            at,
            format!(
                "the tile offset ({}, {}) px is not carried; tiles sit on the grid",
                off["x"], off["y"]
            ),
        );
    }
    if v["grid"]["orientation"]
        .as_str()
        .is_some_and(|o| o != "orthogonal")
    {
        w.add(
            at,
            "the tileset's grid is not orthogonal; tiles draw on a square grid",
        );
    }
    if v["wangsets"].as_array().is_some_and(|a| !a.is_empty()) {
        w.add(at, "Tiled's terrain sets are not read; declare [autotile.<terrain>] rules in a sidecar beside the tileset");
    }
    for t in v["tiles"].as_array().into_iter().flatten() {
        let id = json_u32(t, "id", at)?;
        let def = TileDef {
            class: t["class"]
                .as_str()
                .or_else(|| t["type"].as_str())
                .unwrap_or_default()
                .to_string(),
            properties: json_props(t, at, &mut w),
            animation: t["animation"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|f| {
                    (
                        f["tileid"].as_u64().unwrap_or(0) as u32,
                        f["duration"].as_u64().unwrap_or(100) as u32,
                    )
                })
                .collect(),
            shapes: t["objectgroup"].is_object(),
            image: t["image"].is_string(),
        };
        ts.tiles.insert(id, def);
    }
    ts.warnings = w.out;
    Ok(ts)
}

fn text_of<'a>(shown: &str, bytes: &'a [u8]) -> Result<&'a str> {
    let t = std::str::from_utf8(bytes).map_err(|_| {
        fail(format!(
            "{shown}: not UTF-8 text, as Tiled writes its files"
        ))
        .at(Some(shown.to_string()))
    })?;
    Ok(t.strip_prefix('\u{feff}').unwrap_or(t))
}

fn parse_json(shown: &str, text: &str) -> Result<Json> {
    serde_json::from_str(text).map_err(|e| {
        let at = format!("{shown}:{}", e.line());
        fail(format!("{at}: not valid JSON: {e}"))
            .hint("Tiled writes its files whole; save it again from Tiled.")
            .at(Some(at))
    })
}

/// A tileset file (`.tsx` or `.tsj`) as Tiled wrote it; `shown` names it
/// in messages.
pub fn read_tileset(shown: &str, bytes: &[u8]) -> Result<Tileset> {
    let text = text_of(shown, bytes)?;
    if ext(shown) == "tsj" {
        let v = parse_json(shown, text)?;
        json_tileset(&v, shown)
    } else {
        let root = parse_xml(shown, text)?;
        if root.name != "tileset" {
            return Err(fail(format!(
                "{shown}: not a Tiled tileset: its root is <{}>",
                root.name
            ))
            .at(Some(shown.to_string())));
        }
        xml_tileset(&root, shown)
    }
}

// ----- maps ----------------------------------------------------------------------

/// A tileset a map uses.
#[derive(Clone, Debug, PartialEq)]
pub struct TilesetUse {
    /// The map's id of its first tile.
    pub firstgid: u32,
    /// Its name in Tiled (the file's stem when the file was not read).
    pub name: String,
    /// Its file, root-relative, when it is a file of its own.
    pub source: Option<String>,
    /// The sprite asset its tiles draw from.
    pub asset: String,
    /// Its tiles, when known.
    pub tile_count: Option<u32>,
    /// It is written inside the map.
    pub embedded: bool,
}

/// One of the engine's layers a map becomes: a Tiled tile layer, or the
/// part of one drawn from one tileset.
#[derive(Clone, Debug, PartialEq)]
pub struct MapLayer {
    /// Its name: the Tiled layer's (with its groups, `group/layer`), and
    /// the tileset's in brackets when the layer was split by tileset.
    pub name: String,
    /// The Tiled layer it came from.
    pub from: String,
    /// The sprite asset its tiles draw from (`None` for an empty layer).
    pub tileset: Option<String>,
    /// Draw order: the layer's `z` property, else its place among the
    /// map's tile layers (0, 1, 2, ...).
    pub z: f32,
    /// Its `collision` property: its tiles' properties collide.
    pub collision: bool,
    /// Its parallax factors, its groups' multiplied in: 1, 1 for none.
    pub parallax: [f32; 2],
    /// Shown in Tiled (and in every group around it).
    pub visible: bool,
    /// Its custom properties (with its groups').
    pub properties: Props,
    /// Its tiles: column, row, and tile id (tile `n` is the tileset's
    /// frame `n - 1`).
    pub cells: Vec<[u32; 3]>,
}

/// A Tiled layer as it was, and what it became.
#[derive(Clone, Debug, PartialEq)]
pub struct TiledLayer {
    /// Its name with its groups (`group/layer`).
    pub name: String,
    /// `tiles`, `objects`, or `image`.
    pub kind: String,
    /// Shown in Tiled.
    pub visible: bool,
    /// The tilesets its tiles come from.
    pub tilesets: Vec<String>,
    /// The engine layers it became.
    pub layers: Vec<String>,
}

/// One object of an object layer. Positions and sizes are world units
/// from the map's top-left corner, y up as the world's: a `Tilemap` at
/// `(x, y)` has the object's top-left corner at `(x + pos.x, y + pos.y)`.
#[derive(Clone, Debug, PartialEq)]
pub struct MapObject {
    /// Tiled's id.
    pub id: u32,
    /// Its name.
    pub name: String,
    /// Its class (Tiled's `type` before 1.9).
    pub class: String,
    /// `rect`, `ellipse`, `point`, `polygon`, `polyline`, `tile`, or `text`.
    pub shape: String,
    /// Top-left corner.
    pub pos: [f32; 2],
    /// Width and height.
    pub size: [f32; 2],
    /// Degrees clockwise, as Tiled has it.
    pub rotation: f32,
    /// A polygon's or polyline's points, from `pos`.
    pub points: Vec<[f32; 2]>,
    /// A tile object's tileset (a sprite asset) and tile id.
    pub tile: Option<(String, u32)>,
    /// Shown in Tiled.
    pub visible: bool,
    /// Its custom properties.
    pub properties: Props,
}

/// An object layer: data for the game, not entities.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectLayer {
    /// Its name with its groups.
    pub name: String,
    /// Shown in Tiled.
    pub visible: bool,
    /// Its custom properties.
    pub properties: Props,
    /// Its objects.
    pub objects: Vec<MapObject>,
}

/// A Tiled map read into the engine's terms.
#[derive(Clone, Debug, PartialEq)]
pub struct TiledMap {
    /// `tmx` or `tmj`.
    pub format: String,
    /// Columns of the engine's grid.
    pub width: u32,
    /// Rows of the engine's grid.
    pub height: u32,
    /// Pixels a tile in Tiled.
    pub tile_px: u32,
    /// World units a tile: the map's `tile_size` property, else 1.
    pub tile_size: f32,
    /// An infinite map (its chunks' bounds are the engine's grid).
    pub infinite: bool,
    /// The Tiled cell that is the engine's (0, 0).
    pub origin: [i64; 2],
    /// Where the camera's centre is when the parallax layers are drawn
    /// where their tiles are: Tiled's parallax origin, in world units from
    /// the engine grid's top-left corner, y up.
    pub parallax_origin: [f32; 2],
    /// The map's custom properties.
    pub properties: Props,
    /// Its tilesets, by first id.
    pub tilesets: Vec<TilesetUse>,
    /// Every Tiled layer, groups flattened, in Tiled's order.
    pub tiled_layers: Vec<TiledLayer>,
    /// The engine's layers.
    pub layers: Vec<MapLayer>,
    /// Object layers.
    pub objects: Vec<ObjectLayer>,
    /// What did not carry over.
    pub warnings: Vec<Warning>,
}

fn props_json(p: &Props) -> Json {
    Json::Object(p.iter().map(|(k, v)| (k.clone(), json!(v))).collect())
}

fn round(x: f32) -> f64 {
    (x as f64 * 1e4).round() / 1e4
}

impl TiledMap {
    /// What it holds, as `asset.get` and `scene.tiles.info` show it.
    pub fn info(&self) -> Json {
        json!({
            "format": self.format,
            "orientation": "orthogonal",
            "size": [self.width, self.height],
            "tile_px": self.tile_px,
            "tile_size": self.tile_size,
            "infinite": self.infinite,
            "origin": self.origin,
            "parallax_origin": self.parallax_origin,
            "properties": props_json(&self.properties),
            "tilesets": self.tilesets.iter().map(|t| json!({
                "name": t.name,
                "firstgid": t.firstgid,
                "tiles": t.tile_count,
                "asset": t.asset,
                "source": t.source,
                "embedded": t.embedded,
            })).collect::<Vec<_>>(),
            "tiled_layers": self.tiled_layers.iter().map(|l| json!({
                "name": l.name,
                "kind": l.kind,
                "visible": l.visible,
                "tilesets": l.tilesets,
                "layers": l.layers,
            })).collect::<Vec<_>>(),
            "layers": self.layers.iter().map(|l| json!({
                "name": l.name,
                "kind": "tiles",
                "from": l.from,
                "tileset": l.tileset,
                "collision": l.collision,
                "z": l.z,
                "parallax": l.parallax,
                "visible": l.visible,
                "tiles": l.cells.len(),
                "properties": props_json(&l.properties),
            })).collect::<Vec<_>>(),
            "object_layers": self.objects.iter().map(|o| json!({
                "name": o.name,
                "visible": o.visible,
                "properties": props_json(&o.properties),
                "objects": o.objects.iter().map(|b| {
                    let mut j = json!({
                        "id": b.id,
                        "name": b.name,
                        "class": b.class,
                        "shape": b.shape,
                        "pos": [round(b.pos[0]), round(b.pos[1])],
                        "size": [round(b.size[0]), round(b.size[1])],
                        "rotation": b.rotation,
                        "visible": b.visible,
                        "properties": props_json(&b.properties),
                    });
                    if !b.points.is_empty() {
                        j["points"] = json!(b.points.iter().map(|p| [round(p[0]), round(p[1])]).collect::<Vec<_>>());
                    }
                    if let Some((ts, t)) = &b.tile {
                        j["tile"] = json!({ "tileset": ts, "tile": t });
                    }
                    j
                }).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }
}

/// A tileset as a map names it.
struct TsRef {
    firstgid: u32,
    /// As written.
    source: Option<String>,
    embedded: Option<Tileset>,
    at: String,
}

/// A tile layer as read, in Tiled's cells.
struct RawTiles {
    name: String,
    visible: bool,
    offset: [f64; 2],
    parallax: [f64; 2],
    props: Props,
    cells: Vec<(i64, i64, u32)>,
    /// An infinite map's chunks: x, y, w, h.
    chunks: Vec<[i64; 4]>,
    at: String,
}

struct RawObject {
    id: u32,
    name: String,
    class: String,
    shape: String,
    xy: [f64; 2],
    wh: [f64; 2],
    rotation: f32,
    points: Vec<[f64; 2]>,
    gid: Option<u32>,
    visible: bool,
    props: Props,
    template: bool,
}

struct RawObjects {
    name: String,
    visible: bool,
    offset: [f64; 2],
    props: Props,
    objects: Vec<RawObject>,
    at: String,
}

enum Flat {
    Tiles(RawTiles),
    Objects(RawObjects),
    Image {
        name: String,
        visible: bool,
        at: String,
    },
}

/// What a map file says, before it becomes the engine's.
struct Raw {
    orientation: String,
    width: u32,
    height: u32,
    tw: u32,
    th: u32,
    infinite: bool,
    props: Props,
    tilesets: Vec<TsRef>,
    layers: Vec<Flat>,
    /// Tiled's parallax origin, in pixels.
    parallax_origin: [f64; 2],
}

/// A group's say over the layers in it.
#[derive(Clone)]
struct Ctx {
    prefix: String,
    visible: bool,
    offset: [f64; 2],
    props: Props,
    opacity: f64,
    parallax: [f64; 2],
}

impl Ctx {
    fn top() -> Ctx {
        Ctx {
            prefix: String::new(),
            visible: true,
            offset: [0.0, 0.0],
            props: Props::new(),
            opacity: 1.0,
            parallax: [1.0, 1.0],
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn inner(
        &self,
        name: &str,
        visible: bool,
        offset: [f64; 2],
        props: Props,
        opacity: f64,
        parallax: [f64; 2],
        tint: bool,
        at: &str,
        w: &mut Warns,
    ) -> Ctx {
        let path = format!("{}{name}", self.prefix);
        if opacity < 1.0 {
            w.add(
                at,
                format!("layer {path:?} has opacity {opacity}; the engine draws it opaque"),
            );
        }
        if tint {
            w.add(
                at,
                format!("layer {path:?} has a tint colour; it is not carried"),
            );
        }
        let mut merged = self.props.clone();
        merged.extend(props);
        Ctx {
            prefix: path,
            visible: self.visible && visible,
            offset: [self.offset[0] + offset[0], self.offset[1] + offset[1]],
            props: merged,
            opacity: self.opacity * opacity,
            parallax: [
                self.parallax[0] * parallax[0],
                self.parallax[1] * parallax[1],
            ],
        }
    }
}

/// Layer data as Tiled encodes it: GIDs, flags and all.
fn decode(
    at: &str,
    encoding: Option<&str>,
    compression: Option<&str>,
    text: &str,
) -> Result<Vec<u32>> {
    let bad = |m: String| fail(format!("{at}: {m}")).at(Some(at.to_string()));
    match encoding.unwrap_or("csv") {
        "csv" => text
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.parse::<u32>()
                    .map_err(|_| bad(format!("{s:?} in the layer's CSV is not a tile id")))
            })
            .collect(),
        "base64" => {
            let raw =
                base64(text).ok_or_else(|| bad("the layer's base64 data is malformed".into()))?;
            let bytes = match compression.unwrap_or("") {
                "" => raw,
                "zlib" => inflate(flate2::read::ZlibDecoder::new(&raw[..]))
                    .map_err(|e| bad(format!("the layer's zlib data does not inflate: {e}")))?,
                "gzip" => inflate(flate2::read::MultiGzDecoder::new(&raw[..]))
                    .map_err(|e| bad(format!("the layer's gzip data does not inflate: {e}")))?,
                "zstd" => {
                    return Err(bad("the layer is compressed with zstd, which the engine does not read".into())
                        .hint("Save it with zlib, gzip, or CSV (Tiled: Map > Map Properties > Tile Layer Format)."))
                }
                other => {
                    return Err(bad(format!("compression {other:?} is not one Tiled writes"))
                        .hint("Save it with zlib, gzip, or CSV."))
                }
            };
            if bytes.len() % 4 != 0 {
                return Err(bad(format!(
                    "the layer's data is {} bytes, not a whole number of 4-byte tile ids",
                    bytes.len()
                )));
            }
            Ok(bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| u32::from_le_bytes(*b))
                .collect())
        }
        other => Err(bad(format!("encoding {other:?} is not one Tiled writes"))
            .hint("Save it with CSV or base64.")),
    }
}

fn inflate(mut r: impl std::io::Read) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    r.read_to_end(&mut out)?;
    Ok(out)
}

/// Standard (or URL-safe) base64, whitespace ignored, padding optional.
fn base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn xml_gids(n: &Node, shown: &str) -> Result<Vec<u32>> {
    match n.attr("encoding") {
        None => n
            .all("tile")
            .map(|t| num(t, "gid", shown).map(|g| g.unwrap_or(0)))
            .collect(),
        enc => decode(&n.at(shown), enc, n.attr("compression"), &n.text),
    }
}

fn dense(gids: &[u32], x0: i64, y0: i64, w: u32, out: &mut Vec<(i64, i64, u32)>) {
    for (i, &g) in gids.iter().enumerate() {
        if g != 0 {
            let i = i as i64;
            out.push((x0 + i % w.max(1) as i64, y0 + i / w.max(1) as i64, g));
        }
    }
}

fn xml_raw(shown: &str, root: &Node, w: &mut Warns) -> Result<Raw> {
    if root.name != "map" {
        return Err(fail(format!(
            "{shown}: not a Tiled map: its root is <{}>",
            root.name
        ))
        .at(Some(shown.to_string())));
    }
    let mut raw = Raw {
        orientation: root.attr("orientation").unwrap_or("orthogonal").to_string(),
        width: num(root, "width", shown)?.unwrap_or(0),
        height: num(root, "height", shown)?.unwrap_or(0),
        tw: num(root, "tilewidth", shown)?.unwrap_or(0),
        th: num(root, "tileheight", shown)?.unwrap_or(0),
        infinite: root.attr("infinite") == Some("1"),
        props: xml_props(root, shown, w),
        tilesets: Vec::new(),
        layers: Vec::new(),
        parallax_origin: [
            num(root, "parallaxoriginx", shown)?.unwrap_or(0.0),
            num(root, "parallaxoriginy", shown)?.unwrap_or(0.0),
        ],
    };
    for t in root.all("tileset") {
        let firstgid = num(t, "firstgid", shown)?.unwrap_or(0);
        let source = t.attr("source").map(str::to_string);
        let embedded = match source {
            Some(_) => None,
            None => Some(xml_tileset(t, shown)?),
        };
        raw.tilesets.push(TsRef {
            firstgid,
            source,
            embedded,
            at: t.at(shown),
        });
    }
    xml_walk(shown, root, &Ctx::top(), &mut raw, w)?;
    Ok(raw)
}

fn xml_walk(shown: &str, node: &Node, ctx: &Ctx, raw: &mut Raw, w: &mut Warns) -> Result<()> {
    for c in &node.children {
        if !matches!(
            c.name.as_str(),
            "layer" | "objectgroup" | "imagelayer" | "group"
        ) {
            continue;
        }
        let at = c.at(shown);
        let f = |k: &str, d: f64| -> Result<f64> { Ok(num(c, k, shown)?.unwrap_or(d)) };
        let lc = ctx.inner(
            c.attr("name").unwrap_or_default(),
            c.attr("visible") != Some("0"),
            [f("offsetx", 0.0)?, f("offsety", 0.0)?],
            xml_props(c, shown, w),
            f("opacity", 1.0)?,
            [f("parallaxx", 1.0)?, f("parallaxy", 1.0)?],
            c.attr("tintcolor").is_some(),
            &at,
            w,
        );
        match c.name.as_str() {
            "group" => {
                let mut g = lc.clone();
                g.prefix.push('/');
                xml_walk(shown, c, &g, raw, w)?;
            }
            "imagelayer" => raw.layers.push(Flat::Image {
                name: lc.prefix,
                visible: lc.visible,
                at,
            }),
            "layer" => {
                let data = c.child("data").ok_or_else(|| {
                    fail(format!("{at}: tile layer {:?} has no <data>", lc.prefix))
                        .at(Some(at.clone()))
                })?;
                let mut cells = Vec::new();
                let mut chunks = Vec::new();
                if raw.infinite {
                    for ch in data.all("chunk") {
                        let x: i64 = num(ch, "x", shown)?.unwrap_or(0);
                        let y: i64 = num(ch, "y", shown)?.unwrap_or(0);
                        near_origin(x, y, &ch.at(shown))?;
                        let cw: u32 = num(ch, "width", shown)?.unwrap_or(0);
                        let chh: u32 = num(ch, "height", shown)?.unwrap_or(0);
                        let mut enc = Node {
                            text: ch.text.clone(),
                            line: ch.line,
                            ..Node::default()
                        };
                        enc.attrs = data.attrs.clone();
                        enc.children = ch.children.iter().map(clone_tile).collect();
                        let gids = xml_gids(&enc, shown)?;
                        if gids.len() != cw as usize * chh as usize {
                            return Err(fail(format!(
                                "{}: a {cw}x{chh} chunk of layer {:?} has {} tiles",
                                ch.at(shown),
                                lc.prefix,
                                gids.len()
                            ))
                            .at(Some(ch.at(shown))));
                        }
                        dense(&gids, x, y, cw, &mut cells);
                        chunks.push([x, y, cw as i64, chh as i64]);
                    }
                } else {
                    let lw = num(c, "width", shown)?.unwrap_or(raw.width);
                    let lh: u32 = num(c, "height", shown)?.unwrap_or(raw.height);
                    let gids = xml_gids(data, shown)?;
                    if gids.len() != lw as usize * lh as usize {
                        return Err(fail(format!(
                            "{at}: layer {:?} has {} tiles for its {lw}x{lh} cells",
                            lc.prefix,
                            gids.len()
                        ))
                        .at(Some(at.clone())));
                    }
                    dense(&gids, 0, 0, lw, &mut cells);
                }
                raw.layers.push(Flat::Tiles(RawTiles {
                    name: lc.prefix,
                    visible: lc.visible,
                    offset: lc.offset,
                    parallax: lc.parallax,
                    props: lc.props,
                    cells,
                    chunks,
                    at,
                }));
            }
            _ => {
                let mut objects = Vec::new();
                for o in c.all("object") {
                    let shape = if o.child("ellipse").is_some() {
                        "ellipse"
                    } else if o.child("point").is_some() {
                        "point"
                    } else if o.child("polygon").is_some() {
                        "polygon"
                    } else if o.child("polyline").is_some() {
                        "polyline"
                    } else if o.child("text").is_some() {
                        "text"
                    } else if o.attr("gid").is_some() {
                        "tile"
                    } else {
                        "rect"
                    };
                    let points = o
                        .child("polygon")
                        .or_else(|| o.child("polyline"))
                        .and_then(|p| p.attr("points"))
                        .map(|s| {
                            s.split_whitespace()
                                .filter_map(|p| {
                                    let (x, y) = p.split_once(',')?;
                                    Some([x.parse().ok()?, y.parse().ok()?])
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let g = |k: &str| -> Result<f64> { Ok(num(o, k, shown)?.unwrap_or(0.0)) };
                    objects.push(RawObject {
                        id: num(o, "id", shown)?.unwrap_or(0),
                        name: o.attr("name").unwrap_or_default().to_string(),
                        class: o
                            .attr("class")
                            .or_else(|| o.attr("type"))
                            .unwrap_or_default()
                            .to_string(),
                        shape: shape.to_string(),
                        xy: [g("x")?, g("y")?],
                        wh: [g("width")?, g("height")?],
                        rotation: g("rotation")? as f32,
                        points,
                        gid: num(o, "gid", shown)?,
                        visible: o.attr("visible") != Some("0"),
                        props: xml_props(o, shown, w),
                        template: o.attr("template").is_some(),
                    });
                }
                raw.layers.push(Flat::Objects(RawObjects {
                    name: lc.prefix,
                    visible: lc.visible,
                    offset: lc.offset,
                    props: lc.props,
                    objects,
                    at,
                }));
            }
        }
    }
    Ok(())
}

/// An infinite map's chunk within a billion tiles of the origin, so the
/// sums that place its cells cannot overflow.
fn near_origin(x: i64, y: i64, at: &str) -> Result<()> {
    if x.abs() > 1 << 30 || y.abs() > 1 << 30 {
        return Err(fail(format!(
            "{at}: a chunk at ({x}, {y}) is too far from the map's origin"
        ))
        .at(Some(at.to_string())));
    }
    Ok(())
}

fn clone_tile(n: &Node) -> Node {
    Node {
        name: n.name.clone(),
        attrs: n.attrs.clone(),
        children: Vec::new(),
        text: String::new(),
        line: n.line,
    }
}

fn json_gids(v: &Json, layer: &Json, at: &str) -> Result<Vec<u32>> {
    match &v["data"] {
        Json::Array(a) => a
            .iter()
            .map(|g| {
                g.as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(|| {
                        fail(format!("{at}: {g} in a layer's data is not a tile id"))
                            .at(Some(at.to_string()))
                    })
            })
            .collect(),
        Json::String(s) => decode(
            at,
            Some(layer["encoding"].as_str().unwrap_or("base64")),
            layer["compression"].as_str(),
            s,
        ),
        Json::Null => Ok(Vec::new()),
        x => {
            Err(fail(format!("{at}: a layer's data is {x}, not tile ids")).at(Some(at.to_string())))
        }
    }
}

fn json_raw(shown: &str, v: &Json, w: &mut Warns) -> Result<Raw> {
    if !v.is_object() || v["type"].as_str().is_some_and(|t| t != "map") {
        return Err(fail(format!(
            "{shown}: not a Tiled map (a JSON object with type \"map\")"
        ))
        .at(Some(shown.to_string())));
    }
    let mut raw = Raw {
        orientation: v["orientation"]
            .as_str()
            .unwrap_or("orthogonal")
            .to_string(),
        width: json_u32(v, "width", shown)?,
        height: json_u32(v, "height", shown)?,
        tw: json_u32(v, "tilewidth", shown)?,
        th: json_u32(v, "tileheight", shown)?,
        infinite: v["infinite"].as_bool().unwrap_or(false),
        props: json_props(v, shown, w),
        tilesets: Vec::new(),
        layers: Vec::new(),
        parallax_origin: [
            v["parallaxoriginx"].as_f64().unwrap_or(0.0),
            v["parallaxoriginy"].as_f64().unwrap_or(0.0),
        ],
    };
    for t in v["tilesets"].as_array().into_iter().flatten() {
        let firstgid = json_u32(t, "firstgid", shown)?;
        let source = t["source"].as_str().map(str::to_string);
        let embedded = match source {
            Some(_) => None,
            None => Some(json_tileset(t, shown)?),
        };
        raw.tilesets.push(TsRef {
            firstgid,
            source,
            embedded,
            at: shown.to_string(),
        });
    }
    json_walk(shown, v, &Ctx::top(), &mut raw, w)?;
    Ok(raw)
}

fn json_walk(shown: &str, node: &Json, ctx: &Ctx, raw: &mut Raw, w: &mut Warns) -> Result<()> {
    for l in node["layers"].as_array().into_iter().flatten() {
        let kind = l["type"].as_str().unwrap_or_default();
        if !matches!(kind, "tilelayer" | "objectgroup" | "imagelayer" | "group") {
            continue;
        }
        let at = shown.to_string();
        let f = |k: &str, d: f64| l[k].as_f64().unwrap_or(d);
        let lc = ctx.inner(
            l["name"].as_str().unwrap_or_default(),
            l["visible"].as_bool().unwrap_or(true),
            [f("offsetx", 0.0), f("offsety", 0.0)],
            json_props(l, &at, w),
            f("opacity", 1.0),
            [f("parallaxx", 1.0), f("parallaxy", 1.0)],
            l["tintcolor"].is_string(),
            &at,
            w,
        );
        match kind {
            "group" => {
                let mut g = lc.clone();
                g.prefix.push('/');
                json_walk(shown, l, &g, raw, w)?;
            }
            "imagelayer" => raw.layers.push(Flat::Image {
                name: lc.prefix,
                visible: lc.visible,
                at,
            }),
            "tilelayer" => {
                let mut cells = Vec::new();
                let mut chunks = Vec::new();
                if let Some(chs) = l["chunks"].as_array() {
                    for ch in chs {
                        let x = ch["x"].as_i64().unwrap_or(0);
                        let y = ch["y"].as_i64().unwrap_or(0);
                        near_origin(x, y, &at)?;
                        let cw = json_u32(ch, "width", &at)?;
                        let chh = json_u32(ch, "height", &at)?;
                        let gids = json_gids(ch, l, &at)?;
                        if gids.len() != cw as usize * chh as usize {
                            return Err(fail(format!(
                                "{at}: a {cw}x{chh} chunk of layer {:?} has {} tiles",
                                lc.prefix,
                                gids.len()
                            ))
                            .at(Some(at.clone())));
                        }
                        dense(&gids, x, y, cw, &mut cells);
                        chunks.push([x, y, cw as i64, chh as i64]);
                    }
                } else {
                    let lw = match json_u32(l, "width", &at)? {
                        0 => raw.width,
                        n => n,
                    };
                    let lh = match json_u32(l, "height", &at)? {
                        0 => raw.height,
                        n => n,
                    };
                    let gids = json_gids(l, l, &at)?;
                    if gids.len() != lw as usize * lh as usize {
                        return Err(fail(format!(
                            "{at}: layer {:?} has {} tiles for its {lw}x{lh} cells",
                            lc.prefix,
                            gids.len()
                        ))
                        .at(Some(at.clone())));
                    }
                    dense(&gids, 0, 0, lw, &mut cells);
                }
                raw.layers.push(Flat::Tiles(RawTiles {
                    name: lc.prefix,
                    visible: lc.visible,
                    offset: lc.offset,
                    parallax: lc.parallax,
                    props: lc.props,
                    cells,
                    chunks,
                    at,
                }));
            }
            _ => {
                let mut objects = Vec::new();
                for o in l["objects"].as_array().into_iter().flatten() {
                    let shape = if o["ellipse"].as_bool() == Some(true) {
                        "ellipse"
                    } else if o["point"].as_bool() == Some(true) {
                        "point"
                    } else if o["polygon"].is_array() {
                        "polygon"
                    } else if o["polyline"].is_array() {
                        "polyline"
                    } else if o["text"].is_object() {
                        "text"
                    } else if o["gid"].is_u64() {
                        "tile"
                    } else {
                        "rect"
                    };
                    let points = o["polygon"]
                        .as_array()
                        .or_else(|| o["polyline"].as_array())
                        .into_iter()
                        .flatten()
                        .map(|p| {
                            [
                                p["x"].as_f64().unwrap_or(0.0),
                                p["y"].as_f64().unwrap_or(0.0),
                            ]
                        })
                        .collect();
                    let g = |k: &str| o[k].as_f64().unwrap_or(0.0);
                    objects.push(RawObject {
                        id: o["id"].as_u64().unwrap_or(0) as u32,
                        name: o["name"].as_str().unwrap_or_default().to_string(),
                        class: o["class"]
                            .as_str()
                            .or_else(|| o["type"].as_str())
                            .unwrap_or_default()
                            .to_string(),
                        shape: shape.to_string(),
                        xy: [g("x"), g("y")],
                        wh: [g("width"), g("height")],
                        rotation: g("rotation") as f32,
                        points,
                        gid: o["gid"].as_u64().and_then(|n| u32::try_from(n).ok()),
                        visible: o["visible"].as_bool().unwrap_or(true),
                        props: json_props(o, &at, w),
                        template: o["template"].is_string(),
                    });
                }
                raw.layers.push(Flat::Objects(RawObjects {
                    name: lc.prefix,
                    visible: lc.visible,
                    offset: lc.offset,
                    props: lc.props,
                    objects,
                    at,
                }));
            }
        }
    }
    Ok(())
}

/// The sprite names of a map's embedded tilesets, by their place in its
/// tileset list (a name taken twice gets the first id after it).
fn embedded_names(path: &str, tilesets: &[TsRef]) -> Vec<Option<String>> {
    let mut seen = BTreeSet::new();
    tilesets
        .iter()
        .map(|t| {
            let ts = t.embedded.as_ref()?;
            let mut n = embedded_name(path, &ts.name, t.firstgid);
            if !seen.insert(n.clone()) {
                n = format!("{n}_{}", t.firstgid);
                seen.insert(n.clone());
            }
            Some(n)
        })
        .collect()
}

fn raw_of(shown: &str, bytes: &[u8], w: &mut Warns) -> Result<Raw> {
    let text = text_of(shown, bytes)?;
    if ext(shown) == "tmj" {
        let v = parse_json(shown, text)?;
        json_raw(shown, &v, w)
    } else {
        let root = parse_xml(shown, text)?;
        xml_raw(shown, &root, w)
    }
}

/// The embedded tilesets of the map at `path` (root-relative; `shown` names
/// it in messages): each one's sprite name and the tileset.
pub fn embedded(shown: &str, path: &str, bytes: &[u8]) -> Result<Vec<(String, Tileset)>> {
    let mut w = Warns { out: Vec::new() };
    let raw = raw_of(shown, bytes, &mut w)?;
    let names = embedded_names(path, &raw.tilesets);
    Ok(raw
        .tilesets
        .into_iter()
        .zip(names)
        .filter_map(|(t, n)| Some((n?, t.embedded?)))
        .collect())
}

/// Read a Tiled map into the engine's terms. `path` is the map's file from
/// the root its tileset paths resolve in (`assets/maps/level1.tmx`), and
/// `shown` names it in messages; `load` reads an external tileset by its
/// root-relative path (for its name and tile count: the cells need only
/// the map). Refused with why: a map that is not orthogonal, tiles that
/// are not square, data the engine cannot decode (zstd), or a file that
/// does not parse.
pub fn read_map(
    shown: &str,
    path: &str,
    bytes: &[u8],
    load: &mut dyn FnMut(&str) -> std::result::Result<Vec<u8>, String>,
) -> Result<TiledMap> {
    let mut w = Warns { out: Vec::new() };
    let raw = raw_of(shown, bytes, &mut w)?;
    if raw.orientation != "orthogonal" {
        return Err(fail(format!(
            "{shown}: the map is {}; the engine's maps are a square grid (orthogonal)",
            raw.orientation
        ))
        .hint("Make an orthogonal map in Tiled, or keep this one out of the game.")
        .at(Some(shown.to_string())));
    }
    if raw.tw == 0 || raw.tw != raw.th {
        return Err(fail(format!(
            "{shown}: its tiles are {}x{} px; the engine's tiles are square",
            raw.tw, raw.th
        ))
        .hint("Give the map square tiles in Tiled (Map > Map Properties).")
        .at(Some(shown.to_string())));
    }
    let tile_size = match raw.props.get("tile_size") {
        None => 1.0,
        Some(s) => match s.trim().parse::<f32>() {
            Ok(v) if v > 0.0 && v.is_finite() => v,
            _ => {
                w.add(
                    shown,
                    format!("tile_size = {s:?} is not a size; tiles are 1 world unit"),
                );
                1.0
            }
        },
    };
    // Tilesets: the sprite each draws from, its name, its tile count.
    let names = embedded_names(path, &raw.tilesets);
    let mut uses: Vec<TilesetUse> = Vec::new();
    let mut collections: BTreeSet<usize> = BTreeSet::new();
    for (t, emb) in raw.tilesets.iter().zip(names) {
        let mut ts: Option<Tileset> = t.embedded.clone();
        let (asset, source) = match (&t.source, emb) {
            (_, Some(n)) => {
                if under_assets(path).is_none() {
                    w.add(&t.at, format!("embedded tileset {:?} is drawn only when the map is under assets/, where the importer makes it a sprite; move the map there or save the tileset as a file under assets/", ts.as_ref().map(|t| t.name.as_str()).unwrap_or_default()));
                }
                (n, None)
            }
            (Some(src), None) => match resolve(path, src) {
                None => {
                    w.add(
                        &t.at,
                        format!(
                            "tileset {src:?} is outside the project; its tiles cannot be drawn"
                        ),
                    );
                    (strip_ext(src).to_string(), None)
                }
                Some(p) => {
                    let asset = match asset_of(&p) {
                        Some(a) => a,
                        None => {
                            w.add(&t.at, format!("tileset {p} is outside assets/, so no sprite asset: its tiles cannot be drawn; move it under assets/"));
                            strip_ext(&p).to_string()
                        }
                    };
                    match load(&p) {
                        Ok(b) => match read_tileset(&p, &b) {
                            Ok(x) => ts = Some(x),
                            Err(e) => {
                                w.add(&t.at, format!("tileset {p} does not read: {}", e.message))
                            }
                        },
                        Err(e) => w.add(&t.at, format!("tileset {p} cannot be read: {e}")),
                    }
                    (asset, Some(p))
                }
            },
            (None, None) => unreachable!("a tileset is embedded or a file"),
        };
        if let Some(x) = &ts {
            if x.is_collection() {
                collections.insert(uses.len());
                w.add(&t.at, format!("tileset {:?} is an image collection (an image per tile); its tiles are not drawn", x.name));
            }
            if (x.tile_width, x.tile_height) != (raw.tw, raw.th) && x.tile_width > 0 {
                w.add(
                    &t.at,
                    format!(
                        "tileset {:?} has {}x{} px tiles and the map {}x{}; each draws in one cell",
                        x.name, x.tile_width, x.tile_height, raw.tw, raw.th
                    ),
                );
            }
        }
        uses.push(TilesetUse {
            firstgid: t.firstgid,
            name: ts
                .as_ref()
                .map(|x| x.name.clone())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| asset.rsplit('/').next().unwrap_or_default().to_string()),
            source,
            asset,
            tile_count: ts.as_ref().map(|x| x.tile_count).filter(|&n| n > 0),
            embedded: t.embedded.is_some(),
        });
    }
    let mut order: Vec<usize> = (0..uses.len()).collect();
    order.sort_by_key(|&i| uses[i].firstgid);
    let firsts: Vec<u32> = order.iter().map(|&i| uses[i].firstgid).collect();
    let find = |gid: u32| -> Option<usize> {
        match firsts.binary_search(&gid) {
            Ok(i) => Some(order[i]),
            Err(0) => None,
            Err(i) => Some(order[i - 1]),
        }
    };
    let tpx = raw.tw as f64;
    // Each tile layer's shift in whole tiles.
    let shift = |offset: [f64; 2], name: &str, at: &str, w: &mut Warns| -> [i64; 2] {
        let s = [offset[0] / tpx, offset[1] / tpx];
        let r = [
            s[0].round().clamp(-1e9, 1e9) as i64,
            s[1].round().clamp(-1e9, 1e9) as i64,
        ];
        if s[0].fract() != 0.0 || s[1].fract() != 0.0 {
            w.add(at, format!("layer {name:?} is offset ({}, {}) px, not whole tiles; it is moved to ({}, {}) tiles", offset[0], offset[1], r[0], r[1]));
        }
        r
    };
    // The engine's grid: the map, or an infinite map's chunks.
    let (x0, y0, width, height) = if raw.infinite {
        let mut b: Option<[i64; 4]> = None;
        for l in &raw.layers {
            let Flat::Tiles(t) = l else { continue };
            let s = shift(t.offset, &t.name, &t.at, &mut Warns { out: Vec::new() });
            for c in &t.chunks {
                let r = [
                    c[0] + s[0],
                    c[1] + s[1],
                    c[0] + s[0] + c[2],
                    c[1] + s[1] + c[3],
                ];
                b = Some(match b {
                    None => r,
                    Some(o) => [
                        o[0].min(r[0]),
                        o[1].min(r[1]),
                        o[2].max(r[2]),
                        o[3].max(r[3]),
                    ],
                });
            }
        }
        let b = b.unwrap_or([0, 0, 1, 1]);
        (b[0], b[1], (b[2] - b[0]).max(1), (b[3] - b[1]).max(1))
    } else {
        (0, 0, raw.width as i64, raw.height as i64)
    };
    if width < 1 || height < 1 || width > 1 << 20 || height > 1 << 20 {
        return Err(fail(format!("{shown}: the map is {width}x{height} tiles"))
            .hint("A map is at least 1x1 tiles.")
            .at(Some(shown.to_string())));
    }
    let mut out = TiledMap {
        format: ext(shown),
        width: width as u32,
        height: height as u32,
        tile_px: raw.tw,
        tile_size,
        infinite: raw.infinite,
        origin: [x0, y0],
        parallax_origin: [
            ((raw.parallax_origin[0] / tpx - x0 as f64) * tile_size as f64) as f32,
            (-(raw.parallax_origin[1] / tpx - y0 as f64) * tile_size as f64) as f32,
        ],
        properties: raw.props.clone(),
        tilesets: uses,
        tiled_layers: Vec::new(),
        layers: Vec::new(),
        objects: Vec::new(),
        warnings: Vec::new(),
    };
    let unit = tile_size as f64 / tpx;
    let mut names: BTreeSet<String> = BTreeSet::new();
    let mut tile_layer = 0usize;
    for l in raw.layers {
        match l {
            Flat::Image { name, visible, at } => {
                w.add(
                    &at,
                    format!("image layer {name:?} is not imported; place its picture as a sprite"),
                );
                out.tiled_layers.push(TiledLayer {
                    name,
                    kind: "image".into(),
                    visible,
                    tilesets: Vec::new(),
                    layers: Vec::new(),
                });
            }
            Flat::Tiles(t) => {
                let s = shift(t.offset, &t.name, &t.at, &mut w);
                let mut by_ts: BTreeMap<usize, Vec<[u32; 3]>> = BTreeMap::new();
                let (mut flipped, mut unknown, mut beyond, mut outside) =
                    (0usize, 0usize, 0usize, 0usize);
                for &(x, y, gid) in &t.cells {
                    if gid & FLAGS != 0 {
                        flipped += 1;
                    }
                    let g = gid & !FLAGS;
                    if g == 0 {
                        continue;
                    }
                    let Some(ti) = find(g) else {
                        unknown += 1;
                        continue;
                    };
                    let local = g - out.tilesets[ti].firstgid + 1;
                    if out.tilesets[ti].tile_count.is_some_and(|n| local > n) {
                        beyond += 1;
                    }
                    let (cx, cy) = (x + s[0] - x0, y + s[1] - y0);
                    if cx < 0 || cy < 0 || cx >= width || cy >= height {
                        outside += 1;
                        continue;
                    }
                    by_ts
                        .entry(ti)
                        .or_default()
                        .push([cx as u32, cy as u32, local]);
                }
                if flipped > 0 {
                    w.add(&t.at, format!("layer {:?} has {flipped} flipped or rotated tiles; the engine draws tiles unflipped, so the flags are dropped", t.name));
                }
                if unknown > 0 {
                    w.add(&t.at, format!("layer {:?} has {unknown} tiles of no tileset the map lists; they are left out", t.name));
                }
                if beyond > 0 {
                    w.add(
                        &t.at,
                        format!(
                            "layer {:?} has {beyond} tiles past the end of their tileset",
                            t.name
                        ),
                    );
                }
                if outside > 0 {
                    w.add(&t.at, format!("layer {:?} has {outside} tiles moved off the map by its offset; they are left out", t.name));
                }
                for ti in by_ts.keys().copied().collect::<Vec<_>>() {
                    if collections.contains(&ti) {
                        by_ts.remove(&ti);
                    }
                }
                let z = match t.props.get("z") {
                    None => tile_layer as f32,
                    Some(v) => match v.trim().parse::<f32>() {
                        Ok(z) if z.is_finite() => z,
                        _ => {
                            w.add(&t.at, format!("layer {:?}: z = {v:?} is not a number; it draws at {tile_layer}", t.name));
                            tile_layer as f32
                        }
                    },
                };
                tile_layer += 1;
                let collision = truthy(t.props.get("collision"));
                let base = if t.name.is_empty() {
                    format!("layer {tile_layer}")
                } else {
                    t.name.clone()
                };
                let split = by_ts.len() > 1;
                let parts: Vec<(Option<usize>, Vec<[u32; 3]>)> = if by_ts.is_empty() {
                    vec![(None, Vec::new())]
                } else {
                    by_ts.into_iter().map(|(k, v)| (Some(k), v)).collect()
                };
                let mut made = Vec::new();
                let mut used = Vec::new();
                for (ti, cells) in parts {
                    let mut name = match (split, ti) {
                        (true, Some(i)) => format!("{base} ({})", out.tilesets[i].name),
                        _ => base.clone(),
                    };
                    if names.contains(&name) {
                        let mut k = 2;
                        while names.contains(&format!("{name} {k}")) {
                            k += 1;
                        }
                        let taken = name.clone();
                        name = format!("{name} {k}");
                        w.add(
                            &t.at,
                            format!("two layers are named {taken:?}; this one is {name:?}"),
                        );
                    }
                    names.insert(name.clone());
                    if let Some(i) = ti {
                        used.push(out.tilesets[i].name.clone());
                    }
                    made.push(name.clone());
                    out.layers.push(MapLayer {
                        name,
                        from: t.name.clone(),
                        tileset: ti.map(|i| out.tilesets[i].asset.clone()),
                        z,
                        collision,
                        parallax: [t.parallax[0] as f32, t.parallax[1] as f32],
                        visible: t.visible,
                        properties: t.props.clone(),
                        cells,
                    });
                }
                out.tiled_layers.push(TiledLayer {
                    name: t.name,
                    kind: "tiles".into(),
                    visible: t.visible,
                    tilesets: used,
                    layers: made,
                });
            }
            Flat::Objects(o) => {
                let ox = (x0 as f64) * tpx - o.offset[0];
                let oy = (y0 as f64) * tpx - o.offset[1];
                let mut objects = Vec::new();
                let mut templates = 0;
                for b in o.objects {
                    if b.template {
                        templates += 1;
                    }
                    let tile = b.gid.and_then(|gid| {
                        let g = gid & !FLAGS;
                        let ti = find(g)?;
                        let u = &out.tilesets[ti];
                        Some((u.asset.clone(), g - u.firstgid + 1))
                    });
                    // A tile object's (x, y) is its bottom-left corner.
                    let top = if b.gid.is_some() {
                        b.xy[1] - b.wh[1]
                    } else {
                        b.xy[1]
                    };
                    objects.push(MapObject {
                        id: b.id,
                        name: b.name,
                        class: b.class,
                        shape: b.shape,
                        pos: [((b.xy[0] - ox) * unit) as f32, (-(top - oy) * unit) as f32],
                        size: [(b.wh[0] * unit) as f32, (b.wh[1] * unit) as f32],
                        rotation: b.rotation,
                        points: b
                            .points
                            .iter()
                            .map(|p| [(p[0] * unit) as f32, (-p[1] * unit) as f32])
                            .collect(),
                        tile,
                        visible: b.visible,
                        properties: b.props,
                    });
                }
                if templates > 0 {
                    w.add(&o.at, format!("object layer {:?} has {templates} objects made from templates; what only the template says is not read", o.name));
                }
                out.tiled_layers.push(TiledLayer {
                    name: o.name.clone(),
                    kind: "objects".into(),
                    visible: o.visible,
                    tilesets: Vec::new(),
                    layers: Vec::new(),
                });
                out.objects.push(ObjectLayer {
                    name: o.name,
                    visible: o.visible,
                    properties: o.props,
                    objects,
                });
            }
        }
    }
    out.warnings = w.out;
    Ok(out)
}

// ----- the importer -----------------------------------------------------------

/// What a map asset is: its file and what it holds.
#[derive(Clone, Debug, PartialEq)]
pub struct MapAsset {
    /// The map file, relative to `assets/`.
    pub file: String,
    /// What it holds and what the engine makes of it ([`TiledMap::info`]).
    pub info: Json,
    /// The engine's layers it becomes.
    pub layers: Vec<String>,
}

impl Importer {
    /// `groups` plus one for each embedded tileset of the maps among them
    /// (`maps/level1.tmx/terrain`), so each is a sprite asset with its own
    /// import, stamps, and removal. A map that does not read adds none;
    /// its own import says why.
    pub(crate) fn expand_tiled(
        &self,
        layer: usize,
        mut groups: BTreeMap<String, Group>,
    ) -> BTreeMap<String, Group> {
        let maps: Vec<String> = groups
            .values()
            .filter(|g| !g.embedded)
            .filter_map(|g| g.tiled.clone())
            .filter(|t| is_map_file(t))
            .collect();
        for rel in maps {
            let Ok(bytes) = argus_os::fs::read(self.dir(layer).join(&rel)) else {
                continue;
            };
            let path = format!("assets/{rel}");
            let Ok(found) = embedded(&path, &path, &bytes) else {
                continue;
            };
            for (name, _) in found {
                groups.entry(name).or_insert_with(|| Group {
                    layer,
                    tiled: Some(rel.clone()),
                    embedded: true,
                    ..Group::default()
                });
            }
        }
        groups
    }

    /// A Tiled file's asset: a tileset's sprite, a map, or one of a map's
    /// embedded tilesets' sprites.
    pub(crate) fn import_tiled(
        &self,
        name: &str,
        g: &Group,
        file: &str,
        side: Option<&str>,
        side_file: Option<&str>,
        mut hash: u64,
    ) -> Result<(Entry, Vec<String>)> {
        let shown = self.shown(g.layer, file);
        let others: Vec<&String> = [&g.wav, &g.wgsl, &g.font, &g.aseprite]
            .into_iter()
            .flatten()
            .chain(g.png.iter().filter(|_| is_map_file(file)))
            .collect();
        if let Some(o) = others.first() {
            return Err(ArgusError::new(
                codes::AMBIGUOUS,
                format!(
                    "{name} has both {shown} and {}; one name is one asset",
                    self.shown(g.layer, o)
                ),
            )
            .hint("Rename one of them.")
            .at(Some(shown)));
        }
        let bytes = self.read_in(g.layer, file)?;
        fnv(&mut hash, &bytes);
        let path = format!("assets/{file}");
        if g.embedded {
            let found = embedded(&shown, &path, &bytes)?;
            let ts = found
                .into_iter()
                .find(|(n, _)| n == name)
                .map(|(_, t)| t)
                .ok_or_else(|| {
                    ArgusError::new(
                        codes::UNKNOWN_ASSET,
                        format!("{shown} no longer embeds the tileset {name}"),
                    )
                    .at(Some(shown.clone()))
                })?;
            return self.tileset_sprite(name, g, &ts, &path, None, None, hash);
        }
        if is_tileset_file(file) {
            let ts = read_tileset(&shown, &bytes)?;
            return self.tileset_sprite(name, g, &ts, &path, side, side_file, hash);
        }
        // A map: what it holds, as the engine will read it.
        let root = self.layers[g.layer].root.clone();
        let mut inputs = Vec::new();
        let mut read = Vec::new();
        let mut load = |p: &str| -> std::result::Result<Vec<u8>, String> {
            let b = argus_os::fs::read(root.join(p)).map_err(|e| e.to_string())?;
            if let Some(r) = under_assets(p) {
                inputs.push(r.to_string());
            }
            read.push(b.clone());
            Ok(b)
        };
        let map = read_map(&shown, &path, &bytes, &mut load)?;
        for b in &read {
            fnv(&mut hash, b);
        }
        let mut warnings = map.warnings.clone();
        if let Some(sf) = side_file {
            warnings.push(Warning {
                message: "a map's sidecar is not read; its custom properties are in the map".into(),
                at: sf.to_string(),
            });
        }
        Ok((
            Entry {
                name: name.to_string(),
                sidecar: None,
                hash,
                warnings,
                asset: Asset::Map(MapAsset {
                    file: file.to_string(),
                    info: map.info(),
                    layers: map.layers.iter().map(|l| l.name.clone()).collect(),
                }),
                ms: 0.0,
            },
            inputs,
        ))
    }

    /// A tileset as a sprite: its image cut into its tiles, each tile's
    /// properties its frame's, animations as clips, and a sidecar's
    /// auto-tiling rules and frame overrides over it. `path` is the file
    /// holding the tileset, from the layer's root.
    #[allow(clippy::too_many_arguments)]
    fn tileset_sprite(
        &self,
        name: &str,
        g: &Group,
        ts: &Tileset,
        path: &str,
        side: Option<&str>,
        side_file: Option<&str>,
        mut hash: u64,
    ) -> Result<(Entry, Vec<String>)> {
        let mut warnings = ts.warnings.clone();
        let warn = |w: &mut Vec<Warning>, m: String, at: &str| {
            w.push(Warning {
                message: m,
                at: at.to_string(),
            })
        };
        if ts.is_collection() {
            return Err(ArgusError::new(
                codes::BAD_SOURCE,
                format!("{}: tileset {:?} is an image collection (an image per tile); the engine draws a tileset from one sheet", ts.at, ts.name),
            )
            .hint("Make the tiles one image and the tileset \"Based on Tileset Image\" in Tiled, or keep each image a sprite of its own.")
            .at(Some(ts.at.clone())));
        }
        let image = ts.image.as_deref().ok_or_else(|| {
            ArgusError::new(
                codes::BAD_SOURCE,
                format!("{}: tileset {:?} has no image", ts.at, ts.name),
            )
            .at(Some(ts.at.clone()))
        })?;
        let img_rel = resolve(path, image)
            .as_deref()
            .and_then(under_assets)
            .map(str::to_string)
            .ok_or_else(|| {
                ArgusError::new(codes::BAD_SOURCE, format!("{}: the image {image:?} is outside assets/", ts.at))
                    .hint("A tileset's image is an asset's image: keep it under assets/, where a release build takes it from.")
                    .at(Some(ts.at.clone()))
            })?;
        if !img_rel.to_ascii_lowercase().ends_with(".png") {
            return Err(ArgusError::new(
                codes::BAD_SOURCE,
                format!(
                    "{}: the image {image:?} is not a PNG; only PNG images import",
                    ts.at
                ),
            )
            .hint("Save the sheet as a PNG and point the tileset at it.")
            .at(Some(ts.at.clone())));
        }
        if let Some(p) = g.png.as_ref().filter(|p| **p != img_rel) {
            return Err(ArgusError::new(
                codes::AMBIGUOUS,
                format!("{name} has both {} and {}, which is not its tileset's image; one name is one asset", self.shown(g.layer, path.trim_start_matches("assets/")), self.shown(g.layer, p)),
            )
            .hint("Rename one of them.")
            .at(Some(self.shown(g.layer, p))));
        }
        let img_bytes = self.read_in(g.layer, &img_rel)?;
        fnv(&mut hash, &img_bytes);
        let at_img = self.shown(g.layer, &img_rel);
        let (w, h, rgba) = crate::decode_png(&self.dir(g.layer).join(&img_rel))
            .map_err(|e| e.at(Some(at_img.clone())))?;
        let rects = ts.rects(w, h);
        if ts.tile_count > 0 && (rects.len() as u32) < ts.tile_count {
            warn(&mut warnings, format!("the tileset has {} tiles and the {w}x{h} image holds {}; the rest are left out", ts.tile_count, rects.len()), &ts.at);
        }
        if rects.is_empty() {
            return Err(ArgusError::new(
                codes::BAD_SOURCE,
                format!(
                    "{}: no {}x{} tile fits the {w}x{h} image",
                    ts.at, ts.tile_width, ts.tile_height
                ),
            )
            .at(Some(ts.at.clone())));
        }
        let ppu = ts
            .properties
            .get("pixels_per_unit")
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|v| *v > 0.0)
            .unwrap_or(ts.tile_width.max(1) as f32);
        let mut frames: Vec<Frame> = rects
            .iter()
            .enumerate()
            .map(|(i, r)| Frame {
                rect: *r,
                pivot: [0.5, 0.5],
                duration_ms: 100,
                properties: ts
                    .tiles
                    .get(&(i as u32))
                    .map(|t| t.properties.clone())
                    .unwrap_or_default(),
            })
            .collect();
        let mut shapes = Vec::new();
        for (id, t) in &ts.tiles {
            if t.shapes
                && !t.properties.contains_key("solid")
                && !t.properties.contains_key("one_way")
                && !t.properties.contains_key("slope")
            {
                shapes.push(*id);
            }
            if *id as usize >= frames.len() && !t.properties.is_empty() {
                warn(
                    &mut warnings,
                    format!(
                        "tile {id} has properties but the image holds {} tiles",
                        frames.len()
                    ),
                    &ts.at,
                );
            }
        }
        if !shapes.is_empty() {
            warn(&mut warnings, format!("tiles {shapes:?} have collision shapes drawn in Tiled; the engine collides by a tile's solid, one_way, or slope property"), &ts.at);
        }
        // Animations as clips, named by the tile's class or tile<id>.
        let mut clips = BTreeMap::new();
        let mut anims = Vec::new();
        for (id, t) in &ts.tiles {
            if t.animation.is_empty() {
                continue;
            }
            let steps: Vec<(u32, u32)> = t
                .animation
                .iter()
                .copied()
                .filter(|(f, _)| (*f as usize) < frames.len())
                .collect();
            if steps.len() < t.animation.len() || steps.is_empty() {
                warn(
                    &mut warnings,
                    format!("tile {id}'s animation names tiles the image does not hold"),
                    &ts.at,
                );
                if steps.is_empty() {
                    continue;
                }
            }
            let mut clip = if t.class.is_empty() {
                format!("tile{id}")
            } else {
                t.class.clone()
            };
            if clips.contains_key(&clip) {
                clip = format!("tile{id}");
            }
            let first = steps[0].1.max(1);
            let fps = if steps.iter().all(|s| s.1 == steps[0].1) {
                Some(1000.0 / first as f32)
            } else {
                // Uneven steps: each frame's own duration, where the frame
                // has one duration in every animation.
                let mut ok = true;
                for &(f, d) in &steps {
                    let fr = &mut frames[f as usize];
                    if fr.duration_ms != 100 && fr.duration_ms != d {
                        ok = false;
                    }
                    fr.duration_ms = d;
                }
                if !ok {
                    warn(&mut warnings, format!("tile {id}'s animation shares a tile with another at another duration; the durations are approximate"), &ts.at);
                }
                None
            };
            clips.insert(
                clip.clone(),
                Clip {
                    frames: steps.iter().map(|s| s.0).collect(),
                    fps,
                    looping: true,
                    direction: Direction::Forward,
                },
            );
            anims.push(clip);
        }
        if !anims.is_empty() {
            warn(&mut warnings, format!("tile animations became clips ({}); a map layer draws its tiles still, a Sprite plays a clip", anims.join(", ")), &ts.at);
        }
        // A sidecar over it: auto-tiling rules and frame overrides.
        let mut autotile = BTreeMap::new();
        let mut ppu_out = ppu;
        if let Some(text) = side {
            let sf = side_file.unwrap_or("");
            let sc = crate::sidecar::parse_sprite(sf, text)?;
            for k in &sc.unknown {
                warn(&mut warnings, format!("unknown key {k:?}, ignored"), sf);
            }
            if sc.grid.is_some() || sc.frames.iter().any(|f| f.rect.is_some()) || sc.shape.is_some()
            {
                warn(&mut warnings, "grid, frame rects, and shapes are ignored: the tileset says where its tiles are".into(), sf);
            }
            if let Some(p) = sc.pixels_per_unit {
                ppu_out = p;
            }
            for f in &sc.frames {
                let Some(i) = f.index else { continue };
                let Some(fr) = frames.get_mut(i as usize) else {
                    warn(
                        &mut warnings,
                        format!("frame {i} does not exist; the tileset has {}", frames.len()),
                        &f.at,
                    );
                    continue;
                };
                if let Some(p) = f.pivot.or(sc.pivot) {
                    fr.pivot = [p[0].clamp(0.0, 1.0), p[1].clamp(0.0, 1.0)];
                }
                if let Some(d) = f.duration_ms {
                    fr.duration_ms = d;
                }
                fr.properties.extend(f.properties.clone());
            }
            for c in &sc.clips {
                let keep: Vec<u32> = c
                    .frames
                    .iter()
                    .copied()
                    .filter(|&f| (f as usize) < frames.len())
                    .collect();
                if keep.len() < c.frames.len() {
                    warn(
                        &mut warnings,
                        format!("clip {} names frames the tileset does not have", c.name),
                        &c.at,
                    );
                }
                if !keep.is_empty() {
                    clips.insert(
                        c.name.clone(),
                        Clip {
                            frames: keep,
                            fps: c.fps,
                            looping: c.looping,
                            direction: c.direction,
                        },
                    );
                }
            }
            for a in &sc.autotile {
                if let Some(bad) = a.tiles.iter().find(|&&t| t as usize > frames.len()) {
                    return Err(ArgusError::new(
                        codes::BAD_SIDECAR,
                        format!("autotile.{} names tile {bad}; the tileset has {} tiles (tile n is Tiled's tile n - 1)", a.terrain, frames.len()),
                    )
                    .at(Some(a.at.clone())));
                }
                autotile.insert(a.terrain.clone(), a.clone());
            }
        }
        for (i, fr) in frames.iter().enumerate() {
            if let Some(s) = fr.properties.get("slope") {
                if crate::parse_slope(s).is_none() {
                    warn(&mut warnings, format!("tile {i}: slope = {s} is not [left, right] with heights from 0 to 1; the tile is solid"), &ts.at);
                }
            }
        }
        let empty: Vec<usize> = frames
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                let [x, y, fw, fh] = f.rect;
                (y..y + fh)
                    .all(|yy| (x..x + fw).all(|xx| rgba[((yy * w + xx) * 4 + 3) as usize] == 0))
            })
            .map(|(i, _)| i)
            .collect();
        if !empty.is_empty() {
            let shown: Vec<String> = empty.iter().take(12).map(|i| i.to_string()).collect();
            warn(
                &mut warnings,
                format!(
                    "{} tiles are empty ({}{})",
                    empty.len(),
                    shown.join(", "),
                    if empty.len() > 12 { ", ..." } else { "" }
                ),
                &at_img,
            );
        }
        let (texture, texture_size, normal) =
            self.cache_texture(name, g, w, h, rgba, &at_img, &mut warnings, &mut hash)?;
        let inputs = if g.png.as_deref() == Some(img_rel.as_str()) {
            Vec::new()
        } else {
            vec![img_rel.clone()]
        };
        Ok((
            Entry {
                name: name.to_string(),
                sidecar: if g.embedded { None } else { g.sidecar.clone() },
                hash,
                warnings,
                asset: Asset::Sprite(SpriteAsset {
                    source: Some(img_rel),
                    width: w,
                    height: h,
                    texture,
                    texture_size,
                    normal,
                    frames,
                    clips,
                    pixels_per_unit: ppu_out,
                    shape: None,
                    autotile,
                    aseprite: None,
                }),
                ms: 0.0,
            },
            inputs,
        ))
    }

    /// Pad an image to powers of two, cache it as the sprite's texture, and
    /// its normal map beside it when the group has one.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn cache_texture(
        &self,
        name: &str,
        g: &Group,
        w: u32,
        h: u32,
        rgba: Vec<u8>,
        at: &str,
        warnings: &mut Vec<Warning>,
        hash: &mut u64,
    ) -> Result<(std::path::PathBuf, [u32; 2], Option<std::path::PathBuf>)> {
        let (tw, th) = (crate::pow2(w), crate::pow2(h));
        let pad = |px: &[u8]| crate::pad(px, w, h, tw, th);
        let rgba = if (tw, th) != (w, h) {
            warnings.push(Warning {
                message: format!("texture {w}x{h} is not power-of-two, padding to {tw}x{th}"),
                at: at.to_string(),
            });
            pad(&rgba)
        } else {
            rgba
        };
        let texture = self.cache.join(format!("{name}.png"));
        crate::encode_png(&texture, tw, th, &rgba)?;
        let normal = match g.normal.as_ref().filter(|_| !g.embedded) {
            None => None,
            Some(np) => {
                let bytes = self.read_in(g.layer, np)?;
                fnv(hash, &bytes);
                let shown = self.shown(g.layer, np);
                let (nw, nh, n) = crate::decode_png(&self.dir(g.layer).join(np))
                    .map_err(|e| e.at(Some(shown.clone())))?;
                if (nw, nh) != (w, h) {
                    return Err(ArgusError::new(
                        codes::BAD_SIDECAR,
                        format!("{shown} is {nw}x{nh}; a normal map is its sprite's size, {w}x{h}"),
                    )
                    .at(Some(shown)));
                }
                let path = self.cache.join(format!("{name}.normal.png"));
                crate::encode_png(&path, tw, th, &pad(&n))?;
                Some(path)
            }
        };
        Ok((texture, [tw, th], normal))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_reads_padded_unpadded_and_spaced() {
        assert_eq!(base64("AQAAAA==").unwrap(), vec![1, 0, 0, 0]);
        assert_eq!(base64("AQAA AA").unwrap(), vec![1, 0, 0, 0]);
        assert_eq!(base64("aGVsbG8=").unwrap(), b"hello".to_vec());
        assert!(base64("a*b").is_none());
    }

    #[test]
    fn layer_data_decodes_from_uncompressed_base64_and_xml_tiles() {
        // [1, 2] as little-endian u32s.
        assert_eq!(
            decode("x", Some("base64"), None, " AQAAAAIAAAA= ").unwrap(),
            vec![1, 2]
        );
        assert_eq!(decode("x", None, None, "1,\n0,3").unwrap(), vec![1, 0, 3]);
        assert!(
            decode("x", Some("base64"), None, "AQAA").is_err(),
            "three bytes are no tile id"
        );
        let n = parse_xml(
            "x",
            "<data><tile gid=\"4\"/><tile/><tile gid=\"2147483650\"/></data>",
        )
        .unwrap();
        assert_eq!(xml_gids(&n, "x").unwrap(), vec![4, 0, 0x8000_0002]);
    }

    #[test]
    fn paths_resolve_from_the_file_and_name_assets() {
        assert_eq!(
            resolve("assets/maps/a.tmx", "../tiles/c.tsx").as_deref(),
            Some("assets/tiles/c.tsx")
        );
        assert_eq!(resolve("a.tmx", "../c.tsx"), None);
        assert_eq!(asset_of("assets/tiles/c.tsx").as_deref(), Some("tiles/c"));
        assert_eq!(asset_of("mods/m/assets/m/c.tsj").as_deref(), Some("m/c"));
        assert_eq!(asset_of("maps/c.tsx"), None);
        assert_eq!(
            embedded_name("assets/maps/a.tmx", "Dungeon tiles!", 1),
            "maps/a.tmx/Dungeon_tiles"
        );
        assert_eq!(
            embedded_name("assets/maps/a.tmx", "??", 7),
            "maps/a.tmx/tileset7"
        );
    }
}
