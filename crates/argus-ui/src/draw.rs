//! The UI as quads to draw (§12): every shown node in draw order, its
//! panel (a flat colour, an image stretched or nine-sliced, a border), a
//! focus outline on the focused button, then its text, glyph by glyph. The
//! renderer draws the list over the world and its post passes, on the
//! canvas scaled to the window; it knows nothing of layout.

use crate::components::{UiButton, UiNode, UiPanel, UiRect, UiText};
use crate::layout::settings;
use crate::text::{lines, FontFace};
use crate::theme::Rgba;
use crate::Shared;
use argus_ecs::{Entity, StrId, World};
use std::rc::Rc;

/// What a quad shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    /// A flat colour.
    Flat,
    /// Part of a sprite's frame: `sub` is u0, v0, u1, v1 as fractions of
    /// the frame.
    Image {
        /// The sprite asset.
        sprite: Rc<str>,
        /// Its frame (the first if it has no such frame).
        frame: u32,
        /// The part of its frame.
        sub: [f32; 4],
    },
    /// A glyph: a frame of a font's atlas.
    Glyph {
        /// The font's atlas.
        atlas: Rc<str>,
        /// The frame.
        frame: u32,
        /// The atlas holds distances (the SDF material draws it).
        sdf: bool,
        /// Bold (rich text's `[b]`): an SDF font's edge further in; a pixel
        /// font's glyph drawn twice, a pixel apart.
        bold: bool,
        /// Italic (`[i]`): how far right the top leans, a fraction of the
        /// height; 0 upright.
        slant: f32,
    },
}

/// One quad on the canvas.
#[derive(Clone, Debug, PartialEq)]
pub struct Quad {
    /// Top left, canvas pixels (y down).
    pub pos: [f32; 2],
    /// Width and height.
    pub size: [f32; 2],
    /// What it shows.
    pub paint: Paint,
    /// Tint, sRGB, 0 to 1.
    pub color: Rgba,
    /// What it may draw inside: min x, min y, max x, max y.
    pub clip: [f32; 4],
    /// The node it belongs to.
    pub node: Entity,
}

