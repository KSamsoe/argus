//! Art scripts (asset generation, item 2): `assets/<name>.art.luau`, a
//! Luau file the importer runs to draw a sprite, in place of a PNG and its
//! sidecar. It draws on canvases with a pixel-art kit (`art`) and returns
//! the sprite: its frames, clips, pivots, and pixels per unit. The importer
//! packs the frames into a sheet and caches it as it caches a PNG, so the
//! renderer, a release pack, and the look check see an ordinary sprite;
//! a player's machine never runs the script.
//!
//! The VM is sandboxed (no `os`, no files, no clock, no network) and
//! deterministic everywhere: `math`'s functions are libm's, pure Rust, not
//! the platform's; there is no `math.random` (`art.rng(seed)` instead);
//! noise is the kit's own. So a script draws the same pixels on every
//! machine. Coordinates are pixels, x right and y down from the top left,
//! as in a PNG; a pivot is a fraction of the frame, y up from the bottom,
//! as in a sprite sidecar.

use crate::sidecar::Direction;
use crate::{codes, Clip};
use argus_ecs::{ArgusError, Result};
use mlua::{
    AnyUserData, Lua, Table, UserData, UserDataFields, UserDataMethods, UserDataRef, Value,
    Variadic, VmState,
};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

/// A canvas's widest side, and a frame's.
pub const MAX_SIDE: u32 = 2048;
/// How long a script may run.
const TIME_LIMIT_S: f64 = 10.0;
/// How much memory it may use.
const MEMORY: usize = 512 << 20;

/// An RGBA picture to draw on.
#[derive(Clone, Debug)]
pub struct Canvas {
    id: u64,
    /// Width.
    pub w: u32,
    /// Height.
    pub h: u32,
    /// RGBA rows, top first.
    pub rgba: Vec<u8>,
}

/// One frame of the sprite a script returns.
#[derive(Clone, Debug)]
pub struct ArtFrame {
    /// Its picture.
    pub canvas: Canvas,
    /// Its pivot, a fraction of the frame (y up from the bottom).
    pub pivot: [f32; 2],
    /// How long it shows when its clip says no rate.
    pub ms: u32,
    /// Its properties (a tile's slope).
    pub properties: BTreeMap<String, String>,
}

/// The sprite a script returns.
#[derive(Clone, Debug)]
pub struct ArtSprite {
    /// Frames, numbered from 0 as Sprite.frame counts them.
    pub frames: Vec<ArtFrame>,
    /// Clips by name.
    pub clips: BTreeMap<String, Clip>,
    /// World scale.
    pub pixels_per_unit: f32,
    /// What the script printed.
    pub printed: Vec<String>,
}

struct Ids(Cell<u64>);

fn next_id(lua: &Lua) -> u64 {
    let ids = lua.app_data_ref::<Ids>().expect("ids");
    let n = ids.0.get() + 1;
    ids.0.set(n);
    n
}

/// A value's kind, in a word with its article: "a number", "a table".
fn kind(v: &Value) -> &'static str {
    match v {
        Value::Nil => "nothing",
        Value::Boolean(_) => "a boolean",
        Value::Integer(_) | Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Table(_) => "a table",
        Value::Function(_) => "a function",
        Value::UserData(_) => "a canvas or a generator",
        _ => "something else",
    }
}

fn bad(msg: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(msg.into())
}

/// A colour: `#RRGGBB`, `#RRGGBBAA`, `#RGB`, or `none` (clear).
pub fn color(v: &Value) -> mlua::Result<[u8; 4]> {
    let s = match v {
        Value::String(s) => s.to_str()?.to_string(),
        Value::Nil => {
            return Err(bad(
                "a colour is missing: give \"#RRGGBB\", \"#RRGGBBAA\", or \"none\"",
            ))
        }
        other => {
            return Err(bad(format!(
                "a colour is a string such as \"#3A6EA5\", not {}",
                kind(other)
            )))
        }
    };
    parse_hex(&s).ok_or_else(|| {
        bad(format!(
            "{s:?} is not a colour: give \"#RRGGBB\", \"#RRGGBBAA\", \"#RGB\", or \"none\""
        ))
    })
}

fn parse_hex(s: &str) -> Option<[u8; 4]> {
    let t = s.trim();
    if t.eq_ignore_ascii_case("none") || t.eq_ignore_ascii_case("clear") {
        return Some([0, 0, 0, 0]);
    }
    let h = t.strip_prefix('#')?;
    let b = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    match h.len() {
        3 => {
            let d = |i: usize| {
                u8::from_str_radix(h.get(i..i + 1)?, 16)
                    .ok()
                    .map(|x| x * 17)
            };
            Some([d(0)?, d(1)?, d(2)?, 255])
        }
        6 => Some([b(0)?, b(2)?, b(4)?, 255]),
        8 => Some([b(0)?, b(2)?, b(4)?, b(6)?]),
        _ => None,
    }
}

fn hex(c: [u8; 4]) -> String {
    if c[3] == 255 {
        format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
    } else {
        format!("#{:02X}{:02X}{:02X}{:02X}", c[0], c[1], c[2], c[3])
    }
}

/// Over: `src` composited onto `dst` by its alpha (in sRGB, as pixel art
/// is painted).
fn over(dst: [u8; 4], src: [u8; 4]) -> [u8; 4] {
    if src[3] == 255 || dst[3] == 0 {
        return src;
    }
    if src[3] == 0 {
        return dst;
    }
    let sa = src[3] as f32 / 255.0;
    let da = dst[3] as f32 / 255.0;
    let oa = sa + da * (1.0 - sa);
    let ch = |i: usize| {
        let v = (src[i] as f32 * sa + dst[i] as f32 * da * (1.0 - sa)) / oa;
        libm::roundf(v).clamp(0.0, 255.0) as u8
    };
    [ch(0), ch(1), ch(2), libm::roundf(oa * 255.0) as u8]
}

