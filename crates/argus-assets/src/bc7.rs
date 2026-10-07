//! BC7 texture compression (section 15: "compressed textures in atlases";
//! phase 5 item 1), for the sprite pages a release pack carries.
//!
//! The encoder writes two of BC7's eight modes, the two with one subset,
//! which need none of the format's partition tables: mode 6 (colour and
//! alpha on one line, 16 steps) and mode 5 (colour and alpha on lines of
//! their own, 4 steps each), whichever is closer for the block. The decoder
//! reads those two. Whether a sprite ships compressed is [`looks_same`]'s
//! to say, so a block the two modes cannot hold (pixel art often has three
//! colours in four by four pixels) keeps its sprite lossless.

/// Mode 6's interpolation weights (16 steps), out of 64.
const W4: [u32; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];
/// Mode 5's (4 steps).
const W2: [u32; 4] = [0, 21, 43, 64];

type Block = [[u8; 4]; 16];

fn mix(a: u8, b: u8, w: u32) -> u8 {
    (((64 - w) * a as u32 + w * b as u32 + 32) >> 6) as u8
}

/// How far a decoded pixel is from the authored one, for choosing between
/// encodings: alpha counts double, and colour by how much of it shows. An
/// alpha of 0 or 255 must stay so ([`looks_same`]), so missing it costs
/// more than any colour.
fn pixel_error(p: &[u8; 4], q: &[u8; 4]) -> f32 {
    let show = (p[3] as f32 / 255.0).max(0.02);
    let mut e = 0.0;
    for c in 0..3 {
        let d = p[c] as f32 - q[c] as f32;
        e += d * d * show;
    }
    let d = p[3] as f32 - q[3] as f32;
    let exact = matches!(p[3], 0 | 255) && p[3] != q[3];
    e + 2.0 * d * d + if exact { 1e7 } else { 0.0 }
}

// ----- bits -------------------------------------------------------------------

struct Bits {
    v: u128,
    at: u32,
}

impl Bits {
    fn put(&mut self, x: u32, n: u32) {
        self.v |= ((x as u128) & ((1u128 << n) - 1)) << self.at;
        self.at += n;
    }
    fn get(&mut self, n: u32) -> u32 {
        let x = ((self.v >> self.at) & ((1u128 << n) - 1)) as u32;
        self.at += n;
        x
    }
}

// ----- fitting a line -----------------------------------------------------------

/// The ends of the line through `points` (`dims` channels used), weighted:
/// the mean and the principal axis, out to the farthest projections.
fn line<const D: usize>(points: &[[f32; D]], weights: &[f32]) -> ([f32; D], [f32; D]) {
    let total: f32 = weights.iter().sum::<f32>().max(1e-6);
    let mut mean = [0.0f32; D];
    for (p, w) in points.iter().zip(weights) {
        for c in 0..D {
            mean[c] += p[c] * w / total;
        }
    }
    let mut cov = [[0.0f32; D]; D];
    for (p, w) in points.iter().zip(weights) {
        for i in 0..D {
            for j in 0..D {
                cov[i][j] += (p[i] - mean[i]) * (p[j] - mean[j]) * w;
            }
        }
    }
    // The principal axis by power iteration, from the widest channel.
    let mut axis = [0.0f32; D];
    let widest = (0..D)
        .max_by(|&a, &b| cov[a][a].total_cmp(&cov[b][b]))
        .unwrap_or(0);
    axis[widest] = 1.0;
    for _ in 0..12 {
        let mut next = [0.0f32; D];
        for i in 0..D {
            for j in 0..D {
                next[i] += cov[i][j] * axis[j];
            }
        }
        let len = next.iter().map(|x| x * x).sum::<f32>().sqrt();
        if len < 1e-9 {
            break;
        }
        for i in 0..D {
            axis[i] = next[i] / len;
        }
    }
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for (p, w) in points.iter().zip(weights) {
        if *w <= 0.0 {
            continue;
        }
        let t: f32 = (0..D).map(|c| (p[c] - mean[c]) * axis[c]).sum();
        lo = lo.min(t);
        hi = hi.max(t);
    }
    if lo > hi {
        (lo, hi) = (0.0, 0.0);
    }
    let end = |t: f32| {
        let mut e = [0.0f32; D];
        for c in 0..D {
            e[c] = (mean[c] + axis[c] * t).clamp(0.0, 255.0);
        }
        e
    };
    (end(lo), end(hi))
}

