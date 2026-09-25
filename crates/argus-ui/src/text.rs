//! Text (§12 "bitmap and SDF text with shaping"): measuring words and
//! breaking a text into lines of glyphs, for the layout (how big a text
//! node is) and the renderer (where each glyph goes). A font is a
//! [`FontFace`]: the built-in pixel font, or a font asset ([`SdfFont`]),
//! shaped with rustybuzz and drawn from a distance field.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// The built-in pixel font's atlas (argus-render builds it: 6 x 8 cells,
/// printable ASCII from space).
pub const PIXEL_FONT: &str = "argus/font";

/// Line height as a multiple of the text size.
pub const LINE: f32 = 1.25;

/// One glyph to draw: a frame of a font's atlas at a place.
#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    /// Left, from the line's start, in UI pixels.
    pub x: f32,
    /// Top, from the line's top.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
    /// The frame in the font's atlas.
    pub frame: u32,
}

/// A font: what the layout measures and the renderer draws.
pub trait FontFace {
    /// The atlas its glyphs are frames of.
    fn atlas(&self) -> Rc<str>;
    /// Whether the atlas holds distances (drawn with the SDF material).
    fn sdf(&self) -> bool {
        false
    }
    /// How wide `text` is at `size`, shaped.
    fn measure(&self, text: &str, size: f32) -> f32;
    /// `text`'s glyphs at `size`, shaped, from the line's top left.
    fn glyphs(&self, text: &str, size: f32) -> Vec<Glyph>;
}

/// The built-in pixel font: 6 x 8 cells (a 5 x 7 glyph and its spacing),
/// printable ASCII, scaled so a cell is `size` tall.
pub struct PixelFont {
    atlas: Rc<str>,
}

impl Default for PixelFont {
    fn default() -> Self {
        PixelFont {
            atlas: Rc::from(PIXEL_FONT),
        }
    }
}

/// The pixel font's letters past printable ASCII, in atlas order after it
/// (phase 3 item 4: the accented letters French needs, and a few other
/// languages', so a translation's words draw in the built-in font).
pub const PIXEL_EXTRA: &str = "àâäçèéêëîïôöùûüñßÀÂÇÈÉÊÎÔÙÛÑœŒ«»\u{2019}¿¡";

/// The pixel font's frame for a character (`?` for one it lacks).
pub fn pixel_frame(c: char) -> u32 {
    let c = match c {
        '×' => 'x',
        '\u{2018}' => '\'',
        _ => c,
    };
    match c {
        ' '..='~' => c as u32 - 32,
        _ => match PIXEL_EXTRA.chars().position(|x| x == c) {
            Some(i) => 95 + i as u32,
            None => '?' as u32 - 32,
        },
    }
}

impl FontFace for PixelFont {
    fn atlas(&self) -> Rc<str> {
        self.atlas.clone()
    }
    fn measure(&self, text: &str, size: f32) -> f32 {
        text.chars().count() as f32 * size * 0.75
    }
    fn glyphs(&self, text: &str, size: f32) -> Vec<Glyph> {
        let (w, h) = (size * 0.75, size);
        text.chars()
            .enumerate()
            .filter(|(_, c)| *c != ' ')
            .map(|(i, c)| Glyph {
                x: i as f32 * w,
                y: (LINE - 1.0) * 0.5 * size,
                w,
                h,
                frame: pixel_frame(c),
            })
            .collect()
    }
}

/// A glyph of a font asset's atlas (argus-assets bakes them).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SdfGlyph {
    /// Its frame in the atlas.
    pub frame: u32,
    /// The cell's left edge from the pen and its top above the baseline, in
    /// atlas pixels.
    pub offset: [f32; 2],
    /// The cell's size in atlas pixels.
    pub size: [f32; 2],
}

/// A text shaped: each glyph and where its pen is, and the whole advance,
/// in font units (so one shaping serves every size).
struct Shaped {
    /// Each glyph's id, its cell (none for a blank, like a space), and its
    /// pen.
    glyphs: Vec<(u16, Option<SdfGlyph>, f32, f32)>,
    advance: f32,
}