impl Canvas {
    fn new(lua: &Lua, w: u32, h: u32, fill: [u8; 4]) -> mlua::Result<Canvas> {
        if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
            return Err(bad(format!(
                "a canvas is 1 to {MAX_SIDE} pixels a side, not {w} x {h}"
            )));
        }
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..w * h {
            rgba.extend_from_slice(&fill);
        }
        Ok(Canvas {
            id: next_id(lua),
            w,
            h,
            rgba,
        })
    }

    fn fresh(&self, lua: &Lua, w: u32, h: u32) -> mlua::Result<Canvas> {
        Canvas::new(lua, w, h, [0; 4])
    }

    fn inside(&self, x: i64, y: i64) -> bool {
        x >= 0 && y >= 0 && (x as u32) < self.w && (y as u32) < self.h
    }

    fn at(&self, x: i64, y: i64) -> [u8; 4] {
        if !self.inside(x, y) {
            return [0; 4];
        }
        let i = ((y as u32 * self.w + x as u32) * 4) as usize;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }

    fn put(&mut self, x: i64, y: i64, c: [u8; 4]) {
        if self.inside(x, y) {
            let i = ((y as u32 * self.w + x as u32) * 4) as usize;
            self.rgba[i..i + 4].copy_from_slice(&c);
        }
    }

    fn opaque(&self, x: i64, y: i64) -> bool {
        self.at(x, y)[3] > 0
    }

    fn line(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, c: [u8; 4]) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let (mut x, mut y, mut err) = (x0, y0, dx + dy);
        loop {
            self.put(x, y, c);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Every pixel whose centre `inside` takes, in the box around it.
    fn shape(
        &mut self,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        c: [u8; 4],
        inside: impl Fn(f64, f64) -> bool,
    ) {
        let (a, b) = (
            libm::floor(x0).max(0.0) as i64,
            libm::floor(y0).max(0.0) as i64,
        );
        let (e, f) = (
            (libm::ceil(x1) as i64).min(self.w as i64 - 1),
            (libm::ceil(y1) as i64).min(self.h as i64 - 1),
        );
        for y in b..=f {
            for x in a..=e {
                if inside(x as f64, y as f64) {
                    self.put(x, y, c);
                }
            }
        }
    }
}

fn int(v: f64) -> i64 {
    libm::round(v) as i64
}

/// The kit's value noise: smooth, 0 to 1, the same everywhere.
pub fn noise(x: f64, y: f64, seed: u64) -> f64 {
    let lattice = |ix: i64, iy: i64| {
        let mut h = seed
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add((ix as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F))
            .wrapping_add((iy as u64).wrapping_mul(0x1656_67B1_9E37_79F9));
        h ^= h >> 33;
        h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
        h ^= h >> 33;
        (h >> 11) as f64 / (1u64 << 53) as f64
    };
    let (fx, fy) = (libm::floor(x), libm::floor(y));
    let (tx, ty) = (x - fx, y - fy);
    let s = |t: f64| t * t * (3.0 - 2.0 * t);
    let (ix, iy) = (fx as i64, fy as i64);
    let a = lattice(ix, iy) + (lattice(ix + 1, iy) - lattice(ix, iy)) * s(tx);
    let b = lattice(ix, iy + 1) + (lattice(ix + 1, iy + 1) - lattice(ix, iy + 1)) * s(tx);
    a + (b - a) * s(ty)
}

/// A seeded generator for scripts (PCG32, as the engine's streams).
struct Rng {
    state: u64,
    inc: u64,
}

impl Rng {
    fn new(seed: u64) -> Rng {
        let mut r = Rng {
            state: 0,
            inc: (0xDA3E_39CB_94B9_5BDB << 1) | 1,
        };
        r.next();
        r.state = r.state.wrapping_add(seed);
        r.next();
        r
    }
    fn next(&mut self) -> u32 {
        let old = self.state;
        self.state = old
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(self.inc);
        let x = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        x.rotate_right(rot)
    }
    fn float(&mut self) -> f64 {
        self.next() as f64 / 4_294_967_296.0
    }
}

impl UserData for Rng {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method_mut("int", |_, r, (lo, hi): (i64, i64)| {
            if hi < lo {
                return Err(bad(format!("int({lo}, {hi}): the second is the larger")));
            }
            let span = (hi - lo + 1) as f64;
            Ok(lo + libm::floor(r.float() * span) as i64)
        });
        m.add_method_mut("float", |_, r, (lo, hi): (Option<f64>, Option<f64>)| {
            let (lo, hi) = (lo.unwrap_or(0.0), hi.unwrap_or(1.0));
            Ok(lo + (hi - lo) * r.float())
        });
        m.add_method_mut("chance", |_, r, p: f64| Ok(r.float() < p));
        m.add_method_mut("pick", |_, r, t: Table| {
            let n = t.raw_len();
            if n == 0 {
                return Err(bad("pick from an empty list"));
            }
            let i = 1 + libm::floor(r.float() * n as f64) as usize;
            t.raw_get::<Value>(i.min(n))
        });
    }
}

fn mix(a: [u8; 4], b: [u8; 4], t: f64) -> [u8; 4] {
    let t = t.clamp(0.0, 1.0);
    let ch = |i: usize| libm::round(a[i] as f64 + (b[i] as f64 - a[i] as f64) * t) as u8;
    [ch(0), ch(1), ch(2), ch(3)]
}

fn hsv(h: f64, s: f64, v: f64) -> [u8; 3] {
    let h = ((h % 360.0) + 360.0) % 360.0 / 60.0;
    let (s, v) = (s.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
    let c = v * s;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let q = |u: f64| libm::round((u + m) * 255.0) as u8;
    [q(r), q(g), q(b)]
}

/// `c:noise(x, y, w, h, colors, scale, seed, only_drawn)`.
type NoiseArgs = (
    f64,
    f64,
    f64,
    f64,
    Table,
    Option<f64>,
    Option<u64>,
    Option<bool>,
);

const BAYER: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];

