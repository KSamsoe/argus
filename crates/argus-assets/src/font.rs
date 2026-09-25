//! Fonts (§12 "bitmap and SDF text with shaping"; phase 3 item 1): a
//! `.ttf` or `.otf` under `assets/` imports as a signed distance field
//! atlas of its glyphs, which the UI draws crisp at any size, and keeps the
//! font's bytes, which the UI shapes text with (kerning, ligatures, marks).
//!
//! The glyphs baked are the ones a character of the set maps to (printable
//! ASCII, Latin-1, and common punctuation, plus the sidecar's `chars`), and
//! every glyph the font's substitutions can make of those (a ligature whose
//! parts are all baked, an alternate form), so shaped text finds its
//! glyphs. The sidecar, `<name>.toml` beside the font, may set `size` (the
//! em in atlas pixels, default 32), `spread` (how far from an edge the
//! field reaches, in atlas pixels, default 4), and `chars`.

use crate::sidecar::{err, line_of, parse};
use argus_ecs::{ArgusError, Result};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use ttf_parser::{Face, GlyphId};

/// The characters every font bakes: printable ASCII, Latin-1, and the
/// punctuation UI text uses.
pub const DEFAULT_CHARS: &str = "‘’‚“”„–—…•·€™°±×÷←↑→↓≤≥≠";

/// One glyph in the atlas.
#[derive(Clone, Debug, PartialEq)]
pub struct BakedGlyph {
    /// The font's glyph id.
    pub id: u16,
    /// Its cell in the atlas: x, y, w, h in pixels (the glyph and `spread`
    /// around it).
    pub rect: [u32; 4],
    /// The cell's left edge from the pen, and its top above the baseline,
    /// in atlas pixels.
    pub offset: [f32; 2],
}

/// An imported font.
#[derive(Clone, Debug, PartialEq)]
pub struct FontAsset {
    /// The font file, relative to `assets/`.
    pub source: String,
    /// Its bytes (the UI shapes with them).
    pub bytes: Arc<[u8]>,
    /// The family name, when the font has one.
    pub family: String,
    /// The em in atlas pixels.
    pub size: f32,
    /// How far the field reaches from an edge, in atlas pixels: a texel
    /// holds 0.5 on the edge, 1 at `spread` inside, 0 at `spread` outside.
    pub spread: f32,
    /// Font units per em.
    pub units_per_em: u16,
    /// Ascender, descender (negative), and line gap, in font units.
    pub metrics: [i16; 3],
    /// Atlas size in pixels.
    pub atlas: [u32; 2],
    /// The field, one byte a texel, row by row from the top.
    pub field: Arc<[u8]>,
    /// The glyphs baked, by id.
    pub glyphs: Vec<BakedGlyph>,
    /// Characters of the set the font has no glyph for.
    pub missing: Vec<char>,
}

impl FontAsset {
    /// The atlas as RGBA: white, with the field in alpha (what the GPU
    /// takes; alpha is linear in an sRGB texture).
    pub fn rgba(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.field.len() * 4);
        for &a in self.field.iter() {
            out.extend_from_slice(&[255, 255, 255, a]);
        }
        out
    }
}

/// What the sidecar sets.
struct Options {
    size: f32,
    spread: f32,
    chars: String,
}

fn options(file: &str, text: Option<&str>, warnings: &mut Vec<(String, u32)>) -> Result<Options> {
    let mut o = Options {
        size: 32.0,
        spread: 4.0,
        chars: String::new(),
    };
    let Some(text) = text else {
        return Ok(o);
    };
    let doc = parse(file, text)?;
    for (k, item) in doc.iter() {
        let line = line_of(text, item);
        let num = || {
            item.as_value()
                .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
        };
        match k {
            "type" => {
                if item.as_str() != Some("font") {
                    return Err(err(file, line, "type beside a font file is \"font\""));
                }
            }
            "size" => match num() {
                Some(n) if (8.0..=256.0).contains(&n) => o.size = n as f32,
                _ => return Err(err(file, line, "size is the em in atlas pixels, 8 to 256")),
            },
            "spread" => match num() {
                Some(n) if (1.0..=32.0).contains(&n) => o.spread = n as f32,
                _ => return Err(err(file, line, "spread is in atlas pixels, 1 to 32")),
            },
            "chars" => match item.as_str() {
                Some(s) => o.chars = s.to_string(),
                None => return Err(err(file, line, "chars is a string of characters to bake")),
            },
            other => warnings.push((
                format!("{other} is not a font setting (size, spread, chars); ignored"),
                line,
            )),
        }
    }
    Ok(o)
}

