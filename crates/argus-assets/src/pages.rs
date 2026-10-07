//! Sprite pages (section 15: "A release pack is built from the import cache
//! ... compressed textures in atlases"; phase 5 item 1). A release pack's
//! sprites are packed into shared textures, a page a few hundred sprites
//! can share, so a frame binds one texture where it bound one a sprite. A
//! sprite goes on a BC7 page when its compressed pixels look the same as
//! its own to a person ([`crate::bc7::looks_same`]), and on a lossless page
//! otherwise; pixel art, whose four-by-four blocks often hold three colours,
//! mostly stays lossless, which is the point of asking.
//!
//! Each sprite sits in a cell of its own: its pixels, its edge pixels
//! repeated twice around them (so a page samples at a sprite's edge as the
//! sprite's own texture did), rounded out to BC7's four-pixel blocks, so a
//! block never holds two sprites. The catalog's sprites name their page and
//! their frames' places on it; nothing else in a release build knows.

use crate::{bc7, codes, decode_png, png_bytes, PackedSprites};
use argus_ecs::{ArgusError, Result};
use serde_json::{json, Value as Json};
use std::collections::BTreeMap;

/// Where a pack keeps its pages: `argus.cache/pages/<n>.bc7` or `<n>.png`.
pub const PAGES: &str = "argus.cache/pages";
/// A page's widest and tallest: the least a GPU allows (WebGL2's).
pub const MAX: u32 = 2048;
/// A sprite's edge pixels repeated around it.
const GUTTER: u32 = 2;
/// A BC7 page's file: the tag, its width and height, and its blocks.
const BC7_TAG: &[u8; 4] = b"BC7P";

/// The pages a pack carries in place of one texture a sprite.
pub struct Paged {
    /// The catalog, its sprites on their pages.
    pub catalog: Json,
    /// The pages, and the textures of sprites too big for one, by path.
    pub files: Vec<(String, Vec<u8>)>,
    /// What was made: sprites, pages, how many compressed, bytes.
    pub report: Json,
}

struct Cell {
    sprite: usize,
    w: u32,
    h: u32,
    rgba: Vec<u8>,
    bc7: bool,
}

impl Cell {
    fn cw(&self) -> u32 {
        (self.w + 2 * GUTTER).next_multiple_of(4)
    }
    fn ch(&self) -> u32 {
        (self.h + 2 * GUTTER).next_multiple_of(4)
    }
}

/// A BC7 page's bytes.
pub fn bc7_file(width: u32, height: u32, blocks: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + blocks.len());
    out.extend(BC7_TAG);
    out.extend(width.to_le_bytes());
    out.extend(height.to_le_bytes());
    out.extend(blocks);
    out
}

/// A BC7 page's width, height, and blocks; `None` if it is not one.
pub fn read_bc7(bytes: &[u8]) -> Option<(u32, u32, &[u8])> {
    let rest = bytes.strip_prefix(BC7_TAG)?;
    let w = u32::from_le_bytes(rest.get(0..4)?.try_into().ok()?);
    let h = u32::from_le_bytes(rest.get(4..8)?.try_into().ok()?);
    let blocks = rest.get(8..)?;
    (w % 4 == 0 && h % 4 == 0 && blocks.len() == (w * h) as usize).then_some((w, h, blocks))
}