impl UserData for Canvas {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("w", |_, c| Ok(c.w));
        f.add_field_method_get("h", |_, c| Ok(c.h));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method_mut("pixel", |_, c, (x, y, col): (f64, f64, Value)| {
            c.put(int(x), int(y), color(&col)?);
            Ok(())
        });
        m.add_method("get", |_, c, (x, y): (f64, f64)| {
            let (x, y) = (int(x), int(y));
            Ok(c.inside(x, y).then(|| hex(c.at(x, y))))
        });
        m.add_method_mut(
            "rect",
            |_, c, (x, y, w, h, col): (f64, f64, f64, f64, Value)| {
                let col = color(&col)?;
                let (x, y, w, h) = (int(x), int(y), int(w), int(h));
                for yy in y..y + h {
                    for xx in x..x + w {
                        c.put(xx, yy, col);
                    }
                }
                Ok(())
            },
        );
        m.add_method_mut(
            "box",
            |_, c, (x, y, w, h, col): (f64, f64, f64, f64, Value)| {
                let col = color(&col)?;
                let (x, y, w, h) = (int(x), int(y), int(w), int(h));
                if w > 0 && h > 0 {
                    c.line(x, y, x + w - 1, y, col);
                    c.line(x, y + h - 1, x + w - 1, y + h - 1, col);
                    c.line(x, y, x, y + h - 1, col);
                    c.line(x + w - 1, y, x + w - 1, y + h - 1, col);
                }
                Ok(())
            },
        );
        m.add_method_mut(
            "line",
            |_, c, (x0, y0, x1, y1, col): (f64, f64, f64, f64, Value)| {
                c.line(int(x0), int(y0), int(x1), int(y1), color(&col)?);
                Ok(())
            },
        );
        m.add_method_mut(
            "circle",
            |_, c, (cx, cy, r, col): (f64, f64, f64, Value)| {
                let col = color(&col)?;
                let rr = (r + 0.5) * (r + 0.5);
                c.shape(
                    cx - r - 1.0,
                    cy - r - 1.0,
                    cx + r + 1.0,
                    cy + r + 1.0,
                    col,
                    |x, y| (x - cx) * (x - cx) + (y - cy) * (y - cy) <= rr,
                );
                Ok(())
            },
        );
        m.add_method_mut("ring", |_, c, (cx, cy, r, col): (f64, f64, f64, Value)| {
            let col = color(&col)?;
            let (lo, hi) = ((r - 0.5).max(0.0), r + 0.5);
            c.shape(
                cx - r - 1.0,
                cy - r - 1.0,
                cx + r + 1.0,
                cy + r + 1.0,
                col,
                |x, y| {
                    let d = (x - cx) * (x - cx) + (y - cy) * (y - cy);
                    d > lo * lo && d <= hi * hi
                },
            );
            Ok(())
        });
        m.add_method_mut(
            "ellipse",
            |_, c, (cx, cy, rx, ry, col): (f64, f64, f64, f64, Value)| {
                let col = color(&col)?;
                let (ax, ay) = (rx + 0.5, ry + 0.5);
                c.shape(
                    cx - ax - 1.0,
                    cy - ay - 1.0,
                    cx + ax + 1.0,
                    cy + ay + 1.0,
                    col,
                    |x, y| {
                        let (u, v) = ((x - cx) / ax, (y - cy) / ay);
                        u * u + v * v <= 1.0
                    },
                );
                Ok(())
            },
        );
        m.add_method_mut("poly", |_, c, (pts, col): (Table, Value)| {
            let col = color(&col)?;
            let v: Vec<f64> = pts.sequence_values::<f64>().collect::<mlua::Result<_>>()?;
            if v.len() < 6 || v.len() % 2 == 1 {
                return Err(bad(
                    "poly takes { x1, y1, x2, y2, x3, y3, ... }: three points or more",
                ));
            }
            let p: Vec<(f64, f64)> = v.chunks(2).map(|q| (q[0], q[1])).collect();
            let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for &(x, y) in &p {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
            c.shape(x0, y0, x1, y1, col, |x, y| {
                // Even-odd at the pixel's centre, the points at pixel centres.
                let mut inside = false;
                let n = p.len();
                for i in 0..n {
                    let (ax, ay) = p[i];
                    let (bx, by) = p[(i + n - 1) % n];
                    if (ay > y) != (by > y) && x < (bx - ax) * (y - ay) / (by - ay) + ax {
                        inside = !inside;
                    }
                }
                inside || p.iter().any(|&(px, py)| px == x && py == y)
            });
            Ok(())
        });
        // Pixel art as text: each row a string, each character a colour
        // from `key` (a character the key does not have is left as it is,
        // so "." is clear), drawn from (x, y).
        m.add_method_mut("rows", |_, c, (x, y, rows, key): (f64, f64, Table, Table)| {
            let mut colours: HashMap<char, [u8; 4]> = HashMap::new();
            for kv in key.pairs::<String, Value>() {
                let (k, v) = kv?;
                let mut chars = k.chars();
                let (Some(ch), None) = (chars.next(), chars.next()) else {
                    return Err(bad(format!(
                        "rows' key maps one character to a colour, not {k:?}"
                    )));
                };
                colours.insert(ch, color(&v)?);
            }
            let (x, y) = (int(x), int(y));
            for (r, row) in rows.sequence_values::<String>().enumerate() {
                for (i, ch) in row?.chars().enumerate() {
                    if let Some(col) = colours.get(&ch) {
                        c.put(x + i as i64, y + r as i64, *col);
                    }
                }
            }
            Ok(())
        });
        m.add_method_mut("fill", |_, c, (x, y, col): (f64, f64, Value)| {
            let col = color(&col)?;
            let (x, y) = (int(x), int(y));
            if !c.inside(x, y) {
                return Ok(());
            }
            let from = c.at(x, y);
            if from == col {
                return Ok(());
            }
            let mut stack = vec![(x, y)];
            while let Some((px, py)) = stack.pop() {
                if !c.inside(px, py) || c.at(px, py) != from {
                    continue;
                }
                c.put(px, py, col);
                stack.extend([(px + 1, py), (px - 1, py), (px, py + 1), (px, py - 1)]);
            }
            Ok(())
        });
        m.add_method_mut("outline", |_, c, (col, corners): (Value, Option<bool>)| {
            let col = color(&col)?;
            let snap = c.clone();
            let corners = corners.unwrap_or(false);
            for y in 0..c.h as i64 {
                for x in 0..c.w as i64 {
                    if snap.opaque(x, y) {
                        continue;
                    }
                    let mut near = snap.opaque(x + 1, y)
                        || snap.opaque(x - 1, y)
                        || snap.opaque(x, y + 1)
                        || snap.opaque(x, y - 1);
                    if corners && !near {
                        near = snap.opaque(x + 1, y + 1)
                            || snap.opaque(x - 1, y + 1)
                            || snap.opaque(x + 1, y - 1)
                            || snap.opaque(x - 1, y - 1);
                    }
                    if near {
                        c.put(x, y, col);
                    }
                }
            }
            Ok(())
        });
        m.add_method_mut("shade", |_, c, o: Table| {
            let light = match o.get::<Value>("light")? {
                Value::Nil => None,
                v => Some(color(&v)?),
            };
            let dark = match o.get::<Value>("dark")? {
                Value::Nil => None,
                v => Some(color(&v)?),
            };
            let (dx, dy) = match o.get::<Option<Table>>("from")? {
                Some(t) => (t.get::<i64>(1)?, t.get::<i64>(2)?),
                None => (-1, -1),
            };
            let snap = c.clone();
            for y in 0..c.h as i64 {
                for x in 0..c.w as i64 {
                    if !snap.opaque(x, y) {
                        continue;
                    }
                    if let Some(l) = light {
                        if !snap.opaque(x + dx.signum(), y + dy.signum()) {
                            c.put(x, y, l);
                            continue;
                        }
                    }
                    if let Some(d) = dark {
                        if !snap.opaque(x - dx.signum(), y - dy.signum()) {
                            c.put(x, y, d);
                        }
                    }
                }
            }
            Ok(())
        });
        m.add_method_mut(
            "dither",
            |_, c, (x, y, w, h, col, amount): (f64, f64, f64, f64, Value, f64)| {
                let col = color(&col)?;
                let k = (amount.clamp(0.0, 1.0) * 16.0) as u8;
                let (x, y, w, h) = (int(x), int(y), int(w), int(h));
                for yy in y.max(0)..(y + h).min(c.h as i64) {
                    for xx in x.max(0)..(x + w).min(c.w as i64) {
                        if BAYER[((yy & 3) * 4 + (xx & 3)) as usize] < k {
                            c.put(xx, yy, col);
                        }
                    }
                }
                Ok(())
            },
        );
        m.add_method_mut(
            "noise",
            |_, c, (x, y, w, h, cols, scale, seed, only): NoiseArgs| {
                let cols: Vec<[u8; 4]> = cols
                    .sequence_values::<Value>()
                    .map(|v| color(&v?))
                    .collect::<mlua::Result<_>>()?;
                if cols.is_empty() {
                    return Err(bad("noise needs a list of colours"));
                }
                let scale = scale.unwrap_or(4.0).max(0.01);
                let (seed, only) = (seed.unwrap_or(1), only.unwrap_or(false));
                let (x, y, w, h) = (int(x), int(y), int(w), int(h));
                for yy in y.max(0)..(y + h).min(c.h as i64) {
                    for xx in x.max(0)..(x + w).min(c.w as i64) {
                        if only && !c.opaque(xx, yy) {
                            continue;
                        }
                        let n = noise(xx as f64 / scale, yy as f64 / scale, seed);
                        let i = ((n * cols.len() as f64) as usize).min(cols.len() - 1);
                        c.put(xx, yy, cols[i]);
                    }
                }
                Ok(())
            },
        );
        m.add_method_mut(
            "speckle",
            |_, c, (cols, amount, seed): (Table, f64, Option<u64>)| {
                let cols: Vec<[u8; 4]> = cols
                    .sequence_values::<Value>()
                    .map(|v| color(&v?))
                    .collect::<mlua::Result<_>>()?;
                if cols.is_empty() {
                    return Err(bad("speckle needs a list of colours"));
                }
                let mut r = Rng::new(seed.unwrap_or(1));
                for y in 0..c.h as i64 {
                    for x in 0..c.w as i64 {
                        let hit = r.float() < amount;
                        let pick = (r.float() * cols.len() as f64) as usize;
                        if hit && c.opaque(x, y) {
                            c.put(x, y, cols[pick.min(cols.len() - 1)]);
                        }
                    }
                }
                Ok(())
            },
        );
        m.add_method_mut("mirror", |_, c, axis: Option<String>| {
            match axis.as_deref().unwrap_or("x") {
                "x" => {
                    for y in 0..c.h as i64 {
                        for x in 0..(c.w / 2) as i64 {
                            let p = c.at(x, y);
                            c.put(c.w as i64 - 1 - x, y, p);
                        }
                    }
                }
                "y" => {
                    for y in 0..(c.h / 2) as i64 {
                        for x in 0..c.w as i64 {
                            let p = c.at(x, y);
                            c.put(x, c.h as i64 - 1 - y, p);
                        }
                    }
                }
                a => return Err(bad(format!("mirror takes \"x\" or \"y\", not {a:?}"))),
            }
            Ok(())
        });
        m.add_method("flip", |lua, c, axis: Option<String>| {
            let mut o = c.fresh(lua, c.w, c.h)?;
            let x_axis = match axis.as_deref().unwrap_or("x") {
                "x" => true,
                "y" => false,
                a => return Err(bad(format!("flip takes \"x\" or \"y\", not {a:?}"))),
            };
            for y in 0..c.h as i64 {
                for x in 0..c.w as i64 {
                    let (sx, sy) = if x_axis {
                        (c.w as i64 - 1 - x, y)
                    } else {
                        (x, c.h as i64 - 1 - y)
                    };
                    o.put(x, y, c.at(sx, sy));
                }
            }
            Ok(o)
        });
        m.add_method("rotate", |lua, c, turns: Option<i64>| {
            let t = turns.unwrap_or(1).rem_euclid(4);
            let (w, h) = if t % 2 == 1 { (c.h, c.w) } else { (c.w, c.h) };
            let mut o = c.fresh(lua, w, h)?;
            let (cw, ch) = (c.w as i64, c.h as i64);
            for y in 0..ch {
                for x in 0..cw {
                    let (nx, ny) = match t {
                        0 => (x, y),
                        1 => (ch - 1 - y, x),
                        2 => (cw - 1 - x, ch - 1 - y),
                        _ => (y, cw - 1 - x),
                    };
                    o.put(nx, ny, c.at(x, y));
                }
            }
            Ok(o)
        });
        m.add_method_mut(
            "draw",
            |_, c, (other, x, y): (UserDataRef<Canvas>, Option<f64>, Option<f64>)| {
                let (ox, oy) = (int(x.unwrap_or(0.0)), int(y.unwrap_or(0.0)));
                for yy in 0..other.h as i64 {
                    for xx in 0..other.w as i64 {
                        let s = other.at(xx, yy);
                        if s[3] > 0 && c.inside(ox + xx, oy + yy) {
                            let d = c.at(ox + xx, oy + yy);
                            c.put(ox + xx, oy + yy, over(d, s));
                        }
                    }
                }
                Ok(())
            },
        );
        m.add_method_mut("recolor", |_, c, map: Table| {
            let mut pairs = Vec::new();
            for kv in map.pairs::<Value, Value>() {
                let (k, v) = kv?;
                pairs.push((color(&k)?, color(&v)?));
            }
            for px in c.rgba.as_chunks_mut::<4>().0 {
                if let Some((_, to)) = pairs.iter().find(|(from, _)| from == px) {
                    *px = *to;
                }
            }
            Ok(())
        });
        m.add_method("copy", |lua, c, ()| {
            let mut o = c.clone();
            o.id = next_id(lua);
            Ok(o)
        });
        m.add_method("crop", |lua, c, (x, y, w, h): (f64, f64, f64, f64)| {
            let (x, y) = (int(x), int(y));
            let mut o = Canvas::new(lua, int(w).max(1) as u32, int(h).max(1) as u32, [0; 4])?;
            for yy in 0..o.h as i64 {
                for xx in 0..o.w as i64 {
                    o.put(xx, yy, c.at(x + xx, y + yy));
                }
            }
            Ok(o)
        });
        m.add_method("scale", |lua, c, k: u32| {
            if k == 0 {
                return Err(bad("scale takes a whole number from 1"));
            }
            let mut o = Canvas::new(lua, c.w * k, c.h * k, [0; 4])?;
            for y in 0..o.h as i64 {
                for x in 0..o.w as i64 {
                    o.put(x, y, c.at(x / k as i64, y / k as i64));
                }
            }
            Ok(o)
        });
    }
}