/// A font baked: the hash of its inputs, the font, and its warnings.
type Baked = (u64, FontAsset, Vec<(String, u32)>);

/// Fonts baked in this process, by the hash of what they were baked from
/// (the font, its sidecar, the importer's version): a test run that loads
/// the project again, or a load, does not bake them again. At most a few.
static BAKED: Mutex<Vec<Baked>> = Mutex::new(Vec::new());
const BAKED_MAX: usize = 8;

/// Import a font file: bake the atlas, or take the one this process baked
/// from the same inputs (`hash`). `warnings` gets (message, line of the
/// sidecar).
pub fn import(
    source: &str,
    bytes: Vec<u8>,
    side: Option<(&str, &str)>,
    hash: u64,
    warnings: &mut Vec<(String, u32)>,
) -> Result<FontAsset> {
    if let Some((_, f, w)) = BAKED.lock().ok().and_then(|b| {
        b.iter()
            .find(|(h, f, _)| *h == hash && f.source == source)
            .cloned()
    }) {
        warnings.extend(w);
        return Ok(f);
    }
    let mut notes = Vec::new();
    let f = bake_file(source, bytes, side, &mut notes)?;
    if let Ok(mut b) = BAKED.lock() {
        if b.len() >= BAKED_MAX {
            b.remove(0);
        }
        b.push((hash, f.clone(), notes.clone()));
    }
    warnings.extend(notes);
    Ok(f)
}

fn bake_file(
    source: &str,
    bytes: Vec<u8>,
    side: Option<(&str, &str)>,
    warnings: &mut Vec<(String, u32)>,
) -> Result<FontAsset> {
    let (side_file, side_text) = match side {
        Some((f, t)) => (f, Some(t)),
        None => ("", None),
    };
    let o = options(side_file, side_text, warnings)?;
    let face = Face::parse(&bytes, 0).map_err(|e| {
        ArgusError::new(
            crate::codes::BAD_SOURCE,
            format!("assets/{source} is not a font this reads: {e}"),
        )
        .hint("TrueType (.ttf) and OpenType (.otf) fonts import; a collection (.ttc) does not.")
        .at(Some(format!("assets/{source}")))
    })?;
    let set: String = (' '..='~')
        .chain('\u{a0}'..='\u{ff}')
        .chain(DEFAULT_CHARS.chars())
        .chain(o.chars.chars())
        .collect();
    let mut ids: BTreeSet<u16> = BTreeSet::from([0]);
    let mut missing = Vec::new();
    for c in set.chars() {
        match face.glyph_index(c) {
            Some(g) => {
                ids.insert(g.0);
            }
            None if !missing.contains(&c) => missing.push(c),
            None => {}
        }
    }
    close_over_substitutions(&face, &mut ids);
    let family = face
        .names()
        .into_iter()
        .find(|n| n.name_id == ttf_parser::name_id::FAMILY && n.is_unicode())
        .and_then(|n| n.to_string())
        .unwrap_or_default();
    let upm = face.units_per_em();
    let (atlas, field, glyphs) = bake(&face, &ids, o.size, o.spread);
    Ok(FontAsset {
        source: source.to_string(),
        family,
        size: o.size,
        spread: o.spread,
        units_per_em: upm,
        metrics: [face.ascender(), face.descender(), face.line_gap()],
        atlas,
        field: field.into(),
        glyphs,
        missing,
        bytes: bytes.into(),
    })
}

