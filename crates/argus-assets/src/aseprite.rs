//! Aseprite files (phase 5 item 4; §14: "Aseprite and Tiled files import
//! natively with their internal structure exposed through the sidecar";
//! §13: "Aseprite tags import directly as clips").
//!
//! `assets/<name>.aseprite` (or `.ase`) is the sprite `<name>`'s source in
//! place of a PNG, read as Aseprite's file specification describes it
//! (`docs/ase-file-specs.md` in Aseprite's repository): RGBA, grayscale, and
//! indexed colour; raw, linked, and compressed cels; layers, groups, tags,
//! slices, palettes old and new, and user data. The importer draws each
//! frame from the layers and lays the frames out as a sheet in the cache,
//! so the rest of the engine sees an ordinary sprite:
//!
//! - Frames: one per Aseprite frame, each the canvas's size, with the
//!   frame's own duration. Frames whose pixels are the same share one place
//!   on the sheet, and the sheet has the column count that makes the
//!   smallest power-of-two texture.
//! - Clips: one per tag, by the tag's name, its frames `from` to `to`, played
//!   by each frame's duration (no fps), in the tag's direction (a ping-pong
//!   reverse is a ping-pong over the frames listed `to` down to `from`).
//!   Repeat 0 (Aseprite plays it forever) loops; repeat 1 plays once, a
//!   ping-pong one way only; repeat n plays the whole sequence n times,
//!   listed out, and stops.
//! - Pivots: the slice named `pivot`, or else the first slice with a pivot,
//!   sets the pivot of every frame it has a key in: its pivot point, or its
//!   middle when it has none. A point is a pixel's top-left corner, so
//!   (8, 16) on a 16 x 16 canvas is the bottom middle, `[0.5, 0.0]`.
//! - Properties: every other slice is the frame property `slice.<name>` =
//!   `"x y w h"`, its bounds in pixels from the frame's top left, y down (a
//!   frame rect's shape), in each frame it has a key in (a hitbox for game
//!   code to read); a 9-patch's centre is `slice.<name>.center`, relative to
//!   the slice as the file has it. From the layers drawn, a cel's user data
//!   text is its frame's `data` property, and its user properties are frame
//!   properties (the top layer's, where two say one thing).
//!
//! The sidecar `<name>.toml` still applies. `layers = ["body", "outline"]`
//! picks the layers drawn, by name or by path through groups
//! (`"fx/sparks"`): a picked layer is drawn even if hidden, and a picked
//! group draws its visible layers. `pivot`, `duration_ms`, and
//! `pixels_per_unit` replace the file's for every frame; `[[frames]] index =
//! n` changes one frame; `[clips.<tag>]` changes only what it writes of the
//! tag's clip (`fps`, `loop`, `frames`; `direction` plays the tag's frames
//! `from` to `to` that way), and a clip of any other name is a clip of its
//! own. `grid`, a frame's `rect`, and a shape's keys are ignored, with a
//! warning, since the file says where the frames are.
//!
//! Drawing is Aseprite's: normal blending with its arithmetic, layer and
//! cel opacity, a group's opacity over its layers drawn together, and a
//! cel's z-index ordering it among its siblings. Other blend modes draw as
//! normal, with a warning naming the layer and the mode; a tilemap layer is
//! skipped with a warning, and a reference layer is not drawn. In an
//! indexed file the transparent index is clear except on the background
//! layer. A colour profile is ignored (the pixels are taken as sRGB), and a
//! chunk of a type the specification does not list is skipped by its size,
//! with a warning.

use crate::sidecar::{self, Direction};
use crate::{
    codes, encode_png, fnv, parse_slope, pow2, repeat_edges, Asset, Clip, Entry, Frame, Group,
    Importer, SpriteAsset, Warning,
};
use argus_ecs::{ArgusError, Result};
use serde_json::{json, Value as Json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read;

/// The most pixels a canvas, or one cel, may have: 4096 x 4096.
const MAX_PIXELS: u64 = 4096 * 4096;
/// The most pixels all the frames together may have.
const MAX_TOTAL: u64 = 64 << 20;
/// The widest or tallest sheet the frames are laid out on.
const MAX_SHEET: u64 = 16384;
/// The most layers, and the deepest groups go.
const MAX_LAYERS: usize = 4096;
const MAX_NESTING: usize = 64;

/// What an Aseprite file holds, as `asset.get` shows it, so an agent can
/// write a sidecar that picks layers or names a clip without opening
/// Aseprite.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    /// The canvas, in pixels.
    pub canvas: [u32; 2],
    /// `rgba`, `grayscale`, or `indexed`.
    pub depth: &'static str,
    /// Frames in the file.
    pub frames: u32,
    /// Layers, bottom to top as the file lists them (a group before its own).
    pub layers: Vec<LayerInfo>,
    /// Tags.
    pub tags: Vec<TagInfo>,
    /// Slices.
    pub slices: Vec<SliceInfo>,
}

/// One layer of an Aseprite file.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerInfo {
    /// Its name.
    pub name: String,
    /// Its groups' names and its own, joined by `/`.
    pub path: String,
    /// `image`, `group`, `tilemap`, or `reference`.
    pub kind: &'static str,
    /// Visible in the file.
    pub visible: bool,
    /// 0 to 255.
    pub opacity: u8,
    /// `normal`, `multiply`, and so on.
    pub blend: &'static str,
    /// Drawn into the sprite (a group: one of its layers is).
    pub used: bool,
}

/// One tag of an Aseprite file.
#[derive(Clone, Debug, PartialEq)]
pub struct TagInfo {
    /// Its name, the clip's.
    pub name: String,
    /// First frame.
    pub from: u32,
    /// Last frame.
    pub to: u32,
    /// `forward`, `reverse`, `pingpong`, or `pingpong_reverse`.
    pub direction: &'static str,
    /// Times played; 0 is unspecified (Aseprite loops it).
    pub repeat: u32,
}

/// One slice of an Aseprite file.
#[derive(Clone, Debug, PartialEq)]
pub struct SliceInfo {
    /// Its name.
    pub name: String,
    /// Its keys: each holds from its frame until the next key.
    pub keys: Vec<SliceKey>,
}

/// A slice from one frame on.
#[derive(Clone, Debug, PartialEq)]
pub struct SliceKey {
    /// The first frame it holds for.
    pub frame: u32,
    /// x, y, w, h in pixels of the canvas, y down.
    pub bounds: [i64; 4],
    /// A 9-patch's centre: x, y, w, h relative to the bounds.
    pub center: Option<[i64; 4]>,
    /// The pivot, relative to the bounds' origin.
    pub pivot: Option<[i64; 2]>,
}