/// The UI's quads in draw order. `frame_size` gives the size in pixels of
/// a sprite's frame (for nine-slicing); `None` for one the renderer lacks.
pub fn draw_list(
    world: &World,
    shared: &Shared,
    frame_size: &dyn Fn(&str, u32) -> Option<[f32; 2]>,
) -> Vec<Quad> {
    let Some(rect_id) = world.component_id_of::<UiRect>() else {
        return Vec::new();
    };
    let (scale, contrast) = settings(world, shared);
    let theme = &shared.theme;
    let color = |id: StrId, default: &str| -> Option<Rgba> {
        let s = world.strings().resolve(id);
        let name = if s.is_empty() { default } else { &s };
        if name.is_empty() {
            return None;
        }
        theme.color(name, contrast)
    };
    // What each shown node has, read column by column (a lookup of a
    // component a node lacks would make an error to throw away, every
    // frame).
    struct Shown {
        depth: u32,
        e: Entity,
        r: UiRect,
        button: Option<UiButton>,
        panel: Option<UiPanel>,
        text: Option<UiText>,
        pad: argus_ecs::Vec2,
    }
    let mut nodes: Vec<Shown> = Vec::new();
    world
        .query()
        .has(rect_id)
        .for_each_view(|v| {
            let rs = v.column::<UiRect>()?;
            let bs = v.column::<UiButton>().ok();
            let ps = v.column::<UiPanel>().ok();
            let ts = v.column::<UiText>().ok();
            let ns = v.column::<UiNode>().ok();
            for (i, e) in v.entities().iter().enumerate() {
                if rs[i].shown && rs[i].alpha > 0.0 {
                    nodes.push(Shown {
                        depth: rs[i].depth,
                        e: *e,
                        r: rs[i],
                        button: bs.as_ref().map(|c| c[i]),
                        panel: ps.as_ref().map(|c| c[i]),
                        text: ts.as_ref().map(|c| c[i]),
                        pad: ns.as_ref().map_or(argus_ecs::Vec2::ZERO, |c| c[i].pad),
                    });
                }
            }
            Ok(())
        })
        .ok();
    nodes.sort_by_key(|n| (n.depth, n.e));

    let mut out = Vec::new();
    for n in nodes {
        let (e, r) = (n.e, n.r);
        let clip = [r.clip_min.x, r.clip_min.y, r.clip_max.x, r.clip_max.y];
        if clip[2] <= clip[0] || clip[3] <= clip[1] {
            continue;
        }
        let fade = |mut c: Rgba| {
            c[3] *= r.alpha;
            c
        };
        let pos = [r.pos.x, r.pos.y];
        let size = [r.size.x, r.size.y];
        let mut push = |pos: [f32; 2], size: [f32; 2], paint: Paint, c: Rgba| {
            if size[0] > 0.0 && size[1] > 0.0 && c[3] > 0.0 {
                out.push(Quad {
                    pos,
                    size,
                    paint,
                    color: c,
                    clip,
                    node: e,
                });
            }
        };
        let button = n.button;
        if let Some(p) = n.panel {
            let image = world.strings().resolve(p.image).to_string();
            let fill = color(p.color, if image.is_empty() { "" } else { "#FFFFFF" });
            if image.is_empty() {
                if let Some(c) = fill {
                    push(pos, size, Paint::Flat, fade(c));
                }
            } else {
                let sprite: Rc<str> = Rc::from(image.as_str());
                let tint = fade(fill.unwrap_or([1.0; 4]));
                let frame = p.frame;
                match frame_size(&image, frame) {
                    Some(fs) if p.slice > 0.0 => {
                        let k = if p.scale > 0.0 { p.scale } else { 1.0 };
                        nine(pos, size, fs, p.slice, k, |pos, size, sub| {
                            push(
                                pos,
                                size,
                                Paint::Image {
                                    sprite: sprite.clone(),
                                    frame,
                                    sub,
                                },
                                tint,
                            )
                        });
                    }
                    _ => push(
                        pos,
                        size,
                        Paint::Image {
                            sprite,
                            frame,
                            sub: [0.0, 0.0, 1.0, 1.0],
                        },
                        tint,
                    ),
                }
            }
            if p.border > 0.0 {
                if let Some(c) = color(p.border_color, "border") {
                    border(pos, size, p.border, |p, s| push(p, s, Paint::Flat, fade(c)));
                }
            }
        }
        if button.is_some_and(|b| b.focused) {
            if let Some(c) = color(StrId::EMPTY, "focus") {
                border(pos, size, 2.0, |p, s| push(p, s, Paint::Flat, fade(c)));
            }
        }
        if let Some(t) = n.text {
            let shown = world.strings().resolve(t.shown).to_string();
            if shown.is_empty() {
                continue;
            }
            let font = shared
                .fonts
                .face(&world.strings().resolve(t.font), &theme.font);
            let tsize = if t.size > 0.0 {
                t.size
            } else {
                theme.text_size
            } * scale;
            let pad = n.pad;
            let inner = [pos[0] + pad.x, pos[1] + pad.y];
            let width = (size[0] - 2.0 * pad.x).max(0.0);
            let disabled = button.is_some_and(|b| b.disabled);
            let c = if disabled {
                color(StrId::EMPTY, "disabled")
            } else {
                color(t.color, "text")
            };
            let Some(c) = c else { continue };
            let align = world.strings().resolve(t.align).to_string();
            // Rich text (reaching players item 3): pieces of their own
            // colour, size, weight, and slant; keys on a plate; pictures.
            if crate::rich::has_markup(&shown) {
                let (spans, _) = crate::rich::parse(&shown);
                let ls = crate::rich::lines(font.as_ref(), &spans, tsize, t.wrap.then_some(width));
                // A key is outlined, as a keycap, in the theme's border
                // colour, its words in the text colour: readable on any
                // panel, high contrast included.
                let plate = theme.color("border", contrast);
                let key_words = theme.color("text", contrast).unwrap_or(c);
                let mut y0 = inner[1];
                for line in &ls {
                    let x0 = match align.as_str() {
                        "center" => inner[0] + (width - line.width) * 0.5,
                        "right" => inner[0] + width - line.width,
                        _ => inner[0],
                    };
                    // Each key outlined whole: a run of key pieces side by
                    // side ("Arrow keys") is one keycap, with room inside.
                    if let Some(pl) = plate {
                        let mut caps: Vec<[f32; 4]> = Vec::new();
                        let mut joined = false;
                        for p in &line.pieces {
                            if !p.style.key {
                                joined = false;
                                continue;
                            }
                            let h = p.size * crate::text::LINE;
                            let (x, top) = (x0 + p.x, y0 + line.height - h);
                            match caps.last_mut().filter(|_| joined) {
                                Some(c) => {
                                    c[2] = x + p.width;
                                    c[1] = c[1].min(top);
                                    c[3] = c[3].max(h);
                                }
                                None => caps.push([x, top, x + p.width, h]),
                            }
                            joined = true;
                        }
                        let k = (tsize / 14.0).max(1.0);
                        for [l, top, r, h] in caps {
                            let (x, w) = (l - 2.0 * k, r - l + 4.0 * k);
                            for (at, size) in [
                                ([x, top], [w, k]),
                                ([x, top + h - k], [w, k]),
                                ([x, top], [k, h]),
                                ([x + w - k, top], [k, h]),
                            ] {
                                push(at, size, Paint::Flat, fade(pl));
                            }
                        }
                    }
                    for p in &line.pieces {
                        // Each piece stands on the line's foot.
                        let box_h = p.size * crate::text::LINE;
                        let top = y0 + line.height - box_h;
                        let own = if p.style.key && !disabled {
                            key_words
                        } else {
                            c
                        };
                        let pc = p
                            .style
                            .color
                            .as_deref()
                            .filter(|_| !disabled)
                            .and_then(|n| theme.color(n, contrast))
                            .unwrap_or(own);
                        match &p.kind {
                            crate::rich::Kind::Img { sprite, frame } => push(
                                [x0 + p.x, top + (box_h - p.size) * 0.5],
                                [p.size, p.size],
                                Paint::Image {
                                    sprite: Rc::from(sprite.as_str()),
                                    frame: *frame,
                                    sub: [0.0, 0.0, 1.0, 1.0],
                                },
                                fade([1.0, 1.0, 1.0, c[3]]),
                            ),
                            crate::rich::Kind::Text(words) => {
                                let slant = if p.style.italic { 0.2 } else { 0.0 };
                                let bold = p.style.bold;
                                // A pixel font has no weight: bold is the
                                // glyph again, a font pixel to the right.
                                let twice = bold && !font.sdf();
                                glyphs(font.as_ref(), words, p.size, |g| {
                                    let at = [x0 + p.x + g.x, top + g.y];
                                    let paint = Paint::Glyph {
                                        atlas: font.atlas(),
                                        frame: g.frame,
                                        sdf: font.sdf(),
                                        bold,
                                        slant,
                                    };
                                    if twice {
                                        push(
                                            [at[0] + p.size / 8.0, at[1]],
                                            [g.w, g.h],
                                            paint.clone(),
                                            fade(pc),
                                        );
                                    }
                                    push(at, [g.w, g.h], paint, fade(pc));
                                });
                            }
                        }
                    }
                    y0 += line.height;
                }
                continue;
            }
            let l = lines(font.as_ref(), &shown, tsize, t.wrap.then_some(width));
            for (row, line) in l.lines.iter().enumerate() {
                let x0 = match align.as_str() {
                    "center" => inner[0] + (width - line.width) * 0.5,
                    "right" => inner[0] + width - line.width,
                    _ => inner[0],
                };
                let y0 = inner[1] + row as f32 * l.line_height;
                glyphs(font.as_ref(), &line.text, tsize, |g| {
                    push(
                        [x0 + g.x, y0 + g.y],
                        [g.w, g.h],
                        Paint::Glyph {
                            atlas: font.atlas(),
                            frame: g.frame,
                            sdf: font.sdf(),
                            bold: false,
                            slant: 0.0,
                        },
                        fade(c),
                    )
                });
            }
        }
    }
    out
}