/// Add every glyph the font's substitutions make from glyphs in `ids`: a
/// single or alternate substitute, a multiple's parts, a ligature whose
/// components are all in, until nothing more comes.
fn close_over_substitutions(face: &Face, ids: &mut BTreeSet<u16>) {
    use ttf_parser::gsub::{SingleSubstitution, SubstitutionSubtable as S};
    let Some(gsub) = face.tables().gsub else {
        return;
    };
    for _ in 0..8 {
        let before = ids.len();
        let have: Vec<u16> = ids.iter().copied().collect();
        for lookup in gsub.lookups {
            for sub in lookup.subtables.into_iter::<S>() {
                for &g in &have {
                    let g = GlyphId(g);
                    match &sub {
                        S::Single(SingleSubstitution::Format1 { coverage, delta }) => {
                            if coverage.contains(g) {
                                ids.insert((g.0 as i32 + *delta as i32) as u16);
                            }
                        }
                        S::Single(SingleSubstitution::Format2 {
                            coverage,
                            substitutes,
                        }) => {
                            if let Some(s) = coverage.get(g).and_then(|i| substitutes.get(i)) {
                                ids.insert(s.0);
                            }
                        }
                        S::Multiple(m) => {
                            if let Some(seq) = m.coverage.get(g).and_then(|i| m.sequences.get(i)) {
                                ids.extend(seq.substitutes.into_iter().map(|s| s.0));
                            }
                        }
                        S::Alternate(a) => {
                            if let Some(set) =
                                a.coverage.get(g).and_then(|i| a.alternate_sets.get(i))
                            {
                                ids.extend(set.alternates.into_iter().map(|s| s.0));
                            }
                        }
                        S::Ligature(l) => {
                            if let Some(set) =
                                l.coverage.get(g).and_then(|i| l.ligature_sets.get(i))
                            {
                                for lig in set.into_iter() {
                                    if lig
                                        .components
                                        .into_iter()
                                        .all(|c| have.binary_search(&c.0).is_ok())
                                    {
                                        ids.insert(lig.glyph.0);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        if ids.len() == before {
            break;
        }
    }
    let n = face.number_of_glyphs();
    ids.retain(|g| *g < n);
}

/// A glyph's outline as line segments, in atlas pixels (y up, the pen at
/// the origin).
struct Segments {
    scale: f32,
    segs: Vec<[f32; 4]>,
    start: (f32, f32),
    at: (f32, f32),
}

impl Segments {
    fn line(&mut self, x: f32, y: f32) {
        let (x, y) = (x * self.scale, y * self.scale);
        if (x, y) != self.at {
            self.segs.push([self.at.0, self.at.1, x, y]);
        }
        self.at = (x, y);
    }
}

impl ttf_parser::OutlineBuilder for Segments {
    fn move_to(&mut self, x: f32, y: f32) {
        self.at = (x * self.scale, y * self.scale);
        self.start = self.at;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.line(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (x0, y0) = (self.at.0 / self.scale, self.at.1 / self.scale);
        const N: usize = 8;
        for i in 1..=N {
            let t = i as f32 / N as f32;
            let u = 1.0 - t;
            self.line(
                u * u * x0 + 2.0 * u * t * x1 + t * t * x,
                u * u * y0 + 2.0 * u * t * y1 + t * t * y,
            );
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (x0, y0) = (self.at.0 / self.scale, self.at.1 / self.scale);
        const N: usize = 12;
        for i in 1..=N {
            let t = i as f32 / N as f32;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.line(
                a * x0 + b * x1 + c * x2 + d * x,
                a * y0 + b * y1 + c * y2 + d * y,
            );
        }
    }
    fn close(&mut self) {
        if self.at != self.start {
            self.segs
                .push([self.at.0, self.at.1, self.start.0, self.start.1]);
        }
        self.at = self.start;
    }
}

/// The signed distance from `p` to an outline, in the outline's units:
/// positive inside (non-zero winding), negative outside.
fn signed_distance(segs: &[[f32; 4]], px: f32, py: f32) -> f32 {
    let mut best = f32::MAX;
    let mut winding = 0i32;
    for s in segs {
        let (ax, ay, bx, by) = (s[0], s[1], s[2], s[3]);
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 {
            (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (cx, cy) = (ax + t * dx - px, ay + t * dy - py);
        best = best.min(cx * cx + cy * cy);
        // Crossings of the ray to the right.
        if (ay <= py) != (by <= py) {
            let x = ax + (py - ay) / (by - ay) * dx;
            if x > px {
                winding += if by > ay { 1 } else { -1 };
            }
        }
    }
    let d = best.sqrt();
    if winding != 0 {
        d
    } else {
        -d
    }
}

/// Bake the glyphs: each one's field in a cell with `spread` around it,
/// packed on shelves, tallest first.
fn bake(
    face: &Face,
    ids: &BTreeSet<u16>,
    size: f32,
    spread: f32,
) -> ([u32; 2], Vec<u8>, Vec<BakedGlyph>) {
    let scale = size / face.units_per_em() as f32;
    let pad = spread.ceil() as i32 + 1;
    struct Cell {
        id: u16,
        segs: Vec<[f32; 4]>,
        x0: i32,
        y1: i32,
        w: u32,
        h: u32,
    }
    let mut cells: Vec<Cell> = Vec::new();
    for &id in ids {
        let mut b = Segments {
            scale,
            segs: Vec::new(),
            start: (0.0, 0.0),
            at: (0.0, 0.0),
        };
        // A glyph with no outline (a space) takes no room.
        let Some(r) = face.outline_glyph(GlyphId(id), &mut b) else {
            continue;
        };
        if b.segs.is_empty() {
            continue;
        }
        let x0 = (r.x_min as f32 * scale).floor() as i32 - pad;
        let x1 = (r.x_max as f32 * scale).ceil() as i32 + pad;
        let y0 = (r.y_min as f32 * scale).floor() as i32 - pad;
        let y1 = (r.y_max as f32 * scale).ceil() as i32 + pad;
        cells.push(Cell {
            id,
            segs: b.segs,
            x0,
            y1,
            w: (x1 - x0) as u32,
            h: (y1 - y0) as u32,
        });
    }
    // Shelves in an atlas as wide as makes it about square.
    let area: u32 = cells.iter().map(|c| c.w * c.h).sum();
    let width = ((area as f32).sqrt() * 1.15).ceil().max(64.0) as u32;
    let width = width.next_multiple_of(4);
    let mut order: Vec<usize> = (0..cells.len()).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(cells[i].h), cells[i].id));
    let mut places = vec![[0u32; 2]; cells.len()];
    let (mut x, mut y, mut shelf) = (0u32, 0u32, 0u32);
    for &i in &order {
        let c = &cells[i];
        if x + c.w > width {
            x = 0;
            y += shelf;
            shelf = 0;
        }
        places[i] = [x, y];
        x += c.w;
        shelf = shelf.max(c.h);
    }
    let height = (y + shelf).max(1).next_multiple_of(4);
    let mut field = vec![0u8; (width * height) as usize];
    let mut glyphs = Vec::with_capacity(cells.len());
    for (i, c) in cells.iter().enumerate() {
        let [ox, oy] = places[i];
        for row in 0..c.h {
            // Texel centres, y up from the baseline.
            let py = c.y1 as f32 - row as f32 - 0.5;
            for col in 0..c.w {
                let px = c.x0 as f32 + col as f32 + 0.5;
                let d = signed_distance(&c.segs, px, py);
                let v = (0.5 + d / (2.0 * spread)).clamp(0.0, 1.0);
                field[((oy + row) * width + ox + col) as usize] = (v * 255.0).round() as u8;
            }
        }
        glyphs.push(BakedGlyph {
            id: c.id,
            rect: [ox, oy, c.w, c.h],
            offset: [c.x0 as f32, c.y1 as f32],
        });
    }
    glyphs.sort_by_key(|g| g.id);
    ([width, height], field, glyphs)
}
