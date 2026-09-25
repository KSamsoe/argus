//! L5 annotation (§8): entity ids and bounds drawn over a captured frame,
//! so what the agent sees in the picture can be matched to what it can
//! query. Drawn on the CPU into the captured image with a built-in 5x7
//! font; nothing here touches the GPU.

use crate::semantic::SemanticFrame;
use crate::Image;

/// Printable ASCII from ' ' to '~', 5 columns by 7 rows each.
#[rustfmt::skip]
const GLYPHS: [[&str; 7]; 95] = [
    [".....", ".....", ".....", ".....", ".....", ".....", "....."], // ' '
    ["..#..", "..#..", "..#..", "..#..", "..#..", ".....", "..#.."], // !
    [".#.#.", ".#.#.", ".....", ".....", ".....", ".....", "....."], // "
    [".#.#.", ".#.#.", "#####", ".#.#.", "#####", ".#.#.", ".#.#."], // #
    ["..#..", ".####", "#.#..", ".###.", "..#.#", "####.", "..#.."], // $
    ["##...", "##..#", "...#.", "..#..", ".#...", "#..##", "...##"], // %
    [".##..", "#..#.", "#.#..", ".#...", "#.#.#", "#..#.", ".##.#"], // &
    ["..#..", "..#..", ".....", ".....", ".....", ".....", "....."], // '
    ["...#.", "..#..", ".#...", ".#...", ".#...", "..#..", "...#."], // (
    [".#...", "..#..", "...#.", "...#.", "...#.", "..#..", ".#..."], // )
    [".....", "..#..", "#.#.#", ".###.", "#.#.#", "..#..", "....."], // *
    [".....", "..#..", "..#..", "#####", "..#..", "..#..", "....."], // +
    [".....", ".....", ".....", ".....", ".##..", "..#..", ".#..."], // ,
    [".....", ".....", ".....", "#####", ".....", ".....", "....."], // -
    [".....", ".....", ".....", ".....", ".....", ".##..", ".##.."], // .
    [".....", "....#", "...#.", "..#..", ".#...", "#....", "....."], // /
    [".###.", "#...#", "#..##", "#.#.#", "##..#", "#...#", ".###."], // 0
    ["..#..", ".##..", "..#..", "..#..", "..#..", "..#..", ".###."], // 1
    [".###.", "#...#", "....#", "...#.", "..#..", ".#...", "#####"], // 2
    ["#####", "...#.", "..#..", "...#.", "....#", "#...#", ".###."], // 3
    ["...#.", "..##.", ".#.#.", "#..#.", "#####", "...#.", "...#."], // 4
    ["#####", "#....", "####.", "....#", "....#", "#...#", ".###."], // 5
    ["..##.", ".#...", "#....", "####.", "#...#", "#...#", ".###."], // 6
    ["#####", "....#", "...#.", "..#..", ".#...", ".#...", ".#..."], // 7
    [".###.", "#...#", "#...#", ".###.", "#...#", "#...#", ".###."], // 8
    [".###.", "#...#", "#...#", ".####", "....#", "...#.", ".##.."], // 9
    [".....", ".##..", ".##..", ".....", ".##..", ".##..", "....."], // :
    [".....", ".##..", ".##..", ".....", ".##..", "..#..", ".#..."], // ;
    ["...#.", "..#..", ".#...", "#....", ".#...", "..#..", "...#."], // <
    [".....", ".....", "#####", ".....", "#####", ".....", "....."], // =
    [".#...", "..#..", "...#.", "....#", "...#.", "..#..", ".#..."], // >
    [".###.", "#...#", "....#", "...#.", "..#..", ".....", "..#.."], // ?
    [".###.", "#...#", "....#", ".##.#", "#.#.#", "#.#.#", ".###."], // @
    [".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"], // A
    ["####.", "#...#", "#...#", "####.", "#...#", "#...#", "####."], // B
    [".###.", "#...#", "#....", "#....", "#....", "#...#", ".###."], // C
    ["###..", "#..#.", "#...#", "#...#", "#...#", "#..#.", "###.."], // D
    ["#####", "#....", "#....", "####.", "#....", "#....", "#####"], // E
    ["#####", "#....", "#....", "####.", "#....", "#....", "#...."], // F
    [".###.", "#...#", "#....", "#.###", "#...#", "#...#", ".####"], // G
    ["#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"], // H
    [".###.", "..#..", "..#..", "..#..", "..#..", "..#..", ".###."], // I
    ["..###", "...#.", "...#.", "...#.", "...#.", "#..#.", ".##.."], // J
    ["#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#"], // K
    ["#....", "#....", "#....", "#....", "#....", "#....", "#####"], // L
    ["#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#"], // M
    ["#...#", "#...#", "##..#", "#.#.#", "#..##", "#...#", "#...#"], // N
    [".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."], // O
    ["####.", "#...#", "#...#", "####.", "#....", "#....", "#...."], // P
    [".###.", "#...#", "#...#", "#...#", "#.#.#", "#..#.", ".##.#"], // Q
    ["####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#"], // R
    [".####", "#....", "#....", ".###.", "....#", "....#", "####."], // S
    ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#.."], // T
    ["#...#", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."], // U
    ["#...#", "#...#", "#...#", "#...#", "#...#", ".#.#.", "..#.."], // V
    ["#...#", "#...#", "#...#", "#.#.#", "#.#.#", "#.#.#", ".#.#."], // W
    ["#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#"], // X
    ["#...#", "#...#", ".#.#.", "..#..", "..#..", "..#..", "..#.."], // Y
    ["#####", "....#", "...#.", "..#..", ".#...", "#....", "#####"], // Z
    [".###.", ".#...", ".#...", ".#...", ".#...", ".#...", ".###."], // [
    [".....", "#....", ".#...", "..#..", "...#.", "....#", "....."], // backslash
    [".###.", "...#.", "...#.", "...#.", "...#.", "...#.", ".###."], // ]
    ["..#..", ".#.#.", "#...#", ".....", ".....", ".....", "....."], // ^
    [".....", ".....", ".....", ".....", ".....", ".....", "#####"], // _
    [".#...", "..#..", ".....", ".....", ".....", ".....", "....."], // `
    [".....", ".....", ".###.", "....#", ".####", "#...#", ".####"], // a
    ["#....", "#....", "#.##.", "##..#", "#...#", "#...#", "####."], // b
    [".....", ".....", ".###.", "#....", "#....", "#...#", ".###."], // c
    ["....#", "....#", ".##.#", "#..##", "#...#", "#...#", ".####"], // d
    [".....", ".....", ".###.", "#...#", "#####", "#....", ".###."], // e
    ["..##.", ".#..#", ".#...", "###..", ".#...", ".#...", ".#..."], // f
    [".....", ".####", "#...#", "#...#", ".####", "....#", ".###."], // g
    ["#....", "#....", "#.##.", "##..#", "#...#", "#...#", "#...#"], // h
    ["..#..", ".....", ".##..", "..#..", "..#..", "..#..", ".###."], // i
    ["...#.", ".....", "..##.", "...#.", "...#.", "#..#.", ".##.."], // j
    ["#....", "#....", "#..#.", "#.#..", "##...", "#.#..", "#..#."], // k
    [".##..", "..#..", "..#..", "..#..", "..#..", "..#..", ".###."], // l
    [".....", ".....", "##.#.", "#.#.#", "#.#.#", "#...#", "#...#"], // m
    [".....", ".....", "#.##.", "##..#", "#...#", "#...#", "#...#"], // n
    [".....", ".....", ".###.", "#...#", "#...#", "#...#", ".###."], // o
    [".....", ".....", "####.", "#...#", "####.", "#....", "#...."], // p
    [".....", ".....", ".##.#", "#..##", ".####", "....#", "....#"], // q
    [".....", ".....", "#.##.", "##..#", "#....", "#....", "#...."], // r
    [".....", ".....", ".###.", "#....", ".###.", "....#", "####."], // s
    [".#...", ".#...", "###..", ".#...", ".#...", ".#..#", "..##."], // t
    [".....", ".....", "#...#", "#...#", "#...#", "#..##", ".##.#"], // u
    [".....", ".....", "#...#", "#...#", "#...#", ".#.#.", "..#.."], // v
    [".....", ".....", "#...#", "#...#", "#.#.#", "#.#.#", ".#.#."], // w
    [".....", ".....", "#...#", ".#.#.", "..#..", ".#.#.", "#...#"], // x
    [".....", ".....", "#...#", "#...#", ".####", "....#", ".###."], // y
    [".....", ".....", "#####", "...#.", "..#..", ".#...", "#####"], // z
    ["...#.", "..#..", "..#..", ".#...", "..#..", "..#..", "...#."], // {
    ["..#..", "..#..", "..#..", "..#..", "..#..", "..#..", "..#.."], // |
    [".#...", "..#..", "..#..", "...#.", "..#..", "..#..", ".#..."], // }
    [".....", ".....", ".#...", "#.#.#", "...#.", ".....", "....."], // ~
];

/// A glyph's rows for a character; anything outside printable ASCII is `?`
/// (and `×` is `x`).
fn glyph(c: char) -> &'static [&'static str; 7] {
    let c = if c == '×' { 'x' } else { c };
    let i = c as u32;
    if (32..127).contains(&i) {
        &GLYPHS[(i - 32) as usize]
    } else {
        &GLYPHS[('?' as u32 - 32) as usize]
    }
}