impl Info {
    /// As `asset.get` shows it.
    pub fn to_json(&self) -> Json {
        let layers: Vec<Json> = self
            .layers
            .iter()
            .map(|l| {
                json!({ "name": l.name, "path": l.path, "kind": l.kind, "visible": l.visible,
                        "opacity": l.opacity, "blend": l.blend, "used": l.used })
            })
            .collect();
        let tags: Vec<Json> = self
            .tags
            .iter()
            .map(|t| json!({ "name": t.name, "from": t.from, "to": t.to, "direction": t.direction, "repeat": t.repeat }))
            .collect();
        let slices: Vec<Json> = self
            .slices
            .iter()
            .map(|s| {
                let keys: Vec<Json> = s
                    .keys
                    .iter()
                    .map(|k| {
                        let mut j = json!({ "frame": k.frame, "bounds": k.bounds });
                        if let Some(c) = k.center {
                            j["center"] = json!(c);
                        }
                        if let Some(p) = k.pivot {
                            j["pivot"] = json!(p);
                        }
                        j
                    })
                    .collect();
                json!({ "name": s.name, "keys": keys })
            })
            .collect();
        json!({
            "canvas": self.canvas,
            "depth": self.depth,
            "frames": self.frames,
            "layers": layers,
            "tags": tags,
            "slices": slices,
        })
    }
}

const BLENDS: [&str; 19] = [
    "normal",
    "multiply",
    "screen",
    "overlay",
    "darken",
    "lighten",
    "color_dodge",
    "color_burn",
    "hard_light",
    "soft_light",
    "difference",
    "exclusion",
    "hue",
    "saturation",
    "color",
    "luminosity",
    "addition",
    "subtract",
    "divide",
];

fn blend_name(b: u16) -> &'static str {
    BLENDS.get(b as usize).copied().unwrap_or("unknown")
}

struct Layer {
    name: String,
    flags: u16,
    /// 0 image, 1 group, 2 tilemap.
    kind: u16,
    blend: u16,
    opacity: u8,
    parent: Option<usize>,
}

impl Layer {
    fn visible(&self) -> bool {
        self.flags & 1 != 0
    }
    fn background(&self) -> bool {
        self.flags & 8 != 0
    }
    fn reference(&self) -> bool {
        self.flags & 64 != 0
    }
}

enum Content {
    /// Pixels in the file's depth.
    Image {
        w: u32,
        h: u32,
        px: Vec<u8>,
    },
    /// The same layer's cel in that frame.
    Link(u32),
    Tilemap,
}

struct Cel {
    layer: usize,
    x: i64,
    y: i64,
    opacity: u8,
    z: i64,
    content: Content,
    /// User data text.
    data: Option<String>,
    /// User properties.
    props: Vec<(String, String)>,
}

struct Tag {
    name: String,
    from: u32,
    to: u32,
    dir: u8,
    repeat: u32,
}

struct FrameData {
    ms: u32,
    cels: Vec<Cel>,
    /// Index into the file's palettes.
    palette: usize,
}

/// A file as read.
struct AseFile {
    w: u32,
    h: u32,
    depth: u16,
    flags: u32,
    transparent: u8,
    frames: Vec<FrameData>,
    layers: Vec<Layer>,
    /// Each group's layers in order, and last the top level's.
    kids: Vec<Vec<usize>>,
    tags: Vec<Tag>,
    slices: Vec<SliceInfo>,
    /// The palette as each frame has it.
    palettes: Vec<Vec<[u8; 4]>>,
    /// What reading noticed.
    notes: Vec<String>,
}

type Res<T> = std::result::Result<T, String>;

/// Bytes read in order, never past `end`.
struct Rd<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> Res<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or_else(|| format!("it is cut short at byte {}", self.b.len()))?;
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn arr<const N: usize>(&mut self) -> Res<[u8; N]> {
        Ok(self.take(N)?.try_into().expect("N bytes"))
    }
    fn u8(&mut self) -> Res<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Res<u16> {
        Ok(u16::from_le_bytes(self.arr()?))
    }
    fn i16(&mut self) -> Res<i16> {
        Ok(i16::from_le_bytes(self.arr()?))
    }
    fn u32(&mut self) -> Res<u32> {
        Ok(u32::from_le_bytes(self.arr()?))
    }
    fn i32(&mut self) -> Res<i32> {
        Ok(i32::from_le_bytes(self.arr()?))
    }
    fn u64(&mut self) -> Res<u64> {
        Ok(u64::from_le_bytes(self.arr()?))
    }
    fn skip(&mut self, n: usize) -> Res<()> {
        self.take(n).map(|_| ())
    }
    fn string(&mut self) -> Res<String> {
        let n = self.u16()? as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn rest(&mut self) -> &'a [u8] {
        let s = &self.b[self.pos.min(self.b.len())..];
        self.pos = self.b.len();
        s
    }
}

/// What a user data chunk belongs to: the chunk before it.
#[derive(Clone, Copy)]
enum Owner {
    None,
    Cel(usize),
    Tags(usize),
}

fn inflate(data: &[u8], n: usize, w: u32, h: u32) -> Res<Vec<u8>> {
    let mut out = Vec::with_capacity(n.min(1 << 24));
    flate2::read::ZlibDecoder::new(data)
        .take(n as u64)
        .read_to_end(&mut out)
        .map_err(|e| format!("its compressed image does not inflate ({e})"))?;
    if out.len() < n {
        return Err(format!(
            "its compressed image holds {} bytes, and {w} x {h} pixels need {n}",
            out.len()
        ));
    }
    Ok(out)
}

/// A user property's value as text: numbers as written, a point `"x y"`,
/// a size `"w h"`, a rect `"x y w h"`, a vector `[a, b]`, a map `{k = v}`.
fn value(r: &mut Rd, ty: u16, depth: u32) -> Res<String> {
    if depth > 16 {
        return Err("user properties nest more than 16 deep".to_string());
    }
    Ok(match ty {
        0x01 => (r.u8()? != 0).to_string(),
        0x02 => (r.u8()? as i8).to_string(),
        0x03 => r.u8()?.to_string(),
        0x04 => r.i16()?.to_string(),
        0x05 => r.u16()?.to_string(),
        0x06 => r.i32()?.to_string(),
        0x07 => r.u32()?.to_string(),
        0x08 => (r.u64()? as i64).to_string(),
        0x09 => r.u64()?.to_string(),
        0x0A => (r.i32()? as f64 / 65536.0).to_string(),
        0x0B => f32::from_le_bytes(r.arr()?).to_string(),
        0x0C => f64::from_le_bytes(r.arr()?).to_string(),
        0x0D => r.string()?,
        0x0E | 0x0F => format!("{} {}", r.i32()?, r.i32()?),
        0x10 => format!("{} {} {} {}", r.i32()?, r.i32()?, r.i32()?, r.i32()?),
        0x11 => {
            let n = r.u32()?;
            let et = r.u16()?;
            let mut items = Vec::new();
            for _ in 0..n {
                let t = if et == 0 { r.u16()? } else { et };
                items.push(value(r, t, depth + 1)?);
            }
            format!("[{}]", items.join(", "))
        }
        0x12 => {
            let n = r.u32()?;
            let mut items = Vec::new();
            for _ in 0..n {
                let k = r.string()?;
                let t = r.u16()?;
                items.push(format!("{k} = {}", value(r, t, depth + 1)?));
            }
            format!("{{{}}}", items.join(", "))
        }
        0x13 => r.take(16)?.iter().map(|b| format!("{b:02x}")).collect(),
        other => return Err(format!("a user property of unknown type {other}")),
    })
}