fn install(lua: &Lua, printed: Rc<RefCell<Vec<String>>>) -> mlua::Result<()> {
    lua.set_app_data(Ids(Cell::new(0)));
    let g = lua.globals();
    // No clock, date, or environment: what os would give a script, it says
    // is not there, and why.
    lua.load(
        r#"os = setmetatable({}, { __index = function(_, k)
          error("os." .. tostring(k) .. " is not in an art script: it has no clock, date, or files, so it draws the same every time", 2)
        end })"#,
    )
    .set_name("=argus")
    .exec()?;
    // math from libm, the same on every platform; no math.random.
    let math: Table = g.get("math")?;
    macro_rules! one {
        ($($name:literal => $f:path),*) => {$(
            math.set($name, lua.create_function(|_, x: f64| Ok($f(x)))?)?;
        )*};
    }
    one!(
        "sin" => libm::sin, "cos" => libm::cos, "tan" => libm::tan,
        "asin" => libm::asin, "acos" => libm::acos, "exp" => libm::exp,
        "sinh" => libm::sinh, "cosh" => libm::cosh, "tanh" => libm::tanh,
        "log10" => libm::log10, "sqrt" => libm::sqrt
    );
    math.set(
        "atan",
        lua.create_function(|_, (y, x): (f64, Option<f64>)| {
            Ok(match x {
                Some(x) => libm::atan2(y, x),
                None => libm::atan(y),
            })
        })?,
    )?;
    math.set(
        "atan2",
        lua.create_function(|_, (y, x): (f64, f64)| Ok(libm::atan2(y, x)))?,
    )?;
    math.set(
        "pow",
        lua.create_function(|_, (x, y): (f64, f64)| Ok(libm::pow(x, y)))?,
    )?;
    math.set(
        "log",
        lua.create_function(|_, (x, b): (f64, Option<f64>)| {
            Ok(match b {
                Some(b) => libm::log(x) / libm::log(b),
                None => libm::log(x),
            })
        })?,
    )?;
    let no_random = lua.create_function(|_, _: Variadic<Value>| -> mlua::Result<()> {
        Err(bad("math.random would draw differently each time: use art.rng(seed), whose :int(lo, hi), :float(), :chance(p), and :pick(list) are the same every run"))
    })?;
    math.set("random", no_random.clone())?;
    math.set("randomseed", no_random)?;
    g.set(
        "print",
        lua.create_function(move |_, args: Variadic<Value>| {
            let words: Vec<String> = args
                .iter()
                .map(|v| match v {
                    Value::String(s) => s.to_string_lossy(),
                    Value::Integer(i) => i.to_string(),
                    Value::Number(n) => n.to_string(),
                    Value::Boolean(b) => b.to_string(),
                    Value::Nil => "nil".to_string(),
                    other => kind(other).trim_start_matches("a ").to_string(),
                })
                .collect();
            printed.borrow_mut().push(words.join(" "));
            Ok(())
        })?,
    )?;

    let art = lua.create_table()?;
    art.set(
        "canvas",
        lua.create_function(|lua, (w, h, fill): (u32, u32, Option<Value>)| {
            let fill = match fill {
                None | Some(Value::Nil) => [0; 4],
                Some(v) => color(&v)?,
            };
            Canvas::new(lua, w, h, fill)
        })?,
    )?;
    art.set(
        "rng",
        lua.create_function(|_, seed: Option<u64>| Ok(Rng::new(seed.unwrap_or(1))))?,
    )?;
    art.set(
        "noise",
        lua.create_function(|_, (x, y, seed): (f64, f64, Option<u64>)| {
            Ok(noise(x, y, seed.unwrap_or(1)))
        })?,
    )?;
    art.set(
        "mix",
        lua.create_function(|_, (a, b, t): (Value, Value, f64)| {
            Ok(hex(mix(color(&a)?, color(&b)?, t)))
        })?,
    )?;
    art.set(
        "lighten",
        lua.create_function(|_, (a, k): (Value, f64)| {
            Ok(hex(mix(color(&a)?, [255, 255, 255, 255], k)))
        })?,
    )?;
    art.set(
        "darken",
        lua.create_function(|_, (a, k): (Value, f64)| Ok(hex(mix(color(&a)?, [0, 0, 0, 255], k))))?,
    )?;
    art.set(
        "alpha",
        lua.create_function(|_, (a, k): (Value, f64)| {
            let mut c = color(&a)?;
            c[3] = libm::round(k.clamp(0.0, 1.0) * 255.0) as u8;
            Ok(hex(c))
        })?,
    )?;
    art.set(
        "hsv",
        lua.create_function(|_, (h, s, v, a): (f64, f64, f64, Option<f64>)| {
            let [r, g, b] = hsv(h, s, v);
            let a = libm::round(a.unwrap_or(1.0).clamp(0.0, 1.0) * 255.0) as u8;
            Ok(hex([r, g, b, a]))
        })?,
    )?;
    art.set(
        "rgb",
        lua.create_function(|_, (r, g, b, a): (u8, u8, u8, Option<u8>)| {
            Ok(hex([r, g, b, a.unwrap_or(255)]))
        })?,
    )?;
    art.set(
        "grid",
        lua.create_function(|lua, (sheet, fw, fh): (UserDataRef<Canvas>, u32, u32)| {
            if fw == 0 || fh == 0 {
                return Err(bad("grid takes a frame's width and height, 1 or more"));
            }
            let out = lua.create_table()?;
            let mut n = 1;
            for gy in 0..sheet.h / fh {
                for gx in 0..sheet.w / fw {
                    let mut o = Canvas::new(lua, fw, fh, [0; 4])?;
                    for y in 0..fh as i64 {
                        for x in 0..fw as i64 {
                            o.put(x, y, sheet.at((gx * fw) as i64 + x, (gy * fh) as i64 + y));
                        }
                    }
                    out.raw_set(n, o)?;
                    n += 1;
                }
            }
            Ok(out)
        })?,
    )?;
    // art.sprite{...} is the table itself: what the script returns.
    art.set("sprite", lua.create_function(|_, t: Table| Ok(t))?)?;
    g.set("art", art)?;
    Ok(())
}