/// Most lines the window's bar shows.
pub const BAR_LINES: usize = 4;

/// The window's own bar (phase 3 item 1, where a one-line picture drawn
/// under the UI stood before): `text` across the bottom of the canvas on a
/// dark plate, in the theme's font and size, wrapped to the canvas, drawn
/// over the game's UI. What the development window says: a mark's note
/// being typed, a proposal on trial, a toast.
///
/// A text longer than [`BAR_LINES`] lines keeps its first line and its
/// last ones, with "..." between: the first says what the bar is, and the
/// end is where a note is being typed. (One line cut to fit hid a long
/// proposal's keys in a small window, found in the phase 2 gate.)
pub fn bar(shared: &Shared, text: &str, color: Rgba) -> Vec<Quad> {
    let theme = &shared.theme;
    let font = shared.fonts.face(&theme.font, &theme.font);
    let size = theme.text_size;
    let pad = [10.0, 6.0];
    let mut l = lines(font.as_ref(), text, size, Some(theme.width - 2.0 * pad[0]));
    if l.lines.len() > BAR_LINES {
        let tail = l.lines.split_off(l.lines.len() - (BAR_LINES - 1));
        l.lines.truncate(1);
        l.lines[0].text.push_str(" ...");
        l.lines.extend(tail);
        l.height = BAR_LINES as f32 * l.line_height;
    }
    let h = l.height + 2.0 * pad[1];
    let top = theme.height - h;
    let clip = [0.0, 0.0, theme.width, theme.height];
    let mut out = vec![Quad {
        pos: [0.0, top],
        size: [theme.width, h],
        paint: Paint::Flat,
        color: [0.02, 0.027, 0.04, 0.92],
        clip,
        node: Entity::NULL,
    }];
    for (row, line) in l.lines.iter().enumerate() {
        let y0 = top + pad[1] + row as f32 * l.line_height;
        glyphs(font.as_ref(), &line.text, size, |g| {
            out.push(Quad {
                pos: [pad[0] + g.x, y0 + g.y],
                size: [g.w, g.h],
                paint: Paint::Glyph {
                    atlas: font.atlas(),
                    frame: g.frame,
                    sdf: font.sdf(),
                    bold: false,
                    slant: 0.0,
                },
                color,
                clip,
                node: Entity::NULL,
            })
        });
    }
    out
}