/// Most lines a bar wraps to.
pub const BAR_LINES: usize = 4;

/// A bar `width` pixels wide with text on it, for the bottom of the window
/// (phase 2 item 2: the note prompt after F8; item 4: the proposal being
/// tried). Scale 2 on a window wider than 900 pixels. The text wraps at
/// spaces (and at a newline) onto up to `BAR_LINES` lines, and the bar
/// grows to fit them. A longer text keeps its first line and its end, with
/// "..." between: the first line says what the bar is, and the end is where
/// a note is being typed. (One line cut to fit hid a long proposal's keys
/// in a small window, found in the phase 2 gate.)
pub fn status_bar(width: u32, text: &str, rgba: [u8; 4]) -> Image {
    let scale = if width > 900 { 2 } else { 1 };
    let pad = 4 * scale;
    let fits = ((width.saturating_sub(2 * pad)) / (6 * scale)).max(4) as usize;
    let mut lines = wrap(text, fits);
    if lines.len() > BAR_LINES {
        let tail = lines.split_off(lines.len() - (BAR_LINES - 1));
        lines.truncate(1);
        let first = &mut lines[0];
        while first.chars().count() + 4 > fits && first.pop().is_some() {}
        first.push_str(" ...");
        lines.extend(tail);
    }
    let pitch = 10 * scale;
    let height = 8 * scale + 2 * pad + (lines.len() as u32 - 1) * pitch;
    let mut img = Image {
        width,
        height,
        rgba: vec![0; (width * height * 4) as usize],
    };
    img.fill_rect(0, 0, width as i64, height as i64, [18, 16, 26, 255]);
    img.fill_rect(0, 0, width as i64, scale as i64, rgba);
    for (i, line) in lines.iter().enumerate() {
        let y = pad + scale + i as u32 * pitch;
        img.text(pad as i64, y as i64, line, scale, rgba);
    }
    for px in img.rgba.chunks_mut(4) {
        px[3] = 235;
    }
    img
}