/// Pack `sprites` onto pages.
pub fn page(sprites: &PackedSprites) -> Result<Paged> {
    let disk: BTreeMap<&str, &std::path::Path> = sprites
        .textures
        .iter()
        .map(|(rel, p)| (rel.as_str(), p.as_path()))
        .collect();
    let mut catalog = sprites.catalog.clone();
    let list = catalog["sprites"]
        .as_array_mut()
        .cloned()
        .unwrap_or_default();
    let mut files = Vec::new();
    let mut cells = Vec::new();
    let mut alone = 0usize;
    let mut before = 0u64;
    for (i, s) in list.iter().enumerate() {
        let rel = s["texture"].as_str().unwrap_or_default();
        let Some(path) = disk.get(rel) else {
            continue;
        };
        before += argus_os::fs::read(path)
            .map(|b| b.len() as u64)
            .unwrap_or(0);
        let (tw, th, rgba) = decode_png(path)?;
        // The part the sprite uses: its size, and every frame.
        let (mut w, mut h) = (
            s["size"][0].as_u64().unwrap_or(0) as u32,
            s["size"][1].as_u64().unwrap_or(0) as u32,
        );
        for f in s["frames"].as_array().into_iter().flatten() {
            let r = |k: usize| f["rect"][k].as_u64().unwrap_or(0) as u32;
            w = w.max(r(0) + r(2));
            h = h.max(r(1) + r(3));
        }
        let (w, h) = (w.clamp(1, tw.max(1)), h.clamp(1, th.max(1)));
        let mut cell = Cell {
            sprite: i,
            w,
            h,
            rgba: Vec::new(),
            bc7: false,
        };
        if cell.cw() > MAX || cell.ch() > MAX {
            // Too big for a page: its texture as it is.
            let bytes = argus_os::fs::read(path)
                .map_err(|e| ArgusError::new(codes::CACHE, format!("{rel}: {e}")))?;
            files.push((rel.to_string(), bytes));
            alone += 1;
            continue;
        }
        // The cell: the sprite, its edges repeated outward.
        let (cw, ch) = (cell.cw(), cell.ch());
        let mut px = vec![0u8; (cw * ch * 4) as usize];
        for y in 0..ch {
            for x in 0..cw {
                let sx = x.saturating_sub(GUTTER).min(w - 1);
                let sy = y.saturating_sub(GUTTER).min(h - 1);
                let from = ((sy * tw + sx) * 4) as usize;
                let to = ((y * cw + x) * 4) as usize;
                px[to..to + 4].copy_from_slice(&rgba[from..from + 4]);
            }
        }
        let back = bc7::decode(cw, ch, &bc7::encode(cw, ch, &px)).unwrap_or_default();
        let inside = |img: &[u8]| -> Vec<u8> {
            (GUTTER..GUTTER + h)
                .flat_map(|y| {
                    let a = ((y * cw + GUTTER) * 4) as usize;
                    img[a..a + (w * 4) as usize].to_vec()
                })
                .collect()
        };
        cell.bc7 = back.len() == px.len() && bc7::looks_same(&inside(&px), &inside(&back));
        cell.rgba = px;
        cells.push(cell);
    }

    // Shelves, tallest first, BC7 cells and lossless ones apart.
    let mut report_pages = Vec::new();
    let mut n = 0usize;
    for compressed in [true, false] {
        let mut mine: Vec<&Cell> = cells.iter().filter(|c| c.bc7 == compressed).collect();
        if mine.is_empty() {
            continue;
        }
        mine.sort_by(|a, b| (b.ch(), b.cw(), a.sprite).cmp(&(a.ch(), a.cw(), b.sprite)));
        let area: u64 = mine.iter().map(|c| (c.cw() * c.ch()) as u64).sum();
        let widest = mine.iter().map(|c| c.cw()).max().unwrap_or(4);
        let width = ((area as f64).sqrt().ceil() as u32)
            .next_power_of_two()
            .max(widest)
            .min(MAX);
        // Where each cell goes: (page, x, y).
        let mut placed: Vec<(usize, u32, u32)> = Vec::new();
        let mut sizes: Vec<(u32, u32)> = Vec::new();
        let (mut x, mut y, mut shelf) = (0u32, 0u32, 0u32);
        for c in &mine {
            if x + c.cw() > width {
                (x, y, shelf) = (0, y + shelf, 0);
            }
            if sizes.is_empty() || y + c.ch() > MAX {
                sizes.push((0, 0));
                (x, y, shelf) = (0, 0, 0);
            }
            let page = sizes.len() - 1;
            placed.push((page, x, y));
            let s = &mut sizes[page];
            s.0 = s.0.max(x + c.cw());
            s.1 = s.1.max(y + c.ch());
            x += c.cw();
            shelf = shelf.max(c.ch());
        }
        for (p, &(pw, ph)) in sizes.iter().enumerate() {
            let (pw, ph) = (pw.next_multiple_of(4), ph.next_multiple_of(4));
            let mut img = vec![0u8; (pw * ph * 4) as usize];
            let ext = if compressed { "bc7" } else { "png" };
            let rel = format!("{PAGES}/{n}.{ext}");
            let mut count = 0;
            for (c, &(cp, cx, cy)) in mine.iter().zip(&placed) {
                if cp != p {
                    continue;
                }
                count += 1;
                let cw = c.cw();
                for row in 0..c.ch() {
                    let from = (row * cw * 4) as usize;
                    let to = (((cy + row) * pw + cx) * 4) as usize;
                    img[to..to + (cw * 4) as usize]
                        .copy_from_slice(&c.rgba[from..from + (cw * 4) as usize]);
                }
                let s = &mut catalog["sprites"][c.sprite];
                s["texture"] = json!(rel);
                s["texture_size"] = json!([pw, ph]);
                for f in s["frames"].as_array_mut().into_iter().flatten() {
                    let r = &mut f["rect"];
                    r[0] = json!(r[0].as_u64().unwrap_or(0) as u32 + cx + GUTTER);
                    r[1] = json!(r[1].as_u64().unwrap_or(0) as u32 + cy + GUTTER);
                }
            }
            let bytes = if compressed {
                bc7_file(pw, ph, &bc7::encode(pw, ph, &img))
            } else {
                png_bytes(pw, ph, &img)?
            };
            report_pages.push(
                json!({ "path": rel, "size": [pw, ph], "sprites": count, "bytes": bytes.len() }),
            );
            files.push((rel, bytes));
            n += 1;
        }
    }
    let bytes: u64 = files.iter().map(|(_, b)| b.len() as u64).sum();
    let report = json!({
        "sprites": list.len(),
        "compressed": cells.iter().filter(|c| c.bc7).count(),
        "lossless": cells.iter().filter(|c| !c.bc7).count(),
        "too_big_for_a_page": alone,
        "pages": report_pages,
        "bytes": bytes,
        "bytes_before": before,
    });
    Ok(Paged {
        catalog,
        files,
        report,
    })
}