/// The touch controls over the game (reaching players item 1): a stick's
/// ring and its knob where the finger is, a button's disc and its words,
/// in the theme's colours, brighter while a finger is on it.
pub fn touch(shared: &Shared, controls: &[argus_input::TouchShown]) -> Vec<Quad> {
    let theme = &shared.theme;
    let contrast = theme.high_contrast;
    let text = theme
        .color("text", contrast)
        .unwrap_or([1.0, 1.0, 1.0, 1.0]);
    let panel = theme
        .color("panel", contrast)
        .unwrap_or([0.1, 0.1, 0.12, 1.0]);
    let accent = theme.color("accent", contrast).unwrap_or(text);
    let clip = [0.0, 0.0, theme.width, theme.height];
    let alpha = |c: Rgba, a: f32| [c[0], c[1], c[2], a];
    let disc = |c: (f32, f32), r: f32, color: Rgba| Quad {
        pos: [c.0 - r, c.1 - r],
        size: [2.0 * r, 2.0 * r],
        paint: Paint::Image {
            sprite: "circle".into(),
            frame: 0,
            sub: [0.0, 0.0, 1.0, 1.0],
        },
        color,
        clip,
        node: Entity::NULL,
    };
    let font = shared.fonts.face(&theme.font, &theme.font);
    let mut out = Vec::new();
    for c in controls {
        match c.kind {
            argus_input::TouchKind::Stick => {
                out.push(disc(c.centre, c.radius, alpha(panel, 0.35)));
                let (knob, a) = if c.pressed {
                    (accent, 0.75)
                } else {
                    (text, 0.45)
                };
                out.push(disc(c.knob, c.radius * 0.45, alpha(knob, a)));
            }
            argus_input::TouchKind::Button => {
                let (fill, a) = if c.pressed {
                    (accent, 0.65)
                } else {
                    (panel, 0.45)
                };
                out.push(disc(c.centre, c.radius, alpha(fill, a)));
                if c.label.is_empty() {
                    continue;
                }
                let size = (c.radius * 0.5).clamp(10.0, theme.text_size * 1.5);
                let l = lines(font.as_ref(), &c.label, size, Some(c.radius * 1.8));
                let top = c.centre.1 - l.height * 0.5;
                for (row, line) in l.lines.iter().enumerate() {
                    let x0 = c.centre.0 - line.width * 0.5;
                    let y0 = top + row as f32 * l.line_height;
                    glyphs(font.as_ref(), &line.text, size, |g| {
                        out.push(Quad {
                            pos: [x0 + g.x, y0 + g.y],
                            size: [g.w, g.h],
                            paint: Paint::Glyph {
                                atlas: font.atlas(),
                                frame: g.frame,
                                sdf: font.sdf(),
                                bold: false,
                                slant: 0.0,
                            },
                            color: alpha(text, 0.9),
                            clip,
                            node: Entity::NULL,
                        })
                    });
                }
            }
        }
    }
    out
}