/// `text` in lines of at most `fits` characters: a newline always breaks,
/// and a line too long breaks at its last space that fits (a word longer
/// than a line is cut).
pub fn wrap(text: &str, fits: usize) -> Vec<String> {
    let fits = fits.max(1);
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut rest: Vec<char> = para.chars().collect();
        while rest.len() > fits {
            let cut = rest[..=fits]
                .iter()
                .rposition(|c| *c == ' ')
                .filter(|&p| p > 0)
                .unwrap_or(fits);
            let line: String = rest[..cut].iter().collect();
            lines.push(line.trim_end().to_string());
            let spaces = rest[cut..].iter().take_while(|c| **c == ' ').count();
            rest.drain(..cut + spaces);
        }
        lines.push(rest.into_iter().collect());
    }
    lines
}

/// The atlas the game's own text is drawn from (`Describe.shown`): every
/// printable ASCII character in a 6 by 8 cell, white on clear, sixteen to
/// a row. Returns width, height, RGBA, and each character's frame in
/// texture space, space (32) first.
pub(crate) fn font_atlas() -> (u32, u32, Vec<u8>, Vec<[f32; 4]>) {
    let (cols, rows) = (16u32, 6u32);
    let (w, h) = (cols * 6, rows * 8);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let mut frames = Vec::with_capacity(95);
    for i in 0..95u32 {
        let (cx, cy) = ((i % cols) * 6, (i / cols) * 8);
        for (row, bits) in GLYPHS[i as usize].iter().enumerate() {
            for (col, b) in bits.bytes().enumerate() {
                if b == b'#' {
                    let p = (((cy + row as u32) * w + cx + col as u32) * 4) as usize;
                    rgba[p..p + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
        frames.push([
            cx as f32 / w as f32,
            cy as f32 / h as f32,
            6.0 / w as f32,
            8.0 / h as f32,
        ]);
    }
    (w, h, rgba, frames)
}

/// A character's frame in [`font_atlas`]; anything outside printable
/// ASCII is `?`.
pub(crate) fn font_frame(c: char) -> usize {
    let c = if c == '×' { 'x' } else { c };
    let i = c as u32;
    if (32..127).contains(&i) {
        (i - 32) as usize
    } else {
        ('?' as u32 - 32) as usize
    }
}

/// Pixels a line of text takes at `scale`: 6 across a character, 8 down.
pub fn text_size(s: &str, scale: u32) -> (u32, u32) {
    (s.chars().count() as u32 * 6 * scale, 8 * scale)
}

impl Image {
    fn blend(&mut self, x: i64, y: i64, rgba: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return;
        }
        let i = ((y as u32 * self.width + x as u32) * 4) as usize;
        let a = rgba[3] as u32;
        for (k, &c) in rgba.iter().take(3).enumerate() {
            let d = self.rgba[i + k] as u32;
            self.rgba[i + k] = ((c as u32 * a + d * (255 - a)) / 255) as u8;
        }
        self.rgba[i + 3] = 255;
    }

    /// Fill a rectangle, blended by the colour's alpha.
    pub fn fill_rect(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, rgba: [u8; 4]) {
        for y in y0.max(0)..y1.min(self.height as i64) {
            for x in x0.max(0)..x1.min(self.width as i64) {
                self.blend(x, y, rgba);
            }
        }
    }

    /// A one-pixel outline of a rectangle, with a dark shadow outside it so
    /// it reads on any background.
    pub fn outline(&mut self, b: [i32; 4], rgba: [u8; 4]) {
        let [x0, y0, x1, y1] = b.map(|v| v as i64);
        let (x1, y1) = (x1.max(x0 + 1), y1.max(y0 + 1));
        let shadow = [0, 0, 0, 160];
        for (c, d) in [(shadow, 1i64), (rgba, 0)] {
            for x in x0 - d..x1 + d {
                self.blend(x, y0 - d, c);
                self.blend(x, y1 - 1 + d, c);
            }
            for y in y0 - d..y1 + d {
                self.blend(x0 - d, y, c);
                self.blend(x1 - 1 + d, y, c);
            }
        }
    }

    /// Text at a pixel (its top-left), on a dark plate.
    pub fn text(&mut self, x: i64, y: i64, s: &str, scale: u32, rgba: [u8; 4]) {
        let (w, h) = text_size(s, scale);
        self.fill_rect(
            x - scale as i64,
            y - scale as i64,
            x + w as i64,
            y + h as i64,
            [0, 0, 0, 190],
        );
        let sc = scale as i64;
        for (n, c) in s.chars().enumerate() {
            let g = glyph(c);
            let gx = x + n as i64 * 6 * sc;
            for (row, bits) in g.iter().enumerate() {
                for (col, b) in bits.bytes().enumerate() {
                    if b == b'#' {
                        let px = gx + col as i64 * sc;
                        let py = y + row as i64 * sc;
                        self.fill_rect(px, py, px + sc, py + sc, rgba);
                    }
                }
            }
        }
    }
}

/// One box and label drawn on the picture.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    /// The label: an entity's id, or a group's name and count.
    pub label: String,
    /// Pixel bounds: x0, y0, x1, y1.
    pub bounds: [i32; 4],
    /// Whether the label fitted (it is left off where it would cover another).
    pub labelled: bool,
}

/// Draw the frame's entities and groups over the image: yellow boxes with
/// their ids for entities, cyan boxes with name and count for groups.
/// Labels go above a box, else below it, else nowhere, so none covers
/// another.
pub fn annotate(img: &mut Image, frame: &SemanticFrame) -> Vec<Mark> {
    let scale = if img.width >= 1280 { 2 } else { 1 };
    let mut marks = Vec::new();
    let mut placed: Vec<[i64; 4]> = Vec::new();
    let yellow = [255, 220, 40, 255];
    let cyan = [60, 220, 255, 255];
    let mut items: Vec<(String, [i32; 4], [u8; 4])> = Vec::new();
    for g in &frame.groups {
        items.push((format!("{} x{}", g.what, g.count), g.bounds, cyan));
    }
    for s in &frame.entities {
        items.push((s.id.clone(), s.bounds, yellow));
    }
    for (label, b, colour) in items {
        img.outline(b, colour);
        let (w, h) = text_size(&label, scale);
        let (w, h) = (w as i64, h as i64);
        let x = (b[0] as i64).clamp(0, (img.width as i64 - w).max(0));
        let spots = [b[1] as i64 - h - 2, b[3] as i64 + 2];
        let mut labelled = false;
        for y in spots {
            if y < 0 || y + h > img.height as i64 {
                continue;
            }
            let r = [x - 1, y - 1, x + w, y + h];
            if placed
                .iter()
                .any(|p| p[0] < r[2] && r[0] < p[2] && p[1] < r[3] && r[1] < p[3])
            {
                continue;
            }
            img.text(x, y, &label, scale, colour);
            placed.push(r);
            labelled = true;
            break;
        }
        marks.push(Mark {
            label,
            bounds: b,
            labelled,
        });
    }
    marks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_bar_wraps_and_grows_and_keeps_its_first_line_and_its_end() {
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap("head\nbody  text", 20), ["head", "body  text"]);
        assert_eq!(wrap("", 5), [""]);
        // One line: the bar is as it was.
        assert_eq!(status_bar(480, "F8", [255; 4]).height, 16);
        // 480 pixels at scale 1 fit 78 characters: 100 wrap to two lines.
        let text = format!("{} {}", "a".repeat(60), "b".repeat(39));
        assert_eq!(status_bar(480, &text, [255; 4]).height, 16 + 10);
        // Too long: four lines, the first and the last three.
        let words: Vec<String> = (0..60).map(|i| format!("w{i:02}")).collect();
        let lines = wrap(&words.join(" "), 20);
        assert!(lines.len() > BAR_LINES);
        let img = status_bar(20 * 6 + 8, &words.join(" "), [255; 4]);
        assert_eq!(img.height, 16 + 3 * 10);
    }
}