/// The user properties of a user data chunk (maps keyed 0 are the user's;
/// the others belong to extensions and are skipped).
fn properties(r: &mut Rd) -> Res<Vec<(String, String)>> {
    let start = r.pos;
    let size = r.u32()? as usize;
    let end = start
        .checked_add(size)
        .filter(|&e| e <= r.b.len() && size >= 8)
        .ok_or_else(|| format!("user properties of {size} bytes do not fit their chunk"))?;
    let maps = r.u32()?;
    let mut out = Vec::new();
    for _ in 0..maps {
        let key = r.u32()?;
        let n = r.u32()?;
        for _ in 0..n {
            let name = r.string()?;
            let ty = r.u16()?;
            let v = value(r, ty, 0)?;
            if key == 0 {
                out.push((name, v));
            }
        }
    }
    r.pos = end;
    Ok(out)
}

fn parse(bytes: &[u8]) -> Res<AseFile> {
    if bytes.len() < 128 {
        return Err(format!(
            "it is {} bytes, shorter than an Aseprite header (128)",
            bytes.len()
        ));
    }
    let mut r = Rd { b: bytes, pos: 0 };
    let _size = r.u32()?;
    let magic = r.u16()?;
    if magic != 0xA5E0 {
        return Err(format!(
            "its header's magic number is 0x{magic:04X}, not Aseprite's 0xA5E0"
        ));
    }
    let nframes = r.u16()? as u32;
    let w = r.u16()? as u32;
    let h = r.u16()? as u32;
    let depth = r.u16()?;
    let flags = r.u32()?;
    let speed = r.u16()? as u32;
    r.skip(8)?;
    let transparent = r.u8()?;
    if ![8, 16, 32].contains(&depth) {
        return Err(format!(
            "its colour depth is {depth}; Aseprite writes 32 (RGBA), 16 (grayscale), or 8 (indexed)"
        ));
    }
    if w == 0 || h == 0 || nframes == 0 {
        return Err(format!(
            "its canvas is {w} x {h} with {nframes} frames; a sprite needs at least a pixel and a frame"
        ));
    }
    if (w as u64) * (h as u64) > MAX_PIXELS {
        return Err(format!(
            "its canvas is {w} x {h}, more than the {MAX_PIXELS} pixels a sprite may have"
        ));
    }
    if (w as u64) * (h as u64) * nframes as u64 > MAX_TOTAL {
        return Err(format!(
            "its {nframes} frames of {w} x {h} are more than the {MAX_TOTAL} pixels a sprite's frames may have together"
        ));
    }
    let bpp = (depth / 8) as usize;
    let mut f = AseFile {
        w,
        h,
        depth,
        flags,
        transparent,
        frames: Vec::new(),
        layers: Vec::new(),
        kids: Vec::new(),
        tags: Vec::new(),
        slices: Vec::new(),
        palettes: Vec::new(),
        notes: Vec::new(),
    };
    let mut palette: Vec<[u8; 4]> = Vec::new();
    let mut palette_changed = true;
    let mut new_palette = false;
    // The last layer at each child level, for a layer's group.
    let mut stack: Vec<usize> = Vec::new();
    let mut unknown: BTreeSet<u16> = BTreeSet::new();
    let mut pos = 128usize;
    for fi in 0..nframes {
        let mut fr = Rd { b: bytes, pos };
        let at = pos;
        let size = fr
            .u32()
            .map_err(|_| format!("frame {fi} is missing: the file ends at byte {at}"))?
            as usize;
        let magic = fr
            .u16()
            .map_err(|_| format!("frame {fi} is cut short at byte {}", bytes.len()))?;
        if magic != 0xF1FA {
            return Err(format!(
                "frame {fi} at byte {at} is not a frame: its magic number is 0x{magic:04X}, not 0xF1FA"
            ));
        }
        let end = at
            .checked_add(size)
            .filter(|&e| e <= bytes.len() && size >= 16)
            .ok_or_else(|| {
                format!(
                    "frame {fi} at byte {at} says it is {size} bytes, and the file has {} left",
                    bytes.len() - at
                )
            })?;
        let old = fr.u16()? as u32;
        let ms = fr.u16()? as u32;
        fr.skip(2)?;
        let new = fr.u32()?;
        let chunks = if new == 0 { old } else { new };
        let mut cels: Vec<Cel> = Vec::new();
        let mut owner = Owner::None;
        let mut old_palette: Vec<(usize, [u8; 4])> = Vec::new();
        let mut cp = fr.pos;
        for ci in 0..chunks {
            if end - cp < 6 {
                break;
            }
            let csize = u32::from_le_bytes(bytes[cp..cp + 4].try_into().expect("4")) as usize;
            let ctype = u16::from_le_bytes(bytes[cp + 4..cp + 6].try_into().expect("2"));
            let cend = cp
                .checked_add(csize)
                .filter(|&e| e <= end && csize >= 6)
                .ok_or_else(|| {
                    format!(
                        "frame {fi}'s chunk {ci} (type 0x{ctype:04X}) at byte {cp} says it is {csize} bytes, past its frame's end at byte {end}"
                    )
                })?;
            let mut c = Rd {
                b: &bytes[..cend],
                pos: cp + 6,
            };
            let what =
                move |e: String| format!("frame {fi}'s chunk 0x{ctype:04X} at byte {cp}: {e}");
            match ctype {
                0x2004 => {
                    let lflags = c.u16().map_err(what)?;
                    let kind = c.u16().map_err(what)?;
                    let level = c.u16().map_err(what)? as usize;
                    c.skip(4).map_err(what)?;
                    let blend = c.u16().map_err(what)?;
                    let opacity = c.u8().map_err(what)?;
                    c.skip(3).map_err(what)?;
                    let name = c.string().map_err(what)?;
                    let i = f.layers.len();
                    if i >= MAX_LAYERS || level > MAX_NESTING {
                        return Err(what(format!(
                            "layer {name:?} is layer {i} at group depth {level}; a sprite may have {MAX_LAYERS} layers, {MAX_NESTING} groups deep"
                        )));
                    }
                    let parent = if level == 0 {
                        None
                    } else {
                        stack
                            .get(level - 1)
                            .or(stack.last())
                            .copied()
                            .filter(|&p| f.layers[p].kind == 1)
                    };
                    stack.truncate(level);
                    stack.push(i);
                    f.layers.push(Layer {
                        name,
                        flags: lflags,
                        kind,
                        blend,
                        opacity,
                        parent,
                    });
                    owner = Owner::None;
                }
                0x2005 => {
                    let layer = c.u16().map_err(what)? as usize;
                    let x = c.i16().map_err(what)? as i64;
                    let y = c.i16().map_err(what)? as i64;
                    let opacity = c.u8().map_err(what)?;
                    let ty = c.u16().map_err(what)?;
                    let z = c.i16().map_err(what)? as i64;
                    c.skip(5).map_err(what)?;
                    if layer >= f.layers.len() {
                        return Err(what(format!(
                            "a cel on layer {layer}, and the file has {} layers",
                            f.layers.len()
                        )));
                    }
                    let image = |c: &mut Rd| -> Res<(u32, u32, usize)> {
                        let w = c.u16()? as u32;
                        let h = c.u16()? as u32;
                        if (w as u64) * (h as u64) > MAX_PIXELS {
                            return Err(format!(
                                "a cel of {w} x {h}, more than the {MAX_PIXELS} pixels one may have"
                            ));
                        }
                        Ok((w, h, w as usize * h as usize * bpp))
                    };
                    let content = match ty {
                        0 => {
                            let (w, h, n) = image(&mut c).map_err(what)?;
                            let px = c.take(n).map_err(what)?.to_vec();
                            Content::Image { w, h, px }
                        }
                        1 => Content::Link(c.u16().map_err(what)? as u32),
                        2 => {
                            let (w, h, n) = image(&mut c).map_err(what)?;
                            let px = inflate(c.rest(), n, w, h).map_err(what)?;
                            Content::Image { w, h, px }
                        }
                        3 => Content::Tilemap,
                        other => {
                            f.notes.push(format!(
                                "frame {fi} has a cel of unknown type {other} on layer {:?}; skipped",
                                f.layers[layer].name
                            ));
                            owner = Owner::None;
                            cp = cend;
                            continue;
                        }
                    };
                    cels.push(Cel {
                        layer,
                        x,
                        y,
                        opacity,
                        z,
                        content,
                        data: None,
                        props: Vec::new(),
                    });
                    owner = Owner::Cel(cels.len() - 1);
                }
                0x2018 => {
                    let n = c.u16().map_err(what)?;
                    c.skip(8).map_err(what)?;
                    let first = f.tags.len();
                    for _ in 0..n {
                        let from = c.u16().map_err(what)? as u32;
                        let to = c.u16().map_err(what)? as u32;
                        let dir = c.u8().map_err(what)?;
                        let repeat = c.u16().map_err(what)? as u32;
                        c.skip(10).map_err(what)?;
                        let name = c.string().map_err(what)?;
                        f.tags.push(Tag {
                            name,
                            from,
                            to,
                            dir,
                            repeat,
                        });
                    }
                    owner = Owner::Tags(first);
                }
                0x2019 => {
                    let size = c.u32().map_err(what)? as usize;
                    let first = c.u32().map_err(what)? as usize;
                    let last = c.u32().map_err(what)? as usize;
                    c.skip(8).map_err(what)?;
                    if size > 65536 || first > last || last >= size {
                        return Err(what(format!(
                            "a palette of {size} colours changing {first} to {last}"
                        )));
                    }
                    palette.resize(size, [0, 0, 0, 255]);
                    for e in palette.iter_mut().take(last + 1).skip(first) {
                        let eflags = c.u16().map_err(what)?;
                        *e = c.arr::<4>().map_err(what)?;
                        if eflags & 1 != 0 {
                            c.string().map_err(what)?;
                        }
                    }
                    new_palette = true;
                    palette_changed = true;
                    owner = Owner::None;
                }
                0x0004 | 0x0011 => {
                    let packets = c.u16().map_err(what)?;
                    let mut i = 0usize;
                    for _ in 0..packets {
                        i += c.u8().map_err(what)? as usize;
                        let n = match c.u8().map_err(what)? {
                            0 => 256,
                            n => n as usize,
                        };
                        for _ in 0..n {
                            let [r, g, b] = c.arr::<3>().map_err(what)?;
                            let rgb = if ctype == 0x0011 {
                                // 0 to 63, as Aseprite widens it.
                                [r, g, b].map(|v| (v.min(63) << 2) | (v.min(63) >> 4))
                            } else {
                                [r, g, b]
                            };
                            old_palette.push((i, [rgb[0], rgb[1], rgb[2], 255]));
                            i += 1;
                        }
                    }
                    owner = Owner::None;
                }
                0x2020 => {
                    let uflags = c.u32().map_err(what)?;
                    let text = if uflags & 1 != 0 {
                        Some(c.string().map_err(what)?)
                    } else {
                        None
                    };
                    if uflags & 2 != 0 {
                        c.skip(4).map_err(what)?;
                    }
                    let props = if uflags & 4 != 0 {
                        match properties(&mut c) {
                            Ok(p) => p,
                            Err(e) => {
                                f.notes.push(format!(
                                    "frame {fi}: user properties at byte {cp} could not be read ({e}); skipped"
                                ));
                                Vec::new()
                            }
                        }
                    } else {
                        Vec::new()
                    };
                    match owner {
                        Owner::Cel(i) => {
                            cels[i].data = text;
                            cels[i].props = props;
                        }
                        // One user data chunk a tag follows the tags.
                        Owner::Tags(i) => owner = Owner::Tags(i + 1),
                        Owner::None => {}
                    }
                }
                0x2022 => {
                    let nkeys = c.u32().map_err(what)?;
                    let sflags = c.u32().map_err(what)?;
                    c.skip(4).map_err(what)?;
                    let name = c.string().map_err(what)?;
                    let mut keys = Vec::new();
                    for _ in 0..nkeys {
                        let frame = c.u32().map_err(what)?;
                        let bounds = [
                            c.i32().map_err(what)? as i64,
                            c.i32().map_err(what)? as i64,
                            c.u32().map_err(what)? as i64,
                            c.u32().map_err(what)? as i64,
                        ];
                        let center = if sflags & 1 != 0 {
                            Some([
                                c.i32().map_err(what)? as i64,
                                c.i32().map_err(what)? as i64,
                                c.u32().map_err(what)? as i64,
                                c.u32().map_err(what)? as i64,
                            ])
                        } else {
                            None
                        };
                        let pivot = if sflags & 2 != 0 {
                            Some([c.i32().map_err(what)? as i64, c.i32().map_err(what)? as i64])
                        } else {
                            None
                        };
                        keys.push(SliceKey {
                            frame,
                            bounds,
                            center,
                            pivot,
                        });
                    }
                    keys.sort_by_key(|k| k.frame);
                    f.slices.push(SliceInfo { name, keys });
                    owner = Owner::None;
                }
                // Cel extra (a reference layer's precise bounds) belongs
                // to the cel before it, and a user data chunk may follow.
                0x2006 => {}
                // Colour profile, external files, mask, path, tileset:
                // nothing a sprite draws.
                0x2007 | 0x2008 | 0x2016 | 0x2017 | 0x2023 => owner = Owner::None,
                other => {
                    unknown.insert(other);
                    owner = Owner::None;
                }
            }
            cp = cend;
        }
        // The old palette chunks count only in a file with no new one.
        if !new_palette && !old_palette.is_empty() {
            for (i, c) in old_palette {
                if i >= palette.len() {
                    palette.resize(i + 1, [0, 0, 0, 255]);
                }
                palette[i] = c;
            }
            palette_changed = true;
        }
        if palette_changed {
            f.palettes.push(palette.clone());
            palette_changed = false;
        }
        f.frames.push(FrameData {
            ms: match (ms, speed) {
                (0, 0) => 100,
                (0, s) => s,
                (m, _) => m,
            },
            cels,
            palette: f.palettes.len() - 1,
        });
        pos = end;
    }
    for t in unknown {
        f.notes.push(format!(
            "the file has chunks of type 0x{t:04X}, which the specification does not list; skipped"
        ));
    }
    let n = f.layers.len();
    f.kids = vec![Vec::new(); n + 1];
    for (i, l) in f.layers.iter().enumerate() {
        f.kids[l.parent.unwrap_or(n)].push(i);
    }
    Ok(f)
}