/// The line of a Luau error in `file`, when it names one.
fn line_in(msg: &str, file: &str) -> Option<u32> {
    let at = msg.find(&format!("{file}:"))? + file.len() + 1;
    let digits: String = msg[at..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// Run an art script and read the sprite it returns. `file` is how errors
/// name it (`assets/sprites/hero.art.luau`).
pub fn run(file: &str, source: &str) -> Result<ArtSprite> {
    let fail = |m: String| {
        let line = line_in(&m, file);
        let msg = m.replace(&format!("{file}:"), "line ");
        ArgusError::new(codes::BAD_SOURCE, format!("{file}: {msg}"))
            .hint("An art script returns art.sprite{ frames = {...}, clips = {...}, pixels_per_unit = n } (argus docs files section=art).")
            .at(Some(match line {
                Some(l) => format!("{file}:{l}"),
                None => file.to_string(),
            }))
    };
    let lua = Lua::new();
    let printed = Rc::new(RefCell::new(Vec::new()));
    install(&lua, printed.clone()).map_err(|e| fail(e.to_string()))?;
    lua.set_memory_limit(MEMORY)
        .map_err(|e| fail(e.to_string()))?;
    let started = argus_os::time::Instant::now();
    lua.set_interrupt(move |_| {
        if started.elapsed().as_secs_f64() > TIME_LIMIT_S {
            Err(bad(format!(
                "the script ran past {TIME_LIMIT_S} seconds and was stopped"
            )))
        } else {
            Ok(VmState::Continue)
        }
    });
    lua.sandbox(true).map_err(|e| fail(e.to_string()))?;
    let out: Value = lua
        .load(source)
        .set_name(format!("={file}"))
        .eval()
        .map_err(|e| fail(e.to_string()))?;
    let Value::Table(t) = out else {
        return Err(fail(format!(
            "the script returned {}, not a sprite: end it with return art.sprite{{ frames = {{...}} }}",
            kind(&out)
        )));
    };
    let mut sprite = read_sprite(&t).map_err(|e| fail(e.to_string()))?;
    sprite.printed = printed.borrow().clone();
    Ok(sprite)
}

fn canvas_of(v: &Value) -> Option<AnyUserData> {
    match v {
        Value::UserData(u) if u.is::<Canvas>() => Some(u.clone()),
        _ => None,
    }
}

fn read_sprite(t: &Table) -> mlua::Result<ArtSprite> {
    for kv in t.pairs::<Value, Value>() {
        let (k, _) = kv?;
        let key = match &k {
            Value::String(s) => s.to_str()?.to_string(),
            other => format!("{other:?}"),
        };
        if !["frames", "clips", "pixels_per_unit", "pivot", "ms"].contains(&key.as_str()) {
            return Err(bad(format!(
                "the sprite has a key {key:?}; it takes frames, clips, pixels_per_unit, pivot, and ms"
            )));
        }
    }
    let ppu = t.get::<Option<f64>>("pixels_per_unit")?.unwrap_or(16.0) as f32;
    if ppu.is_nan() || ppu <= 0.0 {
        return Err(bad("pixels_per_unit is more than 0"));
    }
    let pivot = |v: Option<Table>, default: [f32; 2]| -> mlua::Result<[f32; 2]> {
        Ok(match v {
            Some(p) => [p.get::<f64>(1)? as f32, p.get::<f64>(2)? as f32],
            None => default,
        })
    };
    let base_pivot = pivot(t.get("pivot")?, [0.5, 0.5])?;
    let base_ms = t.get::<Option<u32>>("ms")?.unwrap_or(100);
    let mut frames: Vec<ArtFrame> = Vec::new();
    let mut index_of: HashMap<u64, u32> = HashMap::new();
    // A frame entry: a canvas, or { canvas, pivot = {x, y}, ms = n, properties = {...} }.
    let mut add = |v: &Value, frames: &mut Vec<ArtFrame>| -> mlua::Result<u32> {
        let (ud, extra) = match v {
            Value::UserData(_) => (canvas_of(v), None),
            Value::Table(e) => {
                let c = e.get::<Value>(1)?;
                let c = if c.is_nil() {
                    e.get::<Value>("canvas")?
                } else {
                    c
                };
                (canvas_of(&c), Some(e.clone()))
            }
            _ => (None, None),
        };
        let Some(ud) = ud else {
            return Err(bad(format!(
                "a frame is a canvas (art.canvas) or {{ canvas, pivot = {{x, y}}, ms = n }}, not {}",
                kind(v)
            )));
        };
        let c = ud.borrow::<Canvas>()?.clone();
        if let Some(&i) = index_of.get(&c.id) {
            if extra.is_none() {
                return Ok(i);
            }
        }
        let (mut p, mut ms, mut props) = (base_pivot, base_ms, BTreeMap::new());
        if let Some(e) = extra {
            p = pivot(e.get("pivot")?, base_pivot)?;
            ms = e.get::<Option<u32>>("ms")?.unwrap_or(base_ms);
            if let Some(pt) = e.get::<Option<Table>>("properties")? {
                for kv in pt.pairs::<String, Value>() {
                    let (k, v) = kv?;
                    let s = match v {
                        Value::String(s) => s.to_str()?.to_string(),
                        Value::Integer(i) => i.to_string(),
                        Value::Number(n) => n.to_string(),
                        Value::Boolean(b) => b.to_string(),
                        Value::Table(a) => {
                            let parts: Vec<String> = a
                                .sequence_values::<f64>()
                                .map(|x| x.map(|x| x.to_string()))
                                .collect::<mlua::Result<_>>()?;
                            format!("[{}]", parts.join(", "))
                        }
                        other => return Err(bad(format!("property {k} is {}", kind(&other)))),
                    };
                    props.insert(k, s);
                }
            }
        }
        let i = frames.len() as u32;
        index_of.insert(c.id, i);
        frames.push(ArtFrame {
            canvas: c,
            pivot: p,
            ms,
            properties: props,
        });
        Ok(i)
    };
    if let Some(list) = t.get::<Option<Table>>("frames")? {
        for v in list.sequence_values::<Value>() {
            add(&v?, &mut frames)?;
        }
    }
    let mut clips = BTreeMap::new();
    if let Some(ct) = t.get::<Option<Table>>("clips")? {
        let mut named: Vec<(String, Table)> = Vec::new();
        for kv in ct.pairs::<String, Table>() {
            named.push(kv?);
        }
        named.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, c) in named {
            let list: Table = match c.get::<Option<Table>>("frames")? {
                Some(l) => l,
                None => c.clone(),
            };
            let mut fs = Vec::new();
            for v in list.sequence_values::<Value>() {
                let v = v?;
                match v {
                    Value::Integer(i) if i >= 0 => fs.push(i as u32),
                    Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => fs.push(n as u32),
                    other => fs.push(add(&other, &mut frames)?),
                }
            }
            if fs.is_empty() {
                return Err(bad(format!("clip {name} has no frames")));
            }
            let direction = match c.get::<Option<String>>("direction")?.as_deref() {
                None | Some("forward") => Direction::Forward,
                Some("reverse") => Direction::Reverse,
                Some("pingpong") => Direction::PingPong,
                Some(d) => {
                    return Err(bad(format!(
                        "clip {name}: direction is forward, reverse, or pingpong, not {d:?}"
                    )))
                }
            };
            clips.insert(
                name,
                Clip {
                    frames: fs,
                    fps: c.get::<Option<f64>>("fps")?.map(|f| f as f32),
                    looping: c.get::<Option<bool>>("loop")?.unwrap_or(true),
                    direction,
                },
            );
        }
    }
    if frames.is_empty() {
        return Err(bad(
            "the sprite has no frames: give frames = { canvas, ... } or clips",
        ));
    }
    for (name, c) in &clips {
        if let Some(bad_f) = c.frames.iter().find(|&&f| f as usize >= frames.len()) {
            return Err(bad(format!(
                "clip {name} names frame {bad_f}; the sprite has {} (counted from 0)",
                frames.len()
            )));
        }
    }
    Ok(ArtSprite {
        frames,
        clips,
        pixels_per_unit: ppu,
        printed: Vec::new(),
    })
}

/// Pack frames into a sheet: rows of frames, left to right, as wide as
/// the square that would hold them all; frames with the same pixels share
/// a cell. Returns the sheet's width and height and each frame's rect.
pub fn pack(frames: &[ArtFrame]) -> (u32, u32, Vec<[u32; 4]>, Vec<u8>) {
    let mut cell_of: Vec<usize> = Vec::with_capacity(frames.len());
    let mut cells: Vec<usize> = Vec::new();
    {
        let mut seen: HashMap<(u32, u32, &[u8]), usize> = HashMap::new();
        for (i, f) in frames.iter().enumerate() {
            let c = &f.canvas;
            let k = *seen
                .entry((c.w, c.h, c.rgba.as_slice()))
                .or_insert_with(|| {
                    cells.push(i);
                    cells.len() - 1
                });
            cell_of.push(k);
        }
    }
    let area: u64 = cells
        .iter()
        .map(|&i| frames[i].canvas.w as u64 * frames[i].canvas.h as u64)
        .sum();
    let widest = cells.iter().map(|&i| frames[i].canvas.w).max().unwrap_or(1);
    let side = (libm::ceil(libm::sqrt(area as f64)) as u32).max(widest);
    let mut at: Vec<[u32; 2]> = Vec::with_capacity(cells.len());
    let (mut x, mut y, mut row_h, mut w) = (0u32, 0u32, 0u32, 0u32);
    for &i in &cells {
        let c = &frames[i].canvas;
        if x > 0 && x + c.w > side {
            x = 0;
            y += row_h;
            row_h = 0;
        }
        at.push([x, y]);
        x += c.w;
        w = w.max(x);
        row_h = row_h.max(c.h);
    }
    let h = y + row_h;
    let mut sheet = vec![0u8; (w * h * 4) as usize];
    for (k, &i) in cells.iter().enumerate() {
        let c = &frames[i].canvas;
        let [x0, y0] = at[k];
        for yy in 0..c.h {
            let from = (yy * c.w * 4) as usize;
            let to = (((y0 + yy) * w + x0) * 4) as usize;
            sheet[to..to + (c.w * 4) as usize]
                .copy_from_slice(&c.rgba[from..from + (c.w * 4) as usize]);
        }
    }
    let rects = frames
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let [x0, y0] = at[cell_of[i]];
            [x0, y0, f.canvas.w, f.canvas.h]
        })
        .collect();
    (w, h, rects, sheet)
}