/// Endpoints that best fit `points` for these `steps` (weights out of 64)
/// and indices, by least squares; `None` when every index is the same.
fn refit<const D: usize>(
    points: &[[f32; D]],
    weights: &[f32],
    idx: &[usize],
    steps: &[u32],
) -> Option<([f32; D], [f32; D])> {
    let (mut aa, mut ab, mut bb) = (0.0f32, 0.0f32, 0.0f32);
    let mut ax = [0.0f32; D];
    let mut bx = [0.0f32; D];
    for ((p, w), &i) in points.iter().zip(weights).zip(idx) {
        let t = steps[i] as f32 / 64.0;
        let (a, b) = (1.0 - t, t);
        aa += a * a * w;
        ab += a * b * w;
        bb += b * b * w;
        for c in 0..D {
            ax[c] += a * p[c] * w;
            bx[c] += b * p[c] * w;
        }
    }
    let det = aa * bb - ab * ab;
    if det.abs() < 1e-6 {
        return None;
    }
    let mut e0 = [0.0f32; D];
    let mut e1 = [0.0f32; D];
    for c in 0..D {
        e0[c] = ((bb * ax[c] - ab * bx[c]) / det).clamp(0.0, 255.0);
        e1[c] = ((aa * bx[c] - ab * ax[c]) / det).clamp(0.0, 255.0);
    }
    Some((e0, e1))
}

// ----- mode 6 -------------------------------------------------------------------

/// Mode 6: RGBA endpoints of 7 bits and a low bit each, 4-bit indices.
struct Mode6 {
    e: [[u8; 4]; 2],
    idx: [usize; 16],
}

impl Mode6 {
    fn colours(&self) -> [[u8; 4]; 16] {
        let mut out = [[0u8; 4]; 16];
        for (s, o) in out.iter_mut().enumerate() {
            for (c, v) in o.iter_mut().enumerate() {
                *v = mix(self.e[0][c], self.e[1][c], W4[s]);
            }
        }
        out
    }

    /// Endpoints from real values, with the low bits (p) that fit best,
    /// and every pixel at its nearest step.
    fn fit(px: &Block, ends: ([f32; 4], [f32; 4])) -> (Mode6, f32) {
        let mut best: Option<(Mode6, f32)> = None;
        for p0 in 0..2u8 {
            for p1 in 0..2u8 {
                let q = |v: f32, p: u8| {
                    let x = ((v - p as f32) / 2.0).round().clamp(0.0, 127.0) as u8;
                    (x << 1) | p
                };
                let e = [ends.0.map(|v| q(v, p0)), ends.1.map(|v| q(v, p1))];
                let mut m = Mode6 { e, idx: [0; 16] };
                let pal = m.colours();
                let mut err = 0.0;
                for (i, p) in px.iter().enumerate() {
                    let (s, d) = pal
                        .iter()
                        .enumerate()
                        .map(|(s, c)| (s, pixel_error(p, c)))
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .unwrap();
                    m.idx[i] = s;
                    err += d;
                }
                if best.as_ref().is_none_or(|b| err < b.1) {
                    best = Some((m, err));
                }
            }
        }
        best.unwrap()
    }

    fn encode(px: &Block) -> (Mode6, f32) {
        let points: Vec<[f32; 4]> = px.iter().map(|p| p.map(|c| c as f32)).collect();
        let weights = vec![1.0f32; 16];
        let (mut m, mut err) = Mode6::fit(px, line(&points, &weights));
        if let Some(ends) = refit(&points, &weights, &m.idx, &W4) {
            let (m2, e2) = Mode6::fit(px, ends);
            if e2 < err {
                (m, err) = (m2, e2);
            }
        }
        (m, err)
    }