/// Words centred on a canvas point, a line a paragraph, on a dark backing:
/// a debug shape's text (2D staples item 1), in the theme's font.
pub fn label(
    shared: &Shared,
    text: &str,
    size: f32,
    at: [f32; 2],
    color: Rgba,
    clip: [f32; 4],
) -> Vec<Quad> {
    let theme = &shared.theme;
    let font = shared.fonts.face(&theme.font, &theme.font);
    let l = lines(font.as_ref(), text, size, None);
    let pad = [size * 0.3, size * 0.15];
    let top = at[1] - l.height * 0.5;
    let mut out = vec![Quad {
        pos: [at[0] - l.width * 0.5 - pad[0], top - pad[1]],
        size: [l.width + 2.0 * pad[0], l.height + 2.0 * pad[1]],
        paint: Paint::Flat,
        color: [0.0, 0.0, 0.0, 0.6 * color[3]],
        clip,
        node: Entity::NULL,
    }];
    for (row, line) in l.lines.iter().enumerate() {
        let x0 = at[0] - line.width * 0.5;
        let y0 = top + row as f32 * l.line_height;
        glyphs(font.as_ref(), &line.text, size, |g| {
            out.push(Quad {
                pos: [x0 + g.x, y0 + g.y],
                size: [g.w, g.h],
                paint: Paint::Glyph {
                    atlas: font.atlas(),
                    frame: g.frame,
                    sdf: font.sdf(),
                    bold: false,
                    slant: 0.0,
                },
                color,
                clip,
                node: Entity::NULL,
            })
        });
    }
    out
}

fn glyphs(font: &dyn FontFace, text: &str, size: f32, mut f: impl FnMut(crate::text::Glyph)) {
    for g in font.glyphs(text, size) {
        f(g);
    }
}

/// Four quads around a rect, `w` thick, inside it.
fn border(pos: [f32; 2], size: [f32; 2], w: f32, mut f: impl FnMut([f32; 2], [f32; 2])) {
    let w = w.min(size[0] * 0.5).min(size[1] * 0.5);
    f(pos, [size[0], w]);
    f([pos[0], pos[1] + size[1] - w], [size[0], w]);
    f([pos[0], pos[1] + w], [w, size[1] - 2.0 * w]);
    f([pos[0] + size[0] - w, pos[1] + w], [w, size[1] - 2.0 * w]);
}

/// Nine quads of an image whose `inset` edge pixels (drawn `k` canvas
/// pixels each) keep their size while the middle stretches.
fn nine(
    pos: [f32; 2],
    size: [f32; 2],
    frame: [f32; 2],
    inset: f32,
    k: f32,
    mut f: impl FnMut([f32; 2], [f32; 2], [f32; 4]),
) {
    let e = [
        (inset * k).min(size[0] * 0.5),
        (inset * k).min(size[1] * 0.5),
    ];
    let u = [inset / frame[0], inset / frame[1]];
    let xs = [
        pos[0],
        pos[0] + e[0],
        pos[0] + size[0] - e[0],
        pos[0] + size[0],
    ];
    let ys = [
        pos[1],
        pos[1] + e[1],
        pos[1] + size[1] - e[1],
        pos[1] + size[1],
    ];
    let us = [0.0, u[0], 1.0 - u[0], 1.0];
    let vs = [0.0, u[1], 1.0 - u[1], 1.0];
    for row in 0..3 {
        for col in 0..3 {
            f(
                [xs[col], ys[row]],
                [xs[col + 1] - xs[col], ys[row + 1] - ys[row]],
                [us[col], vs[row], us[col + 1], vs[row + 1]],
            );
        }
    }
}