/// Texts kept shaped, at most (then the cache starts again).
const SHAPED_MAX: usize = 8192;

/// A font asset (§12 "SDF text with shaping"): text is shaped with
/// rustybuzz (kerning, ligatures, marks, scripts and directions, as the
/// font says) and drawn from the font's distance-field atlas with the
/// `sdf_text` material, crisp at any size. Shaped texts are kept, so a UI
/// that does not change shapes nothing.
pub struct SdfFont {
    atlas: Rc<str>,
    bytes: Arc<[u8]>,
    /// Atlas pixels per em.
    em: f32,
    /// Font units per em.
    upm: f32,
    /// Ascender and descender (negative), font units.
    ascender: f32,
    descender: f32,
    glyphs: HashMap<u16, SdfGlyph>,
    shaped: RefCell<HashMap<Box<str>, Rc<Shaped>>>,
}

impl SdfFont {
    /// A font whose atlas is `atlas`, from its bytes, the atlas's pixels
    /// per em, its metrics (units per em, ascender, descender), and its
    /// glyphs by id.
    pub fn new(
        atlas: &str,
        bytes: Arc<[u8]>,
        em: f32,
        metrics: [f32; 3],
        glyphs: HashMap<u16, SdfGlyph>,
    ) -> SdfFont {
        SdfFont {
            atlas: Rc::from(atlas),
            bytes,
            em,
            upm: metrics[0].max(1.0),
            ascender: metrics[1],
            descender: metrics[2],
            glyphs,
            shaped: RefCell::new(HashMap::new()),
        }
    }

    /// The font an imported font asset makes, its atlas named `name` (the
    /// renderer loads the asset's atlas under its name, a glyph a frame in
    /// the asset's order).
    pub fn from_asset(name: &str, f: &argus_assets::FontAsset) -> SdfFont {
        let glyphs = f
            .glyphs
            .iter()
            .enumerate()
            .map(|(i, g)| {
                (
                    g.id,
                    SdfGlyph {
                        frame: i as u32,
                        offset: g.offset,
                        size: [g.rect[2] as f32, g.rect[3] as f32],
                    },
                )
            })
            .collect();
        SdfFont::new(
            name,
            f.bytes.clone(),
            f.size,
            [
                f.units_per_em as f32,
                f.metrics[0] as f32,
                f.metrics[1] as f32,
            ],
            glyphs,
        )
    }

    fn shape(&self, text: &str) -> Rc<Shaped> {
        if let Some(s) = self.shaped.borrow().get(text) {
            return s.clone();
        }
        let s = Rc::new(match rustybuzz::Face::from_slice(&self.bytes, 0) {
            Some(face) => {
                let mut buf = rustybuzz::UnicodeBuffer::new();
                buf.push_str(text);
                buf.guess_segment_properties();
                let out = rustybuzz::shape(&face, &[], buf);
                let mut x = 0.0f32;
                let mut glyphs = Vec::with_capacity(out.len());
                for (i, p) in out.glyph_infos().iter().zip(out.glyph_positions()) {
                    let id = i.glyph_id as u16;
                    // One the atlas lacks draws the font's missing-glyph
                    // box, unless it has no outline to draw.
                    let cell = self.glyphs.get(&id).copied().or_else(|| {
                        face.glyph_bounding_box(rustybuzz::ttf_parser::GlyphId(id))
                            .and_then(|_| self.glyphs.get(&0).copied())
                    });
                    glyphs.push((id, cell, x + p.x_offset as f32, p.y_offset as f32));
                    x += p.x_advance as f32;
                }
                Shaped { glyphs, advance: x }
            }
            None => Shaped {
                glyphs: Vec::new(),
                advance: 0.0,
            },
        });
        let mut cache = self.shaped.borrow_mut();
        if cache.len() >= SHAPED_MAX {
            cache.clear();
        }
        cache.insert(Box::from(text), s.clone());
        s
    }