    fn pack(mut self) -> [u8; 16] {
        // The first pixel's index has a top bit of 0: swap the ends if not.
        if self.idx[0] >= 8 {
            self.e.swap(0, 1);
            for i in &mut self.idx {
                *i = 15 - *i;
            }
        }
        let mut b = Bits { v: 0, at: 0 };
        b.put(1 << 6, 7);
        for c in 0..4 {
            b.put((self.e[0][c] >> 1) as u32, 7);
            b.put((self.e[1][c] >> 1) as u32, 7);
        }
        b.put((self.e[0][0] & 1) as u32, 1);
        b.put((self.e[1][0] & 1) as u32, 1);
        for (i, &x) in self.idx.iter().enumerate() {
            b.put(x as u32, if i == 0 { 3 } else { 4 });
        }
        b.v.to_le_bytes()
    }
}

// ----- mode 5 -------------------------------------------------------------------

/// Mode 5: colour endpoints of 7 bits and alpha of 8, 2-bit indices each.
struct Mode5 {
    c: [[u8; 3]; 2],
    a: [u8; 2],
    ci: [usize; 16],
    ai: [usize; 16],
}

fn widen7(x: u8) -> u8 {
    (x << 1) | (x >> 6)
}

impl Mode5 {
    fn colours(&self) -> ([[u8; 3]; 4], [u8; 4]) {
        let (e0, e1) = (self.c[0].map(widen7), self.c[1].map(widen7));
        let mut c = [[0u8; 3]; 4];
        let mut a = [0u8; 4];
        for s in 0..4 {
            for k in 0..3 {
                c[s][k] = mix(e0[k], e1[k], W2[s]);
            }
            a[s] = mix(self.a[0], self.a[1], W2[s]);
        }
        (c, a)
    }

    fn encode(px: &Block) -> (Mode5, f32) {
        let points: Vec<[f32; 3]> = px
            .iter()
            .map(|p| [p[0], p[1], p[2]].map(|c| c as f32))
            .collect();
        // Colour that does not show does not steer the line.
        let opaque = px.iter().any(|p| p[3] > 0);
        let weights: Vec<f32> = px
            .iter()
            .map(|p| if opaque { p[3] as f32 / 255.0 } else { 1.0 })
            .collect();
        let alpha: Vec<f32> = px.iter().map(|p| p[3] as f32).collect();
        let (amin, amax) = alpha
            .iter()
            .fold((255.0f32, 0.0f32), |(lo, hi), &a| (lo.min(a), hi.max(a)));
        let fit = |ends: ([f32; 3], [f32; 3])| {
            let q = |v: f32| (v / 255.0 * 127.0).round().clamp(0.0, 127.0) as u8;
            let mut m = Mode5 {
                c: [ends.0.map(q), ends.1.map(q)],
                a: [amin as u8, amax as u8],
                ci: [0; 16],
                ai: [0; 16],
            };
            let (pal, apal) = m.colours();
            let mut err = 0.0;
            for (i, p) in px.iter().enumerate() {
                let (s, _) = apal
                    .iter()
                    .enumerate()
                    .map(|(s, &a)| (s, (a as f32 - p[3] as f32).abs()))
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .unwrap();
                m.ai[i] = s;
                let (t, _) = pal
                    .iter()
                    .enumerate()
                    .map(|(t, c)| (t, pixel_error(p, &[c[0], c[1], c[2], p[3]])))
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .unwrap();
                m.ci[i] = t;
                let q = [pal[t][0], pal[t][1], pal[t][2], apal[s]];
                err += pixel_error(p, &q);
            }
            (m, err)
        };
        let (mut m, mut err) = fit(line(&points, &weights));
        if let Some(ends) = refit(&points, &weights, &m.ci, &W2) {
            let (m2, e2) = fit(ends);
            if e2 < err {
                (m, err) = (m2, e2);
            }
        }
        (m, err)
    }

    fn pack(mut self) -> [u8; 16] {
        if self.ci[0] >= 2 {
            self.c.swap(0, 1);
            for i in &mut self.ci {
                *i = 3 - *i;
            }
        }
        if self.ai[0] >= 2 {
            self.a.swap(0, 1);
            for i in &mut self.ai {
                *i = 3 - *i;
            }
        }
        let mut b = Bits { v: 0, at: 0 };
        b.put(1 << 5, 6);
        b.put(0, 2); // no rotation
        for k in 0..3 {
            b.put(self.c[0][k] as u32, 7);
            b.put(self.c[1][k] as u32, 7);
        }
        b.put(self.a[0] as u32, 8);
        b.put(self.a[1] as u32, 8);
        for (i, &x) in self.ci.iter().enumerate() {
            b.put(x as u32, if i == 0 { 1 } else { 2 });
        }
        for (i, &x) in self.ai.iter().enumerate() {
            b.put(x as u32, if i == 0 { 1 } else { 2 });
        }
        b.v.to_le_bytes()
    }
}