impl crate::Importer {
    /// Import an art script: run it, pack its frames, and cache the sheet
    /// as a PNG's would be.
    pub(crate) fn import_art(
        &self,
        name: &str,
        g: &crate::Group,
        file: &str,
        mut hash: u64,
    ) -> Result<(crate::Entry, Vec<String>)> {
        use crate::{
            encode_png, fnv, pow2, repeat_edges, Asset, Entry, Frame, SpriteAsset, Warning,
        };
        let shown = self.shown(g.layer, file);
        if let Some(n) = &g.normal {
            let n = self.shown(g.layer, n);
            return Err(ArgusError::new(
                codes::AMBIGUOUS,
                format!("{n} is a normal map for {shown}, and a sprite an art script draws takes none yet"),
            )
            .hint("The importer lays an art script's frames out itself, so a normal map cannot match them.")
            .at(Some(n)));
        }
        let bytes = self.read_in(g.layer, file)?;
        fnv(&mut hash, &bytes);
        let source = String::from_utf8_lossy(&bytes);
        let sprite = run(&shown, &source)?;
        let mut warnings: Vec<Warning> = Vec::new();
        if let Some(s) = &g.sidecar {
            warnings.push(Warning {
                message: format!(
                    "{} is not read: an art script says its own frames, clips, and pixels per unit",
                    self.shown(g.layer, s)
                ),
                at: self.shown(g.layer, s),
            });
        }
        for p in &sprite.printed {
            warnings.push(Warning {
                message: format!("the script printed: {p}"),
                at: shown.clone(),
            });
        }
        let (w, h, rects, sheet) = pack(&sprite.frames);
        if w > MAX_SIDE || h > MAX_SIDE {
            return Err(ArgusError::new(
                codes::BAD_SOURCE,
                format!(
                    "{shown}'s frames make a sheet of {w} x {h}, past {MAX_SIDE} pixels a side"
                ),
            )
            .hint("Draw fewer or smaller frames, or split the sprite into two scripts.")
            .at(Some(shown.clone())));
        }
        let (tw, th) = (pow2(w), pow2(h));
        let mut tex = vec![0u8; (tw * th * 4) as usize];
        for y in 0..h {
            let from = (y * w * 4) as usize;
            let to = (y * tw * 4) as usize;
            tex[to..to + (w * 4) as usize].copy_from_slice(&sheet[from..from + (w * 4) as usize]);
        }
        repeat_edges(&mut tex, w, h, tw, th);
        let frames: Vec<Frame> = sprite
            .frames
            .iter()
            .zip(&rects)
            .enumerate()
            .map(|(i, (f, r))| {
                if f.canvas.rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
                    warnings.push(Warning {
                        message: format!("frame {i} is empty"),
                        at: shown.clone(),
                    });
                }
                Frame {
                    rect: *r,
                    pivot: [f.pivot[0].clamp(0.0, 1.0), f.pivot[1].clamp(0.0, 1.0)],
                    duration_ms: f.ms,
                    properties: f.properties.clone(),
                }
            })
            .collect();
        let texture = self.cache.join(format!("{name}.png"));
        encode_png(&texture, tw, th, &tex)?;
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
                    clips: sprite.clips,
                    pixels_per_unit: sprite.pixels_per_unit,
                    shape: None,
                    autotile: BTreeMap::new(),
                    aseprite: None,
                }),
                ms: 0.0,
            },
            Vec::new(),
        ))
    }
}