    /// The glyph ids `text` shapes to (what a test checks shaping with).
    pub fn glyph_ids(&self, text: &str) -> Vec<u16> {
        self.shape(text).glyphs.iter().map(|g| g.0).collect()
    }
}

impl FontFace for SdfFont {
    fn atlas(&self) -> Rc<str> {
        self.atlas.clone()
    }
    fn sdf(&self) -> bool {
        true
    }
    fn measure(&self, text: &str, size: f32) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        self.shape(text).advance * size / self.upm
    }
    fn glyphs(&self, text: &str, size: f32) -> Vec<Glyph> {
        let k = size / self.upm;
        let a = size / self.em;
        // The font's ascender to descender, centred in the line.
        let baseline =
            (LINE * size - (self.ascender - self.descender) * k) * 0.5 + self.ascender * k;
        let shaped = self.shape(text);
        let mut out = Vec::with_capacity(shaped.glyphs.len());
        for &(_, cell, x, y) in &shaped.glyphs {
            let Some(g) = cell else {
                continue;
            };
            out.push(Glyph {
                x: x * k + g.offset[0] * a,
                y: baseline - y * k - g.offset[1] * a,
                w: g.size[0] * a,
                h: g.size[1] * a,
                frame: g.frame,
            });
        }
        out
    }
}

/// A line of a laid-out text.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// Its words.
    pub text: String,
    /// Its width in UI pixels.
    pub width: f32,
}

/// A text broken into lines.
#[derive(Clone, Debug, PartialEq)]
pub struct Lines {
    /// The lines, top to bottom.
    pub lines: Vec<Line>,
    /// The widest line.
    pub width: f32,
    /// All the lines' height.
    pub height: f32,
    /// One line's height.
    pub line_height: f32,
}

/// A paragraph's width as breaking measures it: its words and the spaces
/// between them, each shaped alone. A line measured whole may kern across
/// a space and come out a little narrower; measured like this, a text laid
/// out at its own width never wraps when it is drawn.
fn para_width(font: &dyn FontFace, para: &str, size: f32, space: f32) -> f32 {
    let mut w = 0.0f32;
    for (i, word) in para.split(' ').enumerate() {
        if i > 0 {
            w += space;
        }
        w += font.measure(word, size);
    }
    w
}

/// Break `text` into lines no wider than `max` (a newline always breaks;
/// words wrap at spaces; a word wider than a line is cut), measured in
/// `font` at `size`. `None`: one line per paragraph.
pub fn lines(font: &dyn FontFace, text: &str, size: f32, max: Option<f32>) -> Lines {
    let space = font.measure(" ", size);
    let mut out = Vec::new();
    for para in text.split('\n') {
        let Some(max) = max else {
            out.push(Line {
                text: para.to_string(),
                width: para_width(font, para, size, space),
            });
            continue;
        };
        let mut cur = String::new();
        let mut cur_w = 0.0f32;
        for word in para.split(' ') {
            let w = font.measure(word, size);
            if !cur.is_empty() && cur_w + space + w > max + 0.01 {
                out.push(Line {
                    text: std::mem::take(&mut cur),
                    width: cur_w,
                });
                cur_w = 0.0;
            }
            if w > max + 0.01 {
                // Cut a word wider than a line: as much as fits, repeatedly.
                let mut rest: Vec<char> = word.chars().collect();
                while !rest.is_empty() {
                    let mut n = rest.len();
                    while n > 1
                        && font.measure(&rest[..n].iter().collect::<String>(), size) > max + 0.01
                    {
                        n -= 1;
                    }
                    let piece: String = rest[..n].iter().collect();
                    let pw = font.measure(&piece, size);
                    rest.drain(..n);
                    if rest.is_empty() {
                        cur = piece;
                        cur_w = pw;
                    } else {
                        out.push(Line {
                            text: piece,
                            width: pw,
                        });
                    }
                }
                continue;
            }
            if cur.is_empty() {
                cur = word.to_string();
                cur_w = w;
            } else {
                cur.push(' ');
                cur.push_str(word);
                cur_w += space + w;
            }
        }
        out.push(Line {
            text: cur,
            width: cur_w,
        });
    }
    let line_height = size * LINE;
    Lines {
        width: out.iter().map(|l| l.width).fold(0.0, f32::max),
        height: out.len() as f32 * line_height,
        line_height,
        lines: out,
    }
}