/// Aseprite's `MUL_UN8`: a times b over 255, rounded.
fn mul(a: u32, b: u32) -> u32 {
    let t = a * b + 0x80;
    ((t >> 8) + t) >> 8
}

/// Aseprite's normal blend of `src` at `opacity` over `dst`.
fn blend(dst: &mut [u8], src: [u8; 4], opacity: u32) {
    if dst[3] == 0 {
        dst[..3].copy_from_slice(&src[..3]);
        dst[3] = mul(src[3] as u32, opacity) as u8;
        return;
    }
    if src[3] == 0 {
        return;
    }
    let sa = mul(src[3] as u32, opacity) as i32;
    let ba = dst[3] as i32;
    let ra = sa + ba - mul(ba as u32, sa as u32) as i32;
    for c in 0..3 {
        let (b, s) = (dst[c] as i32, src[c] as i32);
        dst[c] = (b + (s - b) * sa / ra) as u8;
    }
    dst[3] = ra as u8;
}

impl AseFile {
    fn bpp(&self) -> usize {
        (self.depth / 8) as usize
    }

    fn depth_name(&self) -> &'static str {
        match self.depth {
            32 => "rgba",
            16 => "grayscale",
            _ => "indexed",
        }
    }

    fn path(&self, i: usize) -> String {
        let mut names = vec![self.layers[i].name.as_str()];
        let mut p = self.layers[i].parent;
        while let Some(g) = p {
            names.push(&self.layers[g].name);
            p = self.layers[g].parent;
        }
        names.reverse();
        names.join("/")
    }

    /// `i`'s groups, nearest first.
    fn ancestors(&self, i: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut p = self.layers[i].parent;
        while let Some(g) = p {
            if out.contains(&g) {
                break;
            }
            out.push(g);
            p = self.layers[g].parent;
        }
        out
    }

    /// Which layers are drawn: the visible image layers, or those the
    /// sidecar picks. Groups count when a layer in them does. Also the
    /// picked names the file lacks, and what was skipped.
    fn pick(&self, names: Option<&[String]>) -> (Vec<bool>, Vec<String>, Vec<String>) {
        let n = self.layers.len();
        let mut used = vec![false; n];
        let mut missing = Vec::new();
        let mut notes = Vec::new();
        let want = |i: usize, used: &mut Vec<bool>, notes: &mut Vec<String>| {
            let l = &self.layers[i];
            match l.kind {
                0 => used[i] = true,
                2 => notes.push(format!(
                    "layer {:?} is a tilemap layer; tilemaps are not imported, so it is skipped",
                    self.path(i)
                )),
                _ => {}
            }
        };
        match names {
            None => {
                for i in 0..n {
                    let l = &self.layers[i];
                    let shown = l.visible()
                        && !l.reference()
                        && self.ancestors(i).iter().all(|&g| self.layers[g].visible());
                    if shown {
                        want(i, &mut used, &mut notes);
                    }
                }
            }
            Some(list) => {
                for name in list {
                    let hits: Vec<usize> = (0..n)
                        .filter(|&i| self.layers[i].name == *name || self.path(i) == *name)
                        .collect();
                    if hits.is_empty() {
                        missing.push(name.clone());
                    }
                    for m in hits {
                        if self.layers[m].kind != 1 {
                            want(m, &mut used, &mut notes);
                            continue;
                        }
                        // A group: its visible layers, through visible groups.
                        for i in 0..n {
                            let up = self.ancestors(i);
                            if let Some(k) = up.iter().position(|&g| g == m) {
                                let l = &self.layers[i];
                                if l.kind != 1
                                    && l.visible()
                                    && !l.reference()
                                    && up[..k].iter().all(|&g| self.layers[g].visible())
                                {
                                    want(i, &mut used, &mut notes);
                                }
                            }
                        }
                    }
                }
            }
        }
        for i in 0..n {
            if used[i] {
                for g in self.ancestors(i) {
                    used[g] = true;
                }
            }
        }
        notes.sort();
        notes.dedup();
        (used, missing, notes)
    }

    /// Layer `layer`'s cel in frame `f`, links followed to the cel that
    /// holds the image (its place, opacity, and user data are a link's too).
    fn cel(&self, f: usize, layer: usize, notes: &mut BTreeSet<String>) -> Option<&Cel> {
        let mut c = self.frames[f].cels.iter().find(|c| c.layer == layer)?;
        for _ in 0..8 {
            match c.content {
                Content::Link(to) => {
                    let Some(t) = self
                        .frames
                        .get(to as usize)
                        .and_then(|fr| fr.cels.iter().find(|c| c.layer == layer))
                    else {
                        notes.insert(format!(
                            "frame {f}: layer {:?}'s cel links to frame {to}, which has no cel on it; skipped",
                            self.path(layer)
                        ));
                        return None;
                    };
                    c = t;
                }
                _ => return Some(c),
            }
        }
        notes.insert(format!(
            "frame {f}: layer {:?}'s cel links in a circle; skipped",
            self.path(layer)
        ));
        None
    }

    /// Every frame drawn from the layers `used` marks, RGBA.
    fn composite(&self, used: &[bool]) -> (Vec<Vec<u8>>, Vec<String>) {
        let mut notes = BTreeSet::new();
        for (i, l) in self.layers.iter().enumerate() {
            let counts = match l.kind {
                0 => true,
                1 => self.flags & 2 != 0,
                _ => false,
            };
            if used[i] && counts && l.blend != 0 {
                notes.insert(format!(
                    "layer {:?} blends as {}, drawn as normal",
                    self.path(i),
                    blend_name(l.blend)
                ));
            }
        }
        let size = (self.w * self.h * 4) as usize;
        let frames = (0..self.frames.len())
            .map(|f| {
                let mut buf = vec![0u8; size];
                self.draw_group(None, f, used, &mut buf, &mut notes);
                buf
            })
            .collect();
        (frames, notes.into_iter().collect())
    }

    fn draw_group(
        &self,
        group: Option<usize>,
        f: usize,
        used: &[bool],
        buf: &mut [u8],
        notes: &mut BTreeSet<String>,
    ) {
        // Siblings by index plus z-index, then z-index (the spec's NOTE.5).
        let mut order: Vec<(i64, i64, usize)> = self.kids[group.unwrap_or(self.layers.len())]
            .iter()
            .copied()
            .enumerate()
            .filter(|&(_, i)| used[i])
            .map(|(k, i)| {
                let z = self.frames[f]
                    .cels
                    .iter()
                    .find(|c| c.layer == i)
                    .map(|c| c.z)
                    .unwrap_or(0);
                (k as i64 + z, z, i)
            })
            .collect();
        order.sort();
        for (_, _, i) in order {
            let l = &self.layers[i];
            let opacity = if self.flags & 1 != 0 {
                l.opacity as u32
            } else {
                255
            };
            match l.kind {
                1 => {
                    let opacity = if self.flags & 2 != 0 { opacity } else { 255 };
                    if opacity == 255 {
                        self.draw_group(Some(i), f, used, buf, notes);
                    } else {
                        // Drawn together, then over what is below.
                        let mut tmp = vec![0u8; buf.len()];
                        self.draw_group(Some(i), f, used, &mut tmp, notes);
                        let pairs = buf.as_chunks_mut::<4>().0.iter_mut();
                        for (d, s) in pairs.zip(tmp.as_chunks::<4>().0) {
                            blend(d, *s, opacity);
                        }
                    }
                }
                0 => {
                    if let Some(c) = self.cel(f, i, notes) {
                        if let Content::Image { w, h, px } = &c.content {
                            let op = mul(c.opacity as u32, opacity);
                            self.draw_cel(f, i, c, *w, *h, px, op, buf, notes);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_cel(
        &self,
        f: usize,
        layer: usize,
        c: &Cel,
        w: u32,
        h: u32,
        px: &[u8],
        opacity: u32,
        buf: &mut [u8],
        notes: &mut BTreeSet<String>,
    ) {
        let bpp = self.bpp();
        let palette = &self.palettes[self.frames[f].palette];
        let background = self.layers[layer].background();
        let (cw, ch) = (self.w as i64, self.h as i64);
        for yy in 0..h as i64 {
            let y = c.y + yy;
            if !(0..ch).contains(&y) {
                continue;
            }
            for xx in 0..w as i64 {
                let x = c.x + xx;
                if !(0..cw).contains(&x) {
                    continue;
                }
                let s = ((yy * w as i64 + xx) as usize) * bpp;
                let src = match self.depth {
                    32 => [px[s], px[s + 1], px[s + 2], px[s + 3]],
                    16 => [px[s], px[s], px[s], px[s + 1]],
                    _ => {
                        let i = px[s];
                        if i == self.transparent && !background {
                            [0, 0, 0, 0]
                        } else if let Some(c) = palette.get(i as usize) {
                            *c
                        } else {
                            notes.insert(format!(
                                "frame {f}: index {i} is outside the {}-colour palette; drawn clear",
                                palette.len()
                            ));
                            [0, 0, 0, 0]
                        }
                    }
                };
                let d = ((y * cw + x) * 4) as usize;
                blend(&mut buf[d..d + 4], src, opacity);
            }
        }
    }

    /// Each frame's properties: slices other than `pivot`, and the user
    /// data of the cels drawn.
    fn properties(&self, used: &[bool]) -> Vec<BTreeMap<String, String>> {
        let mut quiet = BTreeSet::new();
        (0..self.frames.len())
            .map(|f| {
                let mut p = BTreeMap::new();
                for (i, l) in self.layers.iter().enumerate() {
                    if !used[i] || l.kind != 0 {
                        continue;
                    }
                    if let Some(c) = self.cel(f, i, &mut quiet) {
                        if let Some(t) = &c.data {
                            p.insert("data".to_string(), t.clone());
                        }
                        for (k, v) in &c.props {
                            p.insert(k.clone(), v.clone());
                        }
                    }
                }
                for s in &self.slices {
                    if s.name == "pivot" {
                        continue;
                    }
                    if let Some(k) = key_at(s, f as u32) {
                        let [x, y, w, h] = k.bounds;
                        p.insert(format!("slice.{}", s.name), format!("{x} {y} {w} {h}"));
                        if let Some([x, y, w, h]) = k.center {
                            p.insert(
                                format!("slice.{}.center", s.name),
                                format!("{x} {y} {w} {h}"),
                            );
                        }
                    }
                }
                p
            })
            .collect()
    }

    /// The slice that sets pivots: the one named `pivot`, or the first with
    /// a pivot point.
    fn pivot_slice(&self) -> Option<usize> {
        self.slices
            .iter()
            .position(|s| s.name == "pivot")
            .or_else(|| {
                self.slices
                    .iter()
                    .position(|s| s.keys.iter().any(|k| k.pivot.is_some()))
            })
    }

    /// Each frame's pivot from that slice, as a fraction of the frame, y up.
    fn pivots(&self, slice: Option<usize>) -> Vec<Option<[f32; 2]>> {
        (0..self.frames.len() as u32)
            .map(|f| {
                let k = key_at(&self.slices[slice?], f)?;
                let [x, y, w, h] = k.bounds;
                let (px, py) = match k.pivot {
                    Some([px, py]) => ((x + px) as f32, (y + py) as f32),
                    None => (x as f32 + w as f32 / 2.0, y as f32 + h as f32 / 2.0),
                };
                Some([px / self.w as f32, 1.0 - py / self.h as f32])
            })
            .collect()
    }

    /// The tags as clips, and each tag's frames from..to.
    fn clips(
        &self,
        notes: &mut Vec<String>,
    ) -> (BTreeMap<String, Clip>, BTreeMap<String, (u32, u32)>) {
        let n = self.frames.len() as u32;
        let mut clips = BTreeMap::new();
        let mut ranges = BTreeMap::new();
        for t in &self.tags {
            if t.from > t.to || t.to >= n {
                notes.push(format!(
                    "tag {:?} runs from frame {} to {}, and the file has {n} frames; dropped",
                    t.name, t.from, t.to
                ));
                continue;
            }
            if clips.contains_key(&t.name) {
                notes.push(format!(
                    "two tags are named {:?}; the second (frames {} to {}) is dropped",
                    t.name, t.from, t.to
                ));
                continue;
            }
            let range: Vec<u32> = (t.from..=t.to).collect();
            let back: Vec<u32> = range.iter().rev().copied().collect();
            let dir = if t.dir > 3 {
                notes.push(format!(
                    "tag {:?} has direction {}, which Aseprite does not write; played forward",
                    t.name, t.dir
                ));
                0
            } else {
                t.dir
            };
            let (frames, direction, looping) = match (dir, t.repeat) {
                (0, 0) => (range.clone(), Direction::Forward, true),
                (1, 0) => (range.clone(), Direction::Reverse, true),
                (2, 0) => (range.clone(), Direction::PingPong, true),
                (3, 0) => (back, Direction::PingPong, true),
                // Once: a ping-pong goes one way.
                (0 | 2, 1) => (range.clone(), Direction::Forward, false),
                (_, 1) => (range.clone(), Direction::Reverse, false),
                // n times, listed out.
                (d, k) => {
                    let mut seq = Vec::new();
                    for pass in 0..k {
                        let forward = match d {
                            0 => true,
                            1 => false,
                            2 => pass % 2 == 0,
                            _ => pass % 2 == 1,
                        };
                        let one = if forward { &range } else { &back };
                        // A ping-pong does not show its turning frame twice.
                        let skip = usize::from(d >= 2 && pass > 0);
                        seq.extend(one.iter().skip(skip));
                    }
                    (seq, Direction::Forward, false)
                }
            };
            ranges.insert(t.name.clone(), (t.from, t.to));
            clips.insert(
                t.name.clone(),
                Clip {
                    frames,
                    fps: None,
                    looping,
                    direction,
                },
            );
        }
        (clips, ranges)
    }

    fn info(&self, used: &[bool]) -> Info {
        Info {
            canvas: [self.w, self.h],
            depth: self.depth_name(),
            frames: self.frames.len() as u32,
            layers: self
                .layers
                .iter()
                .enumerate()
                .map(|(i, l)| LayerInfo {
                    name: l.name.clone(),
                    path: self.path(i),
                    kind: match l.kind {
                        0 if l.reference() => "reference",
                        0 => "image",
                        1 => "group",
                        2 => "tilemap",
                        _ => "unknown",
                    },
                    visible: l.visible(),
                    // As drawn: a group's counts only with header flag 2.
                    opacity: if self.flags & 1 != 0 && (l.kind != 1 || self.flags & 2 != 0) {
                        l.opacity
                    } else {
                        255
                    },
                    blend: blend_name(l.blend),
                    used: used[i],
                })
                .collect(),
            tags: self
                .tags
                .iter()
                .map(|t| TagInfo {
                    name: t.name.clone(),
                    from: t.from,
                    to: t.to,
                    direction: match t.dir {
                        0 => "forward",
                        1 => "reverse",
                        2 => "pingpong",
                        3 => "pingpong_reverse",
                        _ => "unknown",
                    },
                    repeat: t.repeat,
                })
                .collect(),
            slices: self.slices.clone(),
        }
    }
}

/// A slice's key in frame `f`: the last that starts at or before it, if it
/// has an area.
fn key_at(s: &SliceInfo, f: u32) -> Option<&SliceKey> {
    s.keys
        .iter()
        .rev()
        .find(|k| k.frame <= f)
        .filter(|k| k.bounds[2] > 0 && k.bounds[3] > 0)
}

/// The columns and rows that lay out `n` cells of `w` x `h` on the
/// smallest power-of-two texture (then the squarest), within `MAX_SHEET`.
fn layout(n: u32, w: u32, h: u32) -> Option<(u32, u32)> {
    (1..=n.max(1))
        .filter_map(|cols| {
            let rows = n.max(1).div_ceil(cols);
            let (sw, sh) = (cols as u64 * w as u64, rows as u64 * h as u64);
            if sw > MAX_SHEET || sh > MAX_SHEET {
                return None;
            }
            let (tw, th) = (pow2(sw as u32) as u64, pow2(sh as u32) as u64);
            Some(((tw * th, tw.max(th), cols), cols, rows))
        })
        .min()
        .map(|(_, c, r)| (c, r))
}

/// An Aseprite file's structure (its layers, tags, and slices), read
/// without drawing it; every layer a plain import would draw is `used`.
/// The error says what is wrong with the file.
pub fn describe(bytes: &[u8]) -> std::result::Result<Info, String> {
    let f = parse(bytes)?;
    let (used, _, _) = f.pick(None);
    Ok(f.info(&used))
}

impl Importer {
    /// A sprite whose source is an Aseprite file (see the module's doc).
    pub(crate) fn import_aseprite(
        &self,
        name: &str,
        g: &Group,
        file: &str,
        side: Option<&str>,
        side_file: Option<&str>,
        mut hash: u64,
    ) -> Result<(Entry, Vec<String>)> {
        let sf = side_file.unwrap_or("");
        let shown = self.shown(g.layer, file);
        if let Some(n) = &g.normal {
            let n = self.shown(g.layer, n);
            return Err(ArgusError::new(
                codes::AMBIGUOUS,
                format!("{n} is a normal map for {shown}, and a sprite drawn from an Aseprite file takes none yet"),
            )
            .hint("The importer lays an Aseprite file's frames out itself, so a normal map cannot match them; draw the sprite from a PNG to light it with a normal map.")
            .at(Some(n)));
        }
        let sc = match side {
            Some(t) => sidecar::parse_sprite_with(sf, t, true)?,
            None => sidecar::SpriteSidecar::default(),
        };
        let bytes = self.read_in(g.layer, file)?;
        fnv(&mut hash, &bytes);
        let ase = parse(&bytes).map_err(|m| {
            ArgusError::new(
                codes::BAD_SOURCE,
                format!("{shown} cannot be read as an Aseprite file: {m}"),
            )
            .hint("Save it again from Aseprite; a file cut short, or one another tool wrote, is the usual cause.")
            .at(Some(shown.clone()))
        })?;
        let mut warnings = Vec::new();
        let warn = |w: &mut Vec<Warning>, message: String, at: &str| {
            w.push(Warning {
                message,
                at: at.to_string(),
            })
        };
        for k in &sc.unknown {
            warn(&mut warnings, format!("unknown key {k:?}, ignored"), sf);
        }
        if sc.grid.is_some() {
            warn(
                &mut warnings,
                format!("grid is ignored: the frames are {shown}'s"),
                sf,
            );
        }
        if sc.shape.is_some() {
            warn(
                &mut warnings,
                format!("kind, size, and color are ignored: {shown} is the art now"),
                sf,
            );
        }
        for n in &ase.notes {
            warn(&mut warnings, n.clone(), &shown);
        }
        // The layers drawn.
        let (used, missing, notes) = ase.pick(sc.layers.as_deref());
        if !missing.is_empty() {
            let have: Vec<String> = (0..ase.layers.len()).map(|i| ase.path(i)).collect();
            let m = format!(
                "layers names {}, which {shown} does not have; it has {}",
                missing
                    .iter()
                    .map(|n| format!("{n:?}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                have.join(", ")
            );
            if sc.layers.is_some() && !used.iter().any(|&u| u) {
                return Err(ArgusError::new(codes::BAD_SIDECAR, m)
                    .hint("Name layers as asset.get lists them: a layer's name, or its path through its groups (\"fx/sparks\").")
                    .at(Some(sc.layers_at.clone())));
            }
            warn(&mut warnings, m, &sc.layers_at);
        }
        for n in notes {
            warn(&mut warnings, n, &shown);
        }
        let (images, notes) = ase.composite(&used);
        for n in notes {
            warn(&mut warnings, n, &shown);
        }
        // Frames that look the same share a cell of the sheet.
        let (fw, fh) = (ase.w, ase.h);
        let mut cell_of: Vec<u32> = Vec::with_capacity(images.len());
        let mut cells: Vec<usize> = Vec::new();
        {
            let mut seen: HashMap<&[u8], u32> = HashMap::new();
            for (i, img) in images.iter().enumerate() {
                let c = *seen.entry(img.as_slice()).or_insert_with(|| {
                    cells.push(i);
                    cells.len() as u32 - 1
                });
                cell_of.push(c);
            }
        }
        let (cols, rows) = layout(cells.len() as u32, fw, fh).ok_or_else(|| {
            ArgusError::new(
                codes::BAD_SOURCE,
                format!(
                    "{shown}'s {} different frames of {fw} x {fh} do not fit on a {MAX_SHEET}-pixel sheet",
                    cells.len()
                ),
            )
            .hint("Split the animation across files, or make the canvas smaller.")
            .at(Some(shown.clone()))
        })?;
        let (w, h) = (cols * fw, rows * fh);
        let (tw, th) = (pow2(w), pow2(h));
        let mut sheet = vec![0u8; (tw * th * 4) as usize];
        for (c, &fi) in cells.iter().enumerate() {
            let (x0, y0) = ((c as u32 % cols) * fw, (c as u32 / cols) * fh);
            for y in 0..fh {
                let from = (y * fw * 4) as usize;
                let to = (((y0 + y) * tw + x0) * 4) as usize;
                sheet[to..to + (fw * 4) as usize]
                    .copy_from_slice(&images[fi][from..from + (fw * 4) as usize]);
            }
        }
        repeat_edges(&mut sheet, w, h, tw, th);
        // Frames: the file's durations, pivots from a slice, properties.
        let pivot_slice = ase.pivot_slice();
        if let Some(p) = pivot_slice {
            let others: Vec<&str> = ase
                .slices
                .iter()
                .enumerate()
                .filter(|&(i, s)| i != p && s.keys.iter().any(|k| k.pivot.is_some()))
                .map(|(_, s)| s.name.as_str())
                .collect();
            if !others.is_empty() && ase.slices[p].name != "pivot" {
                warn(
                    &mut warnings,
                    format!(
                        "slices {:?} and {} have pivots; {:?}'s is used (name the one you mean pivot)",
                        ase.slices[p].name,
                        others
                            .iter()
                            .map(|n| format!("{n:?}"))
                            .collect::<Vec<_>>()
                            .join(", "),
                        ase.slices[p].name
                    ),
                    &shown,
                );
            }
        }
        let pivots = ase.pivots(pivot_slice);
        let props = ase.properties(&used);
        let mut frames: Vec<Frame> = (0..images.len())
            .map(|i| {
                let c = cell_of[i];
                Frame {
                    rect: [(c % cols) * fw, (c / cols) * fh, fw, fh],
                    pivot: sc.pivot.or(pivots[i]).unwrap_or([0.5, 0.5]),
                    duration_ms: sc.duration_ms.unwrap_or(ase.frames[i].ms),
                    properties: props[i].clone(),
                }
            })
            .collect();
        // The sidecar's frames: by index only.
        for f in &sc.frames {
            if f.rect.is_some() {
                warn(
                    &mut warnings,
                    format!("a frame's rect is ignored: the frames are {shown}'s"),
                    &f.at,
                );
            }
            let Some(i) = f.index else {
                if f.rect.is_none() {
                    warn(
                        &mut warnings,
                        "a [[frames]] entry with neither rect nor index describes nothing"
                            .to_string(),
                        &f.at,
                    );
                }
                continue;
            };
            let Some(fr) = frames.get_mut(i as usize) else {
                warn(
                    &mut warnings,
                    format!("frame {i} does not exist; {shown} has {}", images.len()),
                    &f.at,
                );
                continue;
            };
            if let Some(p) = f.pivot {
                fr.pivot = p;
            }
            if let Some(d) = f.duration_ms {
                fr.duration_ms = d;
            }
            fr.properties.extend(f.properties.clone());
            if let Some(s) = fr.properties.get("slope") {
                if parse_slope(s).is_none() {
                    warn(
                        &mut warnings,
                        format!("slope = {s} is not [left, right] with heights from 0 to 1; the tile is solid"),
                        &f.at,
                    );
                }
            }
        }
        for (i, fr) in frames.iter_mut().enumerate() {
            if !(0.0..=1.0).contains(&fr.pivot[0]) || !(0.0..=1.0).contains(&fr.pivot[1]) {
                warn(
                    &mut warnings,
                    format!(
                        "frame {i} pivot {:?} is outside the frame; clamped",
                        fr.pivot
                    ),
                    if sc.pivot.is_some() {
                        sf
                    } else {
                        shown.as_str()
                    },
                );
                fr.pivot = [fr.pivot[0].clamp(0.0, 1.0), fr.pivot[1].clamp(0.0, 1.0)];
            }
            if images[i].as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
                warn(&mut warnings, format!("frame {i} is empty"), &shown);
            }
        }
        // Clips: the tags, then the sidecar's changes and its own.
        let mut notes = Vec::new();
        let (mut clips, ranges) = ase.clips(&mut notes);
        for n in notes {
            warn(&mut warnings, n, &shown);
        }
        for c in &sc.clips {
            let mut keep = Vec::new();
            for &f in &c.frames {
                if (f as usize) < frames.len() {
                    keep.push(f);
                } else {
                    warn(
                        &mut warnings,
                        format!(
                            "clip {} names frame {f}; the sheet has {} frames",
                            c.name,
                            frames.len()
                        ),
                        &c.at,
                    );
                }
            }
            if c.frames_given && keep.is_empty() {
                warn(
                    &mut warnings,
                    format!("clip {} has no frames left; dropped", c.name),
                    &c.at,
                );
                continue;
            }
            match clips.get_mut(&c.name) {
                Some(clip) => {
                    if c.direction_given && !c.frames_given {
                        let (a, b) = ranges[&c.name];
                        clip.frames = (a..=b).collect();
                    }
                    if c.frames_given {
                        clip.frames = keep;
                    }
                    if c.direction_given {
                        clip.direction = c.direction;
                    }
                    if c.fps.is_some() {
                        clip.fps = c.fps;
                    }
                    if c.loop_given {
                        clip.looping = c.looping;
                    }
                }
                None if !c.frames_given => {
                    let tags: Vec<&str> = ase.tags.iter().map(|t| t.name.as_str()).collect();
                    return Err(ArgusError::new(
                        codes::BAD_SIDECAR,
                        format!(
                            "clips.{} needs frames = [...] or from and to: {shown} has no tag {:?} for it to change (its tags: {})",
                            c.name,
                            c.name,
                            if tags.is_empty() { "none".to_string() } else { tags.join(", ") }
                        ),
                    )
                    .at(Some(c.at.clone())));
                }
                None => {
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
        }
        // Auto-tiling rules must name tiles the sheet has.
        let mut autotile = BTreeMap::new();
        for a in &sc.autotile {
            if let Some(bad) = a.tiles.iter().find(|&&t| t as usize > frames.len()) {
                return Err(ArgusError::new(
                    codes::BAD_SIDECAR,
                    format!("autotile.{} names tile {bad}; the sheet has {} frames (tile n is frame n - 1)", a.terrain, frames.len()),
                )
                .at(Some(a.at.clone())));
            }
            autotile.insert(a.terrain.clone(), a.clone());
        }
        let texture = self.cache.join(format!("{name}.png"));
        encode_png(&texture, tw, th, &sheet)?;
        Ok((
            Entry {
                name: name.to_string(),
                sidecar: g.sidecar.clone(),
                hash,
                warnings,
                asset: Asset::Sprite(SpriteAsset {
                    source: Some(file.to_string()),
                    width: w,
                    height: h,
                    texture,
                    texture_size: [tw, th],
                    normal: None,
                    frames,
                    clips,
                    pixels_per_unit: sc.pixels_per_unit.unwrap_or(16.0),
                    shape: None,
                    autotile,
                    aseprite: Some(ase.info(&used)),
                }),
                ms: 0.0,
            },
            Vec::new(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_keeps_the_texture_small_and_square() {
        // Hollow's hero: 8 frames of 8 x 13 fit 32 x 32 as 4 x 2.
        assert_eq!(layout(8, 8, 13), Some((4, 2)));
        assert_eq!(layout(1, 30, 20), Some((1, 1)));
        // 3 frames of 16 x 16: 2 x 2 is 32 x 32, smaller than 64 x 16? Same
        // area; the squarer wins.
        assert_eq!(layout(3, 16, 16), Some((2, 2)));
        assert_eq!(layout(2, 9000, 9000), None);
    }

    #[test]
    fn normal_blending_is_aseprites_arithmetic() {
        let mut d = [0u8, 0, 0, 0];
        blend(&mut d, [200, 100, 50, 255], 128);
        assert_eq!(d, [200, 100, 50, 128]);
        let mut d = [0u8, 0, 255, 255];
        blend(&mut d, [255, 0, 0, 255], 128);
        assert_eq!(d, [128, 0, 127, 255]);
        let mut d = [10u8, 20, 30, 255];
        blend(&mut d, [255, 255, 255, 0], 255);
        assert_eq!(d, [10, 20, 30, 255]);
    }
}