// ----- blocks and images ------------------------------------------------------------

fn encode_block(px: &Block) -> [u8; 16] {
    let (m6, e6) = Mode6::encode(px);
    let (m5, e5) = Mode5::encode(px);
    if e5 < e6 {
        m5.pack()
    } else {
        m6.pack()
    }
}

/// One block's pixels; `None` for a mode this decoder does not read.
fn decode_block(block: &[u8; 16]) -> Option<Block> {
    let mut b = Bits {
        v: u128::from_le_bytes(*block),
        at: 0,
    };
    let mode = block[0].trailing_zeros();
    let mut out = [[0u8; 4]; 16];
    match mode {
        6 => {
            b.at = 7;
            // Each channel's two ends in turn, then each end's low bit.
            let (mut e0, mut e1) = ([0u8; 4], [0u8; 4]);
            for (x, y) in e0.iter_mut().zip(&mut e1) {
                *x = (b.get(7) << 1) as u8;
                *y = (b.get(7) << 1) as u8;
            }
            let (p0, p1) = (b.get(1) as u8, b.get(1) as u8);
            for (x, y) in e0.iter_mut().zip(&mut e1) {
                *x |= p0;
                *y |= p1;
            }
            for (i, o) in out.iter_mut().enumerate() {
                let w = W4[b.get(if i == 0 { 3 } else { 4 }) as usize];
                for (c, v) in o.iter_mut().enumerate() {
                    *v = mix(e0[c], e1[c], w);
                }
            }
        }
        5 => {
            b.at = 6;
            let rotation = b.get(2);
            let (mut c0, mut c1) = ([0u8; 3], [0u8; 3]);
            for (x, y) in c0.iter_mut().zip(&mut c1) {
                *x = widen7(b.get(7) as u8);
                *y = widen7(b.get(7) as u8);
            }
            let a = [b.get(8) as u8, b.get(8) as u8];
            let ci: Vec<u32> = (0..16).map(|i| b.get(if i == 0 { 1 } else { 2 })).collect();
            let ai: Vec<u32> = (0..16).map(|i| b.get(if i == 0 { 1 } else { 2 })).collect();
            for (i, o) in out.iter_mut().enumerate() {
                for k in 0..3 {
                    o[k] = mix(c0[k], c1[k], W2[ci[i] as usize]);
                }
                o[3] = mix(a[0], a[1], W2[ai[i] as usize]);
                if rotation > 0 {
                    o.swap(3, rotation as usize - 1);
                }
            }
        }
        _ => return None,
    }
    Some(out)
}

fn read_block(w: usize, rgba: &[u8], bx: usize, by: usize) -> Block {
    let mut px = [[0u8; 4]; 16];
    for y in 0..4 {
        for x in 0..4 {
            let i = ((by * 4 + y) * w + bx * 4 + x) * 4;
            px[y * 4 + x].copy_from_slice(&rgba[i..i + 4]);
        }
    }
    px
}

/// RGBA8 as BC7 blocks, row by row. `width` and `height` are multiples of 4.
pub fn encode(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    debug_assert!(w % 4 == 0 && h % 4 == 0 && rgba.len() == w * h * 4);
    let mut out = Vec::with_capacity(w * h);
    for by in 0..h / 4 {
        for bx in 0..w / 4 {
            out.extend(encode_block(&read_block(w, rgba, bx, by)));
        }
    }
    out
}