/// How big `text` is broken into lines as [`lines`] breaks it, without
/// making the lines: what layout measures with, many times a tick.
pub fn size_of(font: &dyn FontFace, text: &str, size: f32, max: Option<f32>) -> (f32, f32) {
    let space = font.measure(" ", size);
    let Some(max) = max else {
        let mut w = 0.0f32;
        let mut n = 0;
        for para in text.split('\n') {
            w = w.max(para_width(font, para, size, space));
            n += 1;
        }
        return (w, n as f32 * size * LINE);
    };
    let mut widest = 0.0f32;
    let mut n = 0usize;
    for para in text.split('\n') {
        // As `lines` keeps them: the line's width and whether it has any
        // characters yet (empty words before the first are dropped).
        let mut cur = 0.0f32;
        let mut chars = 0usize;
        for word in para.split(' ') {
            let w = font.measure(word, size);
            if chars > 0 && cur + space + w > max + 0.01 {
                widest = widest.max(cur);
                n += 1;
                cur = 0.0;
                chars = 0;
            }
            if w > max + 0.01 {
                // A word wider than a line: the long way, rarely.
                let l = lines(font, text, size, Some(max));
                return (l.width, l.height);
            }
            if chars == 0 {
                cur = w;
                chars = word.len();
            } else {
                cur += space + w;
                chars += 1 + word.len();
            }
        }
        widest = widest.max(cur);
        n += 1;
    }
    (widest, n as f32 * size * LINE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_breaks_at_spaces_and_newlines_and_cuts_a_long_word() {
        let f = PixelFont::default();
        // At size 8 a character is 6 wide.
        assert_eq!(f.measure("abc", 8.0), 18.0);
        let l = lines(&f, "one two three", 8.0, Some(48.0));
        let t: Vec<&str> = l.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(t, ["one two", "three"]);
        assert_eq!(l.width, 42.0);
        assert_eq!(l.height, 20.0);
        let l = lines(&f, "head\nabcdefghij", 8.0, Some(24.0));
        let t: Vec<&str> = l.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(t, ["head", "abcd", "efgh", "ij"]);
        let l = lines(&f, "no wrap here", 8.0, None);
        assert_eq!(l.lines.len(), 1);
        assert_eq!(pixel_frame('A'), 33);
        // Past ASCII: the letters a translation needs (phase 3 item 4), and
        // `?` for anything else.
        let e_acute = PIXEL_EXTRA.chars().position(|c| c == 'é').unwrap() as u32;
        assert_eq!(pixel_frame('é'), 95 + e_acute);
        assert_eq!(pixel_frame('\u{1F600}'), pixel_frame('?'));
        assert_eq!(f.glyphs("a b", 16.0).len(), 2, "spaces draw nothing");
    }

    #[test]
    fn measuring_agrees_with_breaking() {
        let f = PixelFont::default();
        for text in [
            "one two three",
            "head\nabcdefghij",
            "  leading and  double  spaces ",
            "",
            "a\n\nb",
            "exactly fits",
        ] {
            for max in [
                None,
                Some(0.0),
                Some(24.0),
                Some(48.0),
                Some(72.0),
                Some(1000.0),
            ] {
                let l = lines(&f, text, 8.0, max);
                assert_eq!(
                    size_of(&f, text, 8.0, max),
                    (l.width, l.height),
                    "{text:?} at {max:?}"
                );
            }
        }
    }
}