/// BC7 blocks (modes 5 and 6, as [`encode`] writes them) as RGBA8; `None`
/// when a block is in another mode or the data is short.
pub fn decode(width: u32, height: u32, data: &[u8]) -> Option<Vec<u8>> {
    let (w, h) = (width as usize, height as usize);
    if w % 4 != 0 || h % 4 != 0 || data.len() < w * h {
        return None;
    }
    let mut out = vec![0u8; w * h * 4];
    let (blocks, _) = data.as_chunks::<16>();
    for by in 0..h / 4 {
        for bx in 0..w / 4 {
            let px = decode_block(&blocks[by * (w / 4) + bx])?;
            for y in 0..4 {
                for x in 0..4 {
                    let i = ((by * 4 + y) * w + bx * 4 + x) * 4;
                    out[i..i + 4].copy_from_slice(&px[y * 4 + x]);
                }
            }
        }
    }
    Some(out)
}

/// The largest colour difference a person does not see: pixelmatch's YIQ
/// distance at its threshold of 0.1, the image goldens' (`35215 × 0.1²`).
const UNSEEN: f32 = 352.15;

fn yiq(p: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = p;
    [
        r * 0.298_895_3 + g * 0.586_622_5 + b * 0.114_482_23,
        r * 0.595_977_99 - g * 0.274_176_1 - b * 0.321_801_9,
        r * 0.211_470_17 - g * 0.522_617_1 + b * 0.311_146_94,
    ]
}

fn over(p: &[u8; 4], bg: f32) -> [f32; 3] {
    let a = p[3] as f32 / 255.0;
    [0, 1, 2].map(|c| p[c] as f32 * a + bg * (1.0 - a))
}

/// Whether `b` shows as `a` to a person, pixel for pixel: alpha that is 0
/// or 255 stays so and any other within 8, and the colour, over black and
/// over white, within the distance a person does not see.
pub fn looks_same(a: &[u8], b: &[u8]) -> bool {
    let (pa, _) = a.as_chunks::<4>();
    let (pb, _) = b.as_chunks::<4>();
    pa.len() == pb.len()
        && pa.iter().zip(pb).all(|(p, q)| {
            let alpha = match p[3] {
                0 | 255 => p[3] == q[3],
                x => x.abs_diff(q[3]) <= 8,
            };
            alpha
                && [0.0f32, 255.0].iter().all(|&bg| {
                    let (y1, y2) = (yiq(over(p, bg)), yiq(over(q, bg)));
                    let d = [y1[0] - y2[0], y1[1] - y2[1], y1[2] - y2[2]];
                    0.5053 * d[0] * d[0] + 0.299 * d[1] * d[1] + 0.1957 * d[2] * d[2] <= UNSEEN
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(w: usize, h: usize, f: impl Fn(usize, usize) -> [u8; 4]) -> Vec<u8> {
        let mut out = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                out.extend(f(x, y));
            }
        }
        out
    }

    #[test]
    fn smooth_art_compresses_invisibly() {
        // A soft gradient with a faded edge: what BC7 is for.
        let src = image(32, 32, |x, y| {
            [
                (x * 8) as u8,
                (y * 8) as u8,
                ((x + y) * 4) as u8,
                if x < 28 {
                    255
                } else {
                    (255 - (x - 27) * 50) as u8
                },
            ]
        });
        let bc = encode(32, 32, &src);
        assert_eq!(bc.len(), 32 * 32);
        let back = decode(32, 32, &bc).unwrap();
        assert!(looks_same(&src, &back));
    }

    #[test]
    fn two_colours_and_clear_are_exact() {
        // Pixel art: an outline, a fill, and nothing around them.
        let src = image(8, 8, |x, y| match (x, y) {
            (0 | 7, _) | (_, 0 | 7) => [0, 0, 0, 0],
            (1 | 6, _) | (_, 1 | 6) => [40, 24, 60, 255],
            _ => [220, 180, 90, 255],
        });
        let back = decode(8, 8, &encode(8, 8, &src)).unwrap();
        assert!(looks_same(&src, &back));
        for (p, q) in src.chunks(4).zip(back.chunks(4)) {
            assert_eq!(p[3], q[3], "alpha is exact");
        }
    }

    #[test]
    fn three_colours_in_a_block_are_seen_and_refused() {
        // Red, green, and blue in one block: no line holds them.
        let src = image(4, 4, |x, _| match x % 3 {
            0 => [255, 0, 0, 255],
            1 => [0, 255, 0, 255],
            _ => [0, 0, 255, 255],
        });
        let back = decode(4, 4, &encode(4, 4, &src)).unwrap();
        assert!(!looks_same(&src, &back));
    }
}
