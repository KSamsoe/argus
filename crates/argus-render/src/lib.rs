//! Argus renderer, phase 1 item 1: a wgpu sprite renderer with instancing
//! and atlases, windowed and offscreen (design §2, §6, §14, §17).
//!
//! A frame is built from the world: every entity with a `Sprite` and a
//! position becomes one instance (position, size, rotation, atlas rect,
//! tint), positions interpolated between the last two sim states by the
//! clock's `alpha` (§6), instances sorted by z and grouped by atlas so an
//! atlas is one draw call (§17). Culling is on the GPU (`shader.wgsl`). A
//! sprite with no atlas is a described shape (`kind = "rect"` or
//! `"circle"`, a size, a colour), so gameplay never waits on art (§14).
//! The offscreen target reads back to RGBA and PNG for tests, CI, and
//! `obs.frame`; the windowed target presents to a winit window.

#![warn(missing_docs)]

pub mod access;
pub mod annotate;
pub mod gpu;
pub mod light;
pub mod post;
pub mod see_as;
pub mod semantic;
#[cfg(not(target_arch = "wasm32"))]
pub mod window;

use argus_ecs::{
    ArgusError, Component, ComponentId, Entity, FieldSchema, FieldType, GlobalTransform, StrId,
    Transform, Value, Vec2, World,
};
use argus_material::{Layout, MaterialSpec, ParamType};
use argus_os::time::Instant;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

pub use annotate::{annotate, Mark};
pub use gpu::{DrawStats, Gpu, Instance, Page, Run, Texels, UiDraw};
pub use light::{LightFrame, LightInstance};
pub use see_as::SeeAs;
pub use semantic::{semantic, Describe, FrameSpec, SemanticFrame, View};

/// Error codes this crate adds (50xx).
pub mod codes {
    /// No graphics adapter at all.
    pub const NO_ADAPTER: u32 = 5001;
    /// The GPU refused something.
    pub const GPU: u32 = 5002;
    /// An atlas file could not be read.
    pub const ATLAS: u32 = 5003;
    /// A PNG could not be written.
    pub const PNG: u32 = 5004;
    /// The window could not be opened.
    pub const WINDOW: u32 = 5005;
    /// Two pictures of different sizes were compared.
    pub const SIZES: u32 = 5006;
}

/// What an entity draws: an atlas frame, or a described shape when
/// `atlas` is empty (`kind` is `rect` or `circle`).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Draws the entity: an atlas frame, or a described shape (rect, circle) with a size and a colour when no atlas is named"
)]
pub struct Sprite {
    /// Atlas name (`assets/<name>.png`); empty for a described shape.
    #[argus(doc = "Atlas name; empty for a described shape")]
    pub atlas: StrId,
    /// Frame index in the atlas.
    #[argus(doc = "Frame in the atlas, 0 first")]
    pub frame: u32,
    /// Shape when no atlas: `rect` or `circle`.
    #[argus(doc = "Shape when no atlas: rect or circle")]
    pub kind: StrId,
    /// Size in world units before the transform's scale.
    #[argus(doc = "Size in world units")]
    pub size: Vec2,
    /// Tint as `#RRGGBB` or `#RRGGBBAA`.
    #[argus(doc = "Tint, #RRGGBB or #RRGGBBAA")]
    pub color: StrId,
    /// Draw order: higher is in front.
    #[argus(doc = "Draw order, higher in front", tune)]
    pub z: f32,
    /// Mirror horizontally.
    #[argus(doc = "Mirror horizontally")]
    pub flip_x: bool,
    /// Draw at all.
    #[argus(doc = "Draw at all")]
    pub visible: bool,
    /// A frame fading out over this one (an animator's crossfade).
    #[argus(
        doc = "A frame of the same atlas drawn over this one at weight blend, fading out (written by an Animator's crossfade)"
    )]
    pub blend_frame: u32,
    /// Weight of `blend_frame`, 0 to 1.
    #[argus(doc = "Weight of blend_frame, 0 (none) to 1", range(0, 1))]
    pub blend: f32,
    /// The material to draw with; empty for the plain sprite.
    #[argus(
        doc = "Material to draw with: an asset (materials/glow) or a built-in (unlit, lit, palette, dissolve, outline, sdf_text); empty for the plain sprite"
    )]
    pub material: StrId,
}

impl Default for Sprite {
    fn default() -> Self {
        Sprite {
            atlas: StrId::EMPTY,
            frame: 0,
            kind: StrId::EMPTY,
            size: Vec2::new(1.0, 1.0),
            color: StrId::EMPTY,
            z: 0.0,
            flip_x: false,
            visible: true,
            blend_frame: 0,
            blend: 0.0,
            material: StrId::EMPTY,
        }
    }
}

/// The view comes from a camera entity (item 7, `argus-camera`): the active
/// camera with the highest priority, its position plus its shake.
pub use argus_camera::{Camera, CameraShake};
use argus_particles::{lut_index, ParticleStore, LUT};

/// Register the renderer's components (and the camera's, which it reads).
pub fn register(world: &mut World) -> argus_ecs::Result<()> {
    world.register::<Sprite>()?;
    world.register::<Describe>()?;
    argus_material::register(world)?;
    argus_camera::register(world)?;
    Ok(())
}

/// The built-in atlas of the pixel font: the UI's own font
/// (`argus_ui::PIXEL_FONT`), and the annotations'.
pub const FONT_ATLAS: &str = "argus/font";

/// Pictures as an animated PNG, looping, `fps` a second (phase 2 item 2:
/// a mark's clip). Every browser and most viewers play it; one that does
/// not shows the first picture. Returns the bytes written.
pub fn save_apng(frames: &[Image], fps: f64, path: &Path) -> argus_ecs::Result<usize> {
    let e = |e: &dyn std::fmt::Display| {
        argus_ecs::ArgusError::new(codes::PNG, format!("{}: {e}", path.display()))
    };
    let Some(first) = frames.first() else {
        return Err(e(&"no pictures to write"));
    };
    if let Some(d) = path.parent() {
        argus_os::fs::create_dir_all(d).map_err(|x| e(&x))?;
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, first.width, first.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_animated(frames.len() as u32, 0)
            .map_err(|x| e(&x))?;
        let ms = (1000.0 / fps.max(0.1)).round() as u16;
        enc.set_frame_delay(ms, 1000).map_err(|x| e(&x))?;
        let mut w = enc.write_header().map_err(|x| e(&x))?;
        for f in frames {
            if f.width != first.width || f.height != first.height {
                return Err(e(&"pictures of different sizes"));
            }
            w.write_image_data(&f.rgba).map_err(|x| e(&x))?;
        }
        w.finish().map_err(|x| e(&x))?;
    }
    std::fs::write(path, &out).map_err(|x| e(&x))?;
    Ok(out.len())
}

/// Eight of `frames`, evenly spaced from the first to the last, at half
/// size in a four-by-two grid, each labelled (a mark's clip for a reader
/// that sees one picture of an animated PNG: an agent).
pub fn contact_sheet(frames: &[Image], labels: &[String]) -> Option<Image> {
    let first = frames.first()?;
    let (w, h) = ((first.width / 2).max(1), (first.height / 2).max(1));
    let n = frames.len().min(8);
    let cols = 4u32;
    let rows = (n as u32).div_ceil(cols);
    let mut sheet = Image {
        width: w * cols,
        height: h * rows,
        rgba: vec![0; (w * cols * h * rows * 4) as usize],
    };
    for k in 0..n {
        let i = if n == 1 {
            0
        } else {
            k * (frames.len() - 1) / (n - 1)
        };
        let small = frames[i].scaled(w, h);
        let (ox, oy) = ((k as u32 % cols) * w, (k as u32 / cols) * h);
        for y in 0..h {
            let src = (y * w * 4) as usize;
            let dst = (((oy + y) * sheet.width + ox) * 4) as usize;
            sheet.rgba[dst..dst + (w * 4) as usize]
                .copy_from_slice(&small.rgba[src..src + (w * 4) as usize]);
        }
        if let Some(l) = labels.get(i) {
            sheet.text(ox as i64 + 3, oy as i64 + 3, l, 1, [255, 214, 90, 255]);
        }
    }
    Some(sheet)
}

/// How a picture differs from its golden ([`Image::compare`]).
#[derive(Clone, Debug)]
pub struct Comparison {
    /// Pixels compared.
    pub pixels: u64,
    /// Those whose difference passed the threshold.
    pub differing: u64,
    /// The largest difference, on the threshold's scale (0 to 1).
    pub worst: f64,
    /// The differences as a picture: the golden faded, differing pixels red.
    pub diff: Image,
}

impl Comparison {
    /// The share of pixels that differ.
    pub fn fraction(&self) -> f64 {
        self.differing as f64 / self.pixels.max(1) as f64
    }
}

/// A pixel in YIQ, blended over white by its alpha.
fn yiq(p: &[u8]) -> (f64, f64, f64) {
    let a = p[3] as f64 / 255.0;
    let c = |v: u8| 255.0 + (v as f64 - 255.0) * a;
    let (r, g, b) = (c(p[0]), c(p[1]), c(p[2]));
    (
        r * 0.298_895_31 + g * 0.586_622_47 + b * 0.114_482_23,
        r * 0.595_977_99 - g * 0.274_176_10 - b * 0.321_801_89,
        r * 0.211_470_17 - g * 0.522_617_11 + b * 0.311_146_94,
    )
}

/// pixelmatch's colour difference between two pixels (0 when equal).
fn yiq_delta(a: &[u8], b: &[u8]) -> f64 {
    if a == b {
        return 0.0;
    }
    let (y1, i1, q1) = yiq(a);
    let (y2, i2, q2) = yiq(b);
    let (y, i, q) = (y1 - y2, i1 - i2, q1 - q2);
    0.5053 * y * y + 0.299 * i * i + 0.1957 * q * q
}

/// A frame's pixels, top row first.
#[derive(Clone, Debug)]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// RGBA8, `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

impl Image {
    /// One pixel as RGBA.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }

    /// How this picture differs from `golden`, as a person would see it
    /// (§9: image goldens "with a perceptual threshold"). pixelmatch's
    /// measure: the colour difference in YIQ, which weighs brightness over
    /// hue as the eye does, translucent pixels blended over white. A pixel
    /// differs when its difference passes `threshold` (0 to 1, 0.1 for a
    /// person's eye; 0 counts any change). The difference picture is the
    /// golden faded towards white with the differing pixels red.
    pub fn compare(&self, golden: &Image, threshold: f64) -> argus_ecs::Result<Comparison> {
        if (self.width, self.height) != (golden.width, golden.height) {
            return Err(argus_ecs::ArgusError::new(
                codes::SIZES,
                format!(
                    "the picture is {}x{} and the golden {}x{}",
                    self.width, self.height, golden.width, golden.height
                ),
            )
            .hint("Ask for the golden's size (width, height), or record it again."));
        }
        let max_delta = 35215.0 * threshold * threshold;
        let mut diff = Vec::with_capacity(self.rgba.len());
        let (mut differing, mut worst) = (0u64, 0f64);
        for (a, b) in self
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .zip(golden.rgba.as_chunks::<4>().0)
        {
            let d = yiq_delta(a, b);
            worst = worst.max(d);
            if d > max_delta {
                differing += 1;
                diff.extend([255, 0, 0, 255]);
            } else {
                // The golden, faded: its brightness at a tenth over white.
                let y = yiq(b).0;
                let v = (255.0 + (y - 255.0) * 0.1).clamp(0.0, 255.0) as u8;
                diff.extend([v, v, v, 255]);
            }
        }
        Ok(Comparison {
            pixels: (self.width as u64) * (self.height as u64),
            differing,
            worst: (worst / 35215.0).sqrt(),
            diff: Image {
                width: self.width,
                height: self.height,
                rgba: diff,
            },
        })
    }

    /// As [`compare`](Self::compare), but a pixel differs only when no
    /// pixel within one of it in the other picture is within `threshold`
    /// either, both ways round: a colour that changed counts, an edge that
    /// moved by a pixel does not. What a release's check asks of its
    /// packed sprites (distribution item 5): pixel art scaled by a fraction
    /// lands some screen pixels exactly on a texel boundary, and which
    /// texel wins depends on the texture coordinate's last bit, which
    /// differs between a sprite's own image and its atlas page, and between
    /// GPUs. Image goldens keep the strict comparison.
    pub fn compare_near(&self, other: &Image, threshold: f64) -> argus_ecs::Result<Comparison> {
        let strict = self.compare(other, threshold)?;
        if strict.differing == 0 {
            return Ok(strict);
        }
        let max_delta = 35215.0 * threshold * threshold;
        let (w, h) = (self.width as i64, self.height as i64);
        // Whether `img` has, within a pixel of (x, y), one close to `p`.
        let near = |img: &Image, x: i64, y: i64, p: [u8; 4]| {
            (-1..=1).any(|dy| {
                (-1..=1).any(|dx| {
                    let (nx, ny) = (x + dx, y + dy);
                    nx >= 0
                        && ny >= 0
                        && nx < w
                        && ny < h
                        && yiq_delta(&p, &img.pixel(nx as u32, ny as u32)) <= max_delta
                })
            })
        };
        let mut diff = strict.diff;
        let mut differing = 0u64;
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                if diff.rgba[i..i + 4] != [255, 0, 0, 255] {
                    continue;
                }
                let (a, b) = (
                    self.pixel(x as u32, y as u32),
                    other.pixel(x as u32, y as u32),
                );
                if near(other, x, y, a) && near(self, x, y, b) {
                    // Moved, not changed: shown as the faded golden.
                    let v = (255.0 + (yiq(&b).0 - 255.0) * 0.1).clamp(0.0, 255.0) as u8;
                    diff.rgba[i..i + 4].copy_from_slice(&[v, v, v, 255]);
                } else {
                    differing += 1;
                }
            }
        }
        Ok(Comparison {
            differing,
            diff,
            ..strict
        })
    }

    /// Write as PNG.
    pub fn save_png(&self, path: &Path) -> argus_ecs::Result<()> {
        let e = |e: &dyn std::fmt::Display| {
            argus_ecs::ArgusError::new(codes::PNG, format!("{}: {e}", path.display()))
        };
        if let Some(d) = path.parent() {
            argus_os::fs::create_dir_all(d).map_err(|x| e(&x))?;
        }
        let file = std::fs::File::create(path).map_err(|x| e(&x))?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), self.width, self.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(|x| e(&x))?;
        w.write_image_data(&self.rgba).map_err(|x| e(&x))?;
        Ok(())
    }

    /// The same picture scaled to `width` by `height`, nearest pixel.
    pub fn scaled(&self, width: u32, height: u32) -> Image {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            let sy = (y as u64 * self.height as u64 / height.max(1) as u64) as u32;
            for x in 0..width {
                let sx = (x as u64 * self.width as u64 / width.max(1) as u64) as u32;
                rgba.extend(self.pixel(sx, sy));
            }
        }
        Image {
            width,
            height,
            rgba,
        }
    }

    /// Count of pixels within a tolerance of a colour.
    pub fn count_color(&self, rgb: [u8; 3], tol: u8) -> usize {
        self.rgba
            .chunks(4)
            .filter(|p| {
                p[0].abs_diff(rgb[0]) <= tol
                    && p[1].abs_diff(rgb[1]) <= tol
                    && p[2].abs_diff(rgb[2]) <= tol
            })
            .count()
    }
}

/// What one frame reported.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrameReport {
    /// Draw statistics.
    pub stats: DrawStats,
    /// Sprites naming an atlas that is not loaded (drawn as magenta rects).
    pub missing_atlases: Vec<String>,
    /// Entities interpolated between two sim states.
    pub interpolated: usize,
    /// View centre in world units.
    pub view_center: [f32; 2],
    /// View half extents in world units.
    pub view_half: [f32; 2],
    /// View roll in radians (camera shake).
    pub view_angle: f32,
    /// The camera that drew, if any.
    pub camera: Option<Entity>,
    /// Particles drawn (those in view).
    pub particles: usize,
    /// Lights drawn (2D lighting on: those reaching the view).
    pub lights: usize,
    /// Occluder edges the lights' shadows were cast from (tiles' and
    /// Occluders').
    pub shadow_edges: usize,
    /// Sprites and particles drawn after the light, unlit (Emissive, or an
    /// emissive material).
    pub glowing: usize,
    /// Sprites naming a material that is not loaded (drawn plain).
    pub missing_materials: Vec<String>,
    /// What stopped a material's parameter from being read this frame: a
    /// `from` naming no field, a field of the wrong type, a texture that
    /// is not a sprite.
    pub material_problems: Vec<String>,
    /// Wall time to build and submit, ms.
    pub ms: f64,
}

/// A sprite that is not an entity: a tile, drawn from an atlas frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Extra {
    /// Atlas (a sprite asset name).
    pub atlas: String,
    /// Frame.
    pub frame: u32,
    /// Centre in world units.
    pub pos: [f32; 2],
    /// Size in world units.
    pub size: [f32; 2],
    /// Draw order.
    pub z: f32,
}

/// Runs of instances into `flat` and `runs`: back to front by z, runs by
/// material and atlas within equal z, one draw a run; particle groups go in
/// after the sprites with the same key. The low 32 bits of a key are the
/// material (high 16) and the atlas.
fn runs_of(
    instances: &mut [(u64, Instance)],
    groups: &[(u64, usize, usize)],
    pflat: &[Instance],
    flat: &mut Vec<Instance>,
    runs: &mut Vec<Run>,
) {
    instances.sort_unstable_by_key(|x| x.0);
    let mut run_key = u64::MAX;
    let mut open = |k: u64, n: usize, at: usize, runs: &mut Vec<Run>| {
        if k == run_key {
            if let Some(r) = runs.last_mut() {
                r.count += n;
                return;
            }
        }
        run_key = k;
        runs.push(Run {
            atlas: (k & 0xffff) as usize,
            material: ((k >> 16) & 0xffff) as usize,
            start: at,
            count: n,
        });
    };
    let mut g = 0;
    for (key, inst) in instances.iter() {
        while g < groups.len() && groups[g].0 < *key {
            let (k, a, b) = groups[g];
            open(k & 0xffff_ffff, b - a, flat.len(), runs);
            flat.extend_from_slice(&pflat[a..b]);
            g += 1;
        }
        open(*key & 0xffff_ffff, 1, flat.len(), runs);
        flat.push(*inst);
    }
    for &(k, a, b) in &groups[g..] {
        open(k & 0xffff_ffff, b - a, flat.len(), runs);
        flat.extend_from_slice(&pflat[a..b]);
    }
}

/// The area a `width` x `height` target shows: centre and half extents in
/// world units, from the drawing camera with its shake (else the origin, 40
/// tall). A rolled view is given as its bounding box, which is what culling
/// and tile queries need.
pub fn view_for(world: &World, width: u32, height: u32) -> ([f32; 2], [f32; 2]) {
    let aspect = width as f32 / height.max(1) as f32;
    match argus_camera::view(world, aspect) {
        Some(v) => (v.center, rolled(v.half, v.angle)),
        None => ([0.0, 0.0], [20.0 * aspect, 20.0]),
    }
}

/// The half extents of the box around a view of half size `half` rolled by `angle`.
fn rolled(half: [f32; 2], angle: f32) -> [f32; 2] {
    let (s, c) = angle.sin_cos();
    let (s, c) = (s.abs(), c.abs());
    [c * half[0] + s * half[1], s * half[0] + c * half[1]]
}

/// One entity's position and rotation as of a sim tick: entity, x, y, rot, tick.
type PrevPose = Option<(Entity, f32, f32, f32, u64)>;

/// The drawing camera as of a sim tick: entity, centre with shake, roll, tick.
type CamPose = Option<(Entity, [f32; 2], f32, u64)>;

/// A renderer over a target: builds a frame from a world and draws it.
pub struct Renderer {
    gpu: Gpu,
    /// Poses as of the tick before `seen_tick`, by entity index: what every
    /// frame of `seen_tick` interpolates from (§6).
    prev: Vec<PrevPose>,
    /// Poses as of `seen_tick`, the latest tick drawn.
    seen: Vec<PrevPose>,
    seen_tick: Option<u64>,
    cam_prev: CamPose,
    cam_seen: CamPose,
    colors: HashMap<StrId, [f32; 4]>,
    /// The UI's theme and fonts, when the world has a UI (phase 3 item 1).
    ui: Option<std::rc::Rc<std::cell::RefCell<argus_ui::Shared>>>,
    /// The window's own bar: its words and colour.
    bar: Option<(String, argus_ui::Rgba)>,
    /// Compiled materials by name.
    materials: HashMap<String, MatState>,
    /// Materials the GPU refused, with why (their last good version draws).
    material_errors: BTreeMap<String, String>,
    /// Milliseconds the last material compile took.
    compile_ms: f64,
    /// Clear colour, linear RGBA.
    pub clear: [f32; 4],
    last: FrameReport,
    /// Solid tiles' edges for the next frames' shadows (phase 5 item 3).
    occluders: Vec<argus_light::Edge>,
    /// Sim seconds: a light's flicker.
    time: f64,
}

/// A material the renderer has compiled: where it is on the GPU, and what
/// it reads.
struct MatState {
    index: usize,
    spec: MaterialSpec,
    layout: Layout,
}

/// One per-sprite parameter read from a sprite's entity.
struct Read {
    slot: usize,
    ty: ParamType,
    comp: ComponentId,
    field: FieldSchema,
    axis: Option<u8>,
}

/// A value read from a field as a parameter's numbers; `None` keeps the
/// default (an empty colour, a type the parameter cannot take).
fn param_value(v: &Value, ty: ParamType, axis: Option<u8>) -> Option<[f32; 4]> {
    let n = |x: f64| Some([x as f32, 0.0, 0.0, 0.0]);
    match (v, ty, axis) {
        (Value::Float(f), ParamType::F32, None) => n(*f),
        (Value::Int(i), ParamType::F32, None) => n(*i as f64),
        (Value::UInt(u), ParamType::F32, None) => n(*u as f64),
        (Value::Bool(b), ParamType::F32, None) => n(if *b { 1.0 } else { 0.0 }),
        (Value::Vec2(v), ParamType::F32, Some(0)) => n(v.x as f64),
        (Value::Vec2(v), ParamType::F32, Some(_)) => n(v.y as f64),
        (Value::Vec2(v), ParamType::Vec2, None) => Some([v.x, v.y, 0.0, 0.0]),
        (Value::Str(s), ParamType::Color, None) if !s.is_empty() => parse_color(s),
        _ => None,
    }
}

/// Whether a field can supply a parameter, and if not, why.
fn fits(field: &FieldSchema, ty: ParamType, axis: Option<u8>) -> std::result::Result<(), String> {
    let ok = match (ty, axis) {
        (ParamType::F32, None) => field.ty.is_numeric() || field.ty == FieldType::Bool,
        (ParamType::F32, Some(_)) | (ParamType::Vec2, None) => field.ty == FieldType::Vec2,
        (ParamType::Color, None) => field.ty == FieldType::Str,
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(format!(
            "a {} field cannot give a {} parameter",
            field.ty.name(),
            ty.name()
        ))
    }
}

fn parse_color(s: &str) -> Option<[f32; 4]> {
    let h = s.trim().strip_prefix('#')?;
    let v = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    let (r, g, b) = (v(0)?, v(2)?, v(4)?);
    let a = if h.len() >= 8 { v(6)? } else { 255 };
    let lin = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    Some([lin(r), lin(g), lin(b), a as f32 / 255.0])
}

impl Renderer {
    /// Offscreen at a size. `software` forces the fallback adapter.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn offscreen(width: u32, height: u32, software: bool) -> argus_ecs::Result<Renderer> {
        Ok(Self::wrap(Gpu::offscreen(width, height, software)?))
    }

    /// Forget the poses frames interpolate from: after a seek the last
    /// drawn tick is another moment, and sliding from it would show a frame
    /// that never happened.
    pub fn forget_motion(&mut self) {
        self.prev = Default::default();
        self.seen = Default::default();
        self.seen_tick = None;
        self.cam_prev = None;
        self.cam_seen = None;
    }

    /// Into a window.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn windowed(
        window: std::sync::Arc<winit::window::Window>,
        software: bool,
    ) -> argus_ecs::Result<Renderer> {
        Ok(Self::wrap(Gpu::windowed(window, software)?))
    }

    /// Into a canvas in a web page (phase 2 item 1): WebGPU where the
    /// browser has it, else WebGL2; `webgl` forces WebGL2.
    #[cfg(target_arch = "wasm32")]
    pub async fn web(
        canvas: web_sys::HtmlCanvasElement,
        webgl: bool,
    ) -> argus_ecs::Result<Renderer> {
        Ok(Self::wrap(Gpu::web(canvas, webgl).await?))
    }

    fn wrap(gpu: Gpu) -> Renderer {
        Renderer {
            gpu,
            prev: Vec::new(),
            seen: Vec::new(),
            seen_tick: None,
            cam_prev: None,
            cam_seen: None,
            colors: HashMap::new(),
            ui: None,
            bar: None,
            materials: HashMap::new(),
            material_errors: BTreeMap::new(),
            compile_ms: 0.0,
            clear: [0.06, 0.06, 0.08, 1.0],
            last: FrameReport::default(),
            occluders: Vec::new(),
            time: 0.0,
        }
    }

    /// Compile every material an importer has (the project's, and the
    /// built-ins it does not shadow). Only new or changed ones compile, so
    /// this is the hot reload. A material whose asset now fails to import
    /// keeps its last good version drawing; one the GPU refuses does too,
    /// and its error is returned.
    pub fn load_materials(&mut self, imp: &argus_assets::Importer) -> Vec<(String, ArgusError)> {
        let failed = imp.failures();
        self.set_materials(imp.materials(), |n| failed.contains_key(n))
    }

    /// Compile these materials; drop any others, except those `keep` names.
    pub fn set_materials<'a>(
        &mut self,
        specs: impl IntoIterator<Item = (&'a str, &'a MaterialSpec)>,
        keep: impl Fn(&str) -> bool,
    ) -> Vec<(String, ArgusError)> {
        let mut seen = std::collections::HashSet::new();
        let mut errors = Vec::new();
        for (name, spec) in specs {
            seen.insert(name.to_string());
            if self.materials.get(name).is_some_and(|m| m.spec == *spec) {
                continue;
            }
            let t = Instant::now();
            match self.gpu.set_material(name, spec) {
                Ok(index) => {
                    self.compile_ms = t.elapsed().as_secs_f64() * 1e3;
                    self.materials.insert(
                        name.to_string(),
                        MatState {
                            index,
                            spec: spec.clone(),
                            layout: spec.layout(),
                        },
                    );
                    self.material_errors.remove(name);
                }
                Err(e) => {
                    self.material_errors
                        .insert(name.to_string(), e.message.clone());
                    errors.push((name.to_string(), e));
                }
            }
        }
        let gone: Vec<String> = self
            .materials
            .keys()
            .filter(|n| !seen.contains(*n) && !keep(n))
            .cloned()
            .collect();
        for n in gone {
            self.materials.remove(&n);
            self.gpu.remove_material(&n);
        }
        errors
    }

    /// Loaded materials by name, with their GPU index.
    pub fn materials(&self) -> Vec<(String, usize)> {
        let mut v: Vec<(String, usize)> = self
            .materials
            .iter()
            .map(|(n, m)| (n.clone(), m.index))
            .collect();
        v.sort();
        v
    }

    /// Materials the GPU refused, with why.
    pub fn material_errors(&self) -> &BTreeMap<String, String> {
        &self.material_errors
    }

    /// Milliseconds the last material compile took on the GPU side.
    pub fn material_compile_ms(&self) -> f64 {
        self.compile_ms
    }

    /// Sim seconds, which every material reads as `in.time`.
    pub fn set_time(&mut self, seconds: f32) {
        self.time = seconds as f64;
        self.gpu.set_time(seconds);
    }

    /// The GPU side.
    pub fn gpu(&self) -> &Gpu {
        &self.gpu
    }

    /// The GPU side, mutably (atlas loading, resize).
    pub fn gpu_mut(&mut self) -> &mut Gpu {
        &mut self.gpu
    }

    /// Import `assets/` (the importer writes its cache under `.argus/`)
    /// and load every sprite as an atlas. Returns the names loaded.
    pub fn load_atlases(&mut self, root: &Path) -> argus_ecs::Result<Vec<String>> {
        let mut imp = argus_assets::Importer::new(root);
        imp.scan();
        self.load_catalog(&imp)
    }

    /// Load every sprite of an importer's catalog as an atlas named by
    /// the asset (`sprites/hero`), with its frames and pivots, and every
    /// font's distance-field atlas, a glyph a frame.
    pub fn load_catalog(&mut self, imp: &argus_assets::Importer) -> argus_ecs::Result<Vec<String>> {
        let names: Vec<String> = imp
            .sprites()
            .map(|(n, _)| n.clone())
            .chain(imp.fonts().map(|(n, _)| n.clone()))
            .collect();
        self.reload_sprites(imp, &names)?;
        self.load_materials(imp);
        Ok(names)
    }

    /// Reload the named sprites from the catalog (hot reload after an
    /// import); a name no longer in the catalog is dropped.
    pub fn reload_sprites(
        &mut self,
        imp: &argus_assets::Importer,
        names: &[String],
    ) -> argus_ecs::Result<()> {
        // A release pack's sprites share pages (phase 5 item 1): each
        // texture is loaded once.
        let mut pages: HashMap<std::path::PathBuf, Page> = HashMap::new();
        for name in names {
            if let Some(f) = imp.fonts().find(|(n, _)| *n == name).map(|(_, f)| f) {
                let (tw, th) = (f.atlas[0].max(1) as f32, f.atlas[1].max(1) as f32);
                let frames: Vec<[f32; 4]> = f
                    .glyphs
                    .iter()
                    .map(|g| {
                        [
                            g.rect[0] as f32 / tw,
                            g.rect[1] as f32 / th,
                            g.rect[2] as f32 / tw,
                            g.rect[3] as f32 / th,
                        ]
                    })
                    .collect();
                let pivots = vec![[0.0, 1.0]; frames.len()];
                self.gpu
                    .add_atlas_frames(name, f.atlas[0], f.atlas[1], &f.rgba(), frames, pivots);
                continue;
            }
            let Some(s) = imp.sprites().find(|(n, _)| *n == name).map(|(_, s)| s) else {
                self.gpu.remove_atlas(name);
                continue;
            };
            let page = match pages.get(&s.texture) {
                Some(p) => p.clone(),
                None => {
                    let p = self.load_page(&s.texture)?;
                    pages.insert(s.texture.clone(), p.clone());
                    p
                }
            };
            let (tw, th) = (page.width.max(1) as f32, page.height.max(1) as f32);
            let frames: Vec<[f32; 4]> = s
                .frames
                .iter()
                .map(|f| {
                    [
                        f.rect[0] as f32 / tw,
                        f.rect[1] as f32 / th,
                        f.rect[2] as f32 / tw,
                        f.rect[3] as f32 / th,
                    ]
                })
                .collect();
            let pivots: Vec<[f32; 2]> = s.frames.iter().map(|f| f.pivot).collect();
            self.gpu.add_atlas_on(name, &page, frames, pivots);
            // Its normal map (phase 5 item 3), the same layout as its texture.
            if let Some(n) = &s.normal {
                let (w, h, rgba) = argus_assets::decode_png(n)?;
                if !self.gpu.set_atlas_normal(name, w, h, &rgba) {
                    return Err(ArgusError::new(
                        codes::GPU,
                        format!(
                            "{name}: its normal map is {w}x{h}, its texture {}x{}",
                            page.width, page.height
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    /// A sprite texture on the GPU: a cached PNG, or a release pack's page
    /// (a PNG, or BC7 blocks).
    fn load_page(&mut self, path: &std::path::Path) -> argus_ecs::Result<Page> {
        let label = path.to_string_lossy().into_owned();
        if path.extension().is_some_and(|x| x == "bc7") {
            let bytes = argus_os::fs::read(path)
                .map_err(|e| argus_ecs::ArgusError::new(codes::PNG, format!("{label}: {e}")))?;
            let (w, h, blocks) = argus_assets::pages::read_bc7(&bytes).ok_or_else(|| {
                argus_ecs::ArgusError::new(codes::PNG, format!("{label} is not a BC7 page"))
                    .hint("Rebuild the game with `argus build`.")
            })?;
            return Ok(self.gpu.add_page(&label, w, h, Texels::Bc7(blocks)));
        }
        let (w, h, rgba) = argus_assets::decode_png(path)?;
        Ok(self.gpu.add_page(&label, w, h, Texels::Rgba(&rgba)))
    }

    /// The last frame's report.
    pub fn last(&self) -> &FrameReport {
        &self.last
    }

    /// Build and draw one frame. `tick` is the sim tick the world is at
    /// and `alpha` how far real time is into the next (0 to 1).
    pub fn render(
        &mut self,
        world: &World,
        tick: u64,
        alpha: f32,
    ) -> argus_ecs::Result<FrameReport> {
        self.render_with(world, tick, alpha, &[])
    }

    /// The edges of the solid tiles around the view, for the next frames'
    /// shadows (`argus_light::tile_edges` over `argus_light::reach`); the
    /// renderer adds the Occluders' itself. Unused with lighting off.
    pub fn set_occluders(&mut self, edges: Vec<argus_light::Edge>) {
        self.occluders = edges;
    }

    /// The area this target would show: centre and half extents in world
    /// units, from the drawing camera (or the origin), not interpolated.
    pub fn view(&self, world: &World) -> ([f32; 2], [f32; 2]) {
        let (w, h) = self.gpu.size();
        view_for(world, w, h)
    }

    /// `render` plus sprites that are not entities (tilemaps).
    pub fn render_with(
        &mut self,
        world: &World,
        tick: u64,
        alpha: f32,
        extras: &[Extra],
    ) -> argus_ecs::Result<FrameReport> {
        self.render_frame(world, tick, alpha, extras, None)
    }

    /// `render_with` plus every particle pool: each pool is one run of
    /// instances from its effect's atlas at its z, its particles drawn
    /// oldest first, between their last two ticks' positions.
    pub fn render_frame(
        &mut self,
        world: &World,
        tick: u64,
        alpha: f32,
        extras: &[Extra],
        particles: Option<&ParticleStore>,
    ) -> argus_ecs::Result<FrameReport> {
        let started = Instant::now();
        let (w, h) = self.gpu.size();
        let aspect = w as f32 / h.max(1) as f32;

        // A new tick: what was the latest becomes what frames interpolate
        // from. Every frame drawn within one tick interpolates from the same
        // poses, however many frames a tick lasts.
        if self.seen_tick != Some(tick) {
            self.prev = std::mem::take(&mut self.seen);
            self.cam_prev = self.cam_seen.take();
            self.seen_tick = Some(tick);
        }

        // The view: the drawing camera with its shake, interpolated between
        // the last two ticks like any entity; else the origin.
        let (mut center, mut angle, mut height, mut camera) =
            ([0.0f32, 0.0], 0.0f32, 40.0f32, None);
        if let Some(v) = argus_camera::view(world, aspect) {
            center = v.center;
            angle = v.angle;
            height = v.half[1] * 2.0;
            camera = Some(v.camera);
            self.cam_seen = Some((v.camera, v.center, v.angle, tick));
            if let Some((pe, pc, pa, pt)) = self.cam_prev {
                if pe == v.camera && pt + 1 == tick && alpha < 1.0 {
                    center = [
                        pc[0] + (v.center[0] - pc[0]) * alpha,
                        pc[1] + (v.center[1] - pc[1]) * alpha,
                    ];
                    angle = pa + (v.angle - pa) * alpha;
                }
            }
        }
        let half = [height * 0.5 * aspect, height * 0.5];

        // Instances from every visible sprite, straight from the columns:
        // two passes, entities with a GlobalTransform and entities without.
        // Materials: each one's per-sprite reads and defaults, by GPU index.
        // Problems are kept by material and reported for those drawn.
        let mut problems: Vec<String> = Vec::new();
        let mut read_problems: Vec<(usize, String)> = Vec::new();
        let mut missing_mats: Vec<String> = Vec::new();
        let slots = self
            .materials
            .values()
            .map(|m| m.index + 1)
            .max()
            .unwrap_or(1)
            .max(1);
        let mut sprite_defaults: Vec<[f32; 4]> = vec![[0.0; 4]; slots];
        let mut sprite_reads: Vec<Vec<Read>> = (0..slots).map(|_| Vec::new()).collect();
        for (name, m) in &self.materials {
            for &(pi, slot) in &m.layout.sprite {
                let p = &m.spec.params[pi];
                let n = p.ty.floats();
                sprite_defaults[m.index][slot..slot + n].copy_from_slice(&p.value[..n]);
                let Some(src) = &p.from else { continue };
                match world.registry().resolve_path(&src.path) {
                    Ok((comp, field)) => match fits(field, p.ty, src.axis) {
                        Ok(()) => sprite_reads[m.index].push(Read {
                            slot,
                            ty: p.ty,
                            comp,
                            field: field.clone(),
                            axis: src.axis,
                        }),
                        Err(why) => read_problems.push((
                            m.index,
                            format!("{name}: params.{} from {}: {why}", p.name, src.text()),
                        )),
                    },
                    // A component no scene has registered is simply absent.
                    Err(_)
                        if world
                            .component_id(src.path.split('.').next().unwrap_or(""))
                            .is_none() => {}
                    Err(e) => read_problems.push((
                        m.index,
                        format!(
                            "{name}: params.{} from {}: {}",
                            p.name,
                            src.text(),
                            e.message
                        ),
                    )),
                }
            }
        }
        let mat_by_name: HashMap<&str, usize> = self
            .materials
            .iter()
            .map(|(n, m)| (n.as_str(), m.index))
            .collect();
        let mut mat_of: HashMap<StrId, usize> = HashMap::new();

        // 2D lighting (phase 5 item 3): only with Lighting on the drawing
        // camera; then what glows is drawn after the light, unlit.
        let lighting = argus_light::lighting(world).map(|(_, l)| l);
        let mut glow_of: HashMap<Entity, f32> = HashMap::new();
        let mut emissive_mats: std::collections::HashSet<usize> = Default::default();
        if lighting.is_some() {
            if let Some(em) = world.component_id_of::<argus_light::Emissive>() {
                for e in world.query().has(em).entities() {
                    if let Ok(g) = world.get::<argus_light::Emissive>(e) {
                        if g.amount > 0.0 {
                            glow_of.insert(e, g.amount.min(1.0));
                        }
                    }
                }
            }
            emissive_mats = self
                .materials
                .values()
                .filter(|m| m.spec.emissive)
                .map(|m| m.index)
                .collect();
        }
        let mut glow: Vec<(u64, Instance)> = Vec::new();

        let mut instances: Vec<(u64, Instance)> = Vec::new();
        let mut missing: Vec<String> = Vec::new();
        let mut interpolated = 0usize;
        let mut next_prev: Vec<PrevPose> = Vec::new();
        let sp = world.component_id_of::<Sprite>();
        let tr = world.component_id_of::<Transform>();
        let gt = world.component_id_of::<GlobalTransform>();
        if let (Some(sp), Some(tr)) = (sp, tr) {
            let names = self.gpu.atlas_names().to_vec();
            let atlas_index: HashMap<&str, u32> = names
                .iter()
                .enumerate()
                .map(|(i, n)| (n.as_str(), i as u32))
                .collect();
            let rect = atlas_index["rect"];
            let circle = atlas_index["circle"];
            // Atlases on one page draw as one run: each is keyed by the
            // first atlas on its page.
            let mut first_on: HashMap<u64, u32> = HashMap::new();
            let run_of: Vec<u32> = names
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    let page = self.gpu.atlas(n).map_or(u64::MAX - i as u64, |a| a.page);
                    *first_on.entry(page).or_insert(i as u32)
                })
                .collect();
            let strings = world.strings();
            // StrId -> atlas index (u32::MAX for a missing atlas), resolved once per name.
            let mut atlas_of: HashMap<StrId, u32> = HashMap::new();
            let mut kind_of: HashMap<StrId, u32> = HashMap::new();
            let prev = &self.prev;
            let colors = &mut self.colors;
            let gpu = &self.gpu;
            let mat_of = &mut mat_of;
            let missing_mats = &mut missing_mats;
            let (glow_of, glow) = (&glow_of, &mut glow);
            let (sprite_defaults, sprite_reads, mat_by_name) =
                (&sprite_defaults, &sprite_reads, &mat_by_name);
            let mut push = |e: Entity, x: f32, y: f32, rot: f32, scale: Vec2, s: &Sprite| {
                let (mut x, mut y, mut rot) = (x, y, rot);
                let idx = e.index() as usize;
                if next_prev.len() <= idx {
                    next_prev.resize(idx + 1, None);
                }
                next_prev[idx] = Some((e, x, y, rot, tick));
                if let Some(Some((pe, px, py, pr, pt))) = prev.get(idx) {
                    if *pe == e && pt + 1 == tick && alpha < 1.0 {
                        x = px + (x - px) * alpha;
                        y = py + (y - py) * alpha;
                        rot = pr + (rot - pr) * alpha;
                        interpolated += 1;
                    }
                }
                if !s.visible {
                    return;
                }
                let (ai, uv, pivot, mut color) = if s.atlas == StrId::EMPTY {
                    let k = *kind_of.entry(s.kind).or_insert_with(|| {
                        if &*strings.get(s.kind) == "circle" {
                            circle
                        } else {
                            rect
                        }
                    });
                    (k, [0.0, 0.0, 1.0, 1.0], [0.5, 0.5], [1.0, 1.0, 1.0, 1.0])
                } else {
                    let ai = *atlas_of.entry(s.atlas).or_insert_with(|| {
                        let name = strings.resolve(s.atlas);
                        match atlas_index.get(name.as_str()) {
                            Some(i) => *i,
                            None => {
                                missing.push(name);
                                u32::MAX
                            }
                        }
                    });
                    if ai == u32::MAX {
                        (rect, [0.0, 0.0, 1.0, 1.0], [0.5, 0.5], [1.0, 0.0, 1.0, 1.0])
                    } else {
                        let a = gpu.atlas(&names[ai as usize]).expect("indexed");
                        let f = if (s.frame as usize) < a.frames.len() {
                            s.frame as usize
                        } else {
                            0
                        };
                        let pivot = a.pivots.get(f).copied().unwrap_or([0.5, 0.5]);
                        (ai, a.frames[f], pivot, [1.0, 1.0, 1.0, 1.0])
                    }
                };
                // Drawn in its page's run; its frames stay its own atlas's.
                let run = run_of[ai as usize] as u64;
                if s.color != StrId::EMPTY {
                    let c = *colors.entry(s.color).or_insert_with(|| {
                        parse_color(&strings.resolve(s.color)).unwrap_or([1.0, 0.0, 1.0, 1.0])
                    });
                    color = [
                        color[0] * c[0],
                        color[1] * c[1],
                        color[2] * c[2],
                        color[3] * c[3],
                    ];
                }
                let mut uv = uv;
                let mut pivot = pivot;
                if s.flip_x {
                    uv = [uv[0] + uv[2], uv[1], -uv[2], uv[3]];
                    pivot[0] = 1.0 - pivot[0];
                }
                // The material, and its numbers from this sprite's entity.
                let mi = if s.material == StrId::EMPTY {
                    0
                } else {
                    *mat_of.entry(s.material).or_insert_with(|| {
                        let name = strings.resolve(s.material);
                        match mat_by_name.get(name.as_str()) {
                            Some(&i) => i,
                            None => {
                                if !missing_mats.contains(&name) {
                                    missing_mats.push(name);
                                }
                                0
                            }
                        }
                    })
                };
                let mut extra = sprite_defaults.get(mi).copied().unwrap_or_default();
                if let Some(reads) = sprite_reads.get(mi) {
                    for r in reads {
                        if let Ok(v) = world.get_field_by(e, r.comp, &r.field) {
                            if let Some(n) = param_value(&v, r.ty, r.axis) {
                                let k = r.ty.floats();
                                extra[r.slot..r.slot + k].copy_from_slice(&n[..k]);
                            }
                        }
                    }
                }
                let mk = (mi as u64) << 16;
                // Sort key: z as an order-preserving integer, then atlas.
                let zb = s.z.to_bits();
                let zo = if zb & 0x8000_0000 != 0 {
                    !zb
                } else {
                    zb | 0x8000_0000
                } as u64;
                let inst = Instance {
                    pos_size: [x, y, s.size.x * scale.x, s.size.y * scale.y],
                    rot_z: [rot, s.z, pivot[0], pivot[1]],
                    uv,
                    color,
                    extra,
                };
                instances.push(((zo << 32) | mk | run, inst));
                // Lit, an Emissive sprite again after the light, at its own
                // colour by its amount.
                if let Some(&k) = glow_of.get(&e) {
                    let mut g = inst;
                    g.color[3] *= k;
                    glow.push(((zo << 32) | mk | run, g));
                }
                // A crossfade: the outgoing frame over this one, fading.
                if s.blend > 0.0 && s.atlas != StrId::EMPTY && ai != rect && ai != circle {
                    if let Some(a) = gpu.atlas(&names[ai as usize]) {
                        let f = (s.blend_frame as usize).min(a.frames.len().saturating_sub(1));
                        let mut buv = a.frames[f];
                        let mut bp = a.pivots.get(f).copied().unwrap_or([0.5, 0.5]);
                        if s.flip_x {
                            buv = [buv[0] + buv[2], buv[1], -buv[2], buv[3]];
                            bp[0] = 1.0 - bp[0];
                        }
                        let w = s.blend.min(1.0);
                        instances.push((
                            ((zo + 1) << 32) | mk | run,
                            Instance {
                                pos_size: [x, y, s.size.x * scale.x, s.size.y * scale.y],
                                rot_z: [rot, s.z, bp[0], bp[1]],
                                uv: buv,
                                color: [color[0], color[1], color[2], color[3] * w],
                                extra,
                            },
                        ));
                    }
                }
            };
            let mut q = world.query().has(sp).has(tr);
            if let Some(gt) = gt {
                q = q.not(gt);
            }
            q.each::<(&Transform, &Sprite)>(|e, (t, s)| {
                push(e, t.pos.x, t.pos.y, t.rot, t.scale, s)
            })?;
            if let Some(gt) = gt {
                world
                    .query()
                    .has(sp)
                    .has(gt)
                    .each::<(&GlobalTransform, &Sprite)>(|e, (g, s)| {
                        push(e, g.pos.x, g.pos.y, g.rot, g.scale, s)
                    })?;
            }
        }
        // The lights reaching the view, placed between ticks as sprites
        // are, each with its shadow (phase 5 item 3).
        let mut light_frame: Option<LightFrame> = None;
        let mut shadow_edges = 0usize;
        if let Some(lit) = lighting {
            let mut frame = LightFrame::default();
            let strings = world.strings();
            let mut col = |id: StrId, default: [f32; 4]| {
                if id == StrId::EMPTY {
                    return default;
                }
                *self.colors.entry(id).or_insert_with(|| {
                    parse_color(&strings.resolve(id)).unwrap_or([1.0, 0.0, 1.0, 1.0])
                })
            };
            let a = col(lit.ambient, [0.0, 0.0, 0.0, 1.0]);
            let level = lit.level.max(0.0);
            frame.ambient = [a[0] * level, a[1] * level, a[2] * level];
            let sprites = if lit.shadows {
                argus_light::occluder_edges(world)
            } else {
                Vec::new()
            };
            let tiles: &[argus_light::Edge] = if lit.shadows && lit.tiles {
                &self.occluders
            } else {
                &[]
            };
            shadow_edges = sprites.len() + tiles.len();
            let box_half = rolled(half, angle);
            if let Some(lc) = world.component_id_of::<argus_light::Light>() {
                for e in world.query().has(lc).entities() {
                    let Ok(l) = world.get::<argus_light::Light>(e) else {
                        continue;
                    };
                    if !l.on || l.radius <= 0.0 || l.intensity <= 0.0 {
                        continue;
                    }
                    let Some((mut pos, mut rot, scale)) = argus_light::pose(world, e) else {
                        continue;
                    };
                    let idx = e.index() as usize;
                    if next_prev.len() <= idx {
                        next_prev.resize(idx + 1, None);
                    }
                    next_prev[idx] = Some((e, pos[0], pos[1], rot, tick));
                    if let Some(Some((pe, px, py, pr, pt))) = self.prev.get(idx) {
                        if *pe == e && pt + 1 == tick && alpha < 1.0 {
                            pos = [px + (pos[0] - px) * alpha, py + (pos[1] - py) * alpha];
                            rot = pr + (rot - pr) * alpha;
                        }
                    }
                    let flipped = argus_light::flipped(world, e);
                    let (p, dir) = argus_light::aim(pos, rot, scale, flipped, &l);
                    let r = l.radius;
                    if (p[0] - center[0]).abs() > box_half[0] + r
                        || (p[1] - center[1]).abs() > box_half[1] + r
                    {
                        continue;
                    }
                    let c = col(l.color, [1.0; 4]);
                    let k = l.intensity * argus_light::flicker(e, self.time, l.flicker);
                    let cone = if l.angle < 180.0 {
                        let outer = l.angle.max(0.0).to_radians();
                        let inner = outer * (1.0 - l.softness.clamp(0.0, 1.0));
                        [dir.cos(), dir.sin(), outer.cos(), inner.cos()]
                    } else {
                        [1.0, 0.0, -2.0, -2.0]
                    };
                    frame.lights.push(LightInstance {
                        pos_radius: [p[0], p[1], r, l.falloff.max(0.01)],
                        color: [c[0] * k, c[1] * k, c[2] * k, l.height],
                        cone,
                    });
                    let start = frame.shadow.len();
                    if lit.shadows && l.shadows {
                        // Not by its own entity's occluder, nor its parent's
                        // (a hero's lantern, a lamp's post).
                        let parent = world.get::<argus_ecs::Parent>(e).ok().map(|x| x.entity);
                        argus_light::shadow(
                            p,
                            r,
                            tiles.iter().copied().chain(
                                sprites
                                    .iter()
                                    .filter(|(o, _)| *o != e && Some(*o) != parent)
                                    .map(|(_, x)| *x),
                            ),
                            &mut frame.shadow,
                        );
                    }
                    frame
                        .shadow_of
                        .push((start as u32, (frame.shadow.len() - start) as u32));
                }
            }
            light_frame = Some(frame);
        }
        self.seen = next_prev;

        // Tiles and other sprites that are not entities.
        if !extras.is_empty() {
            let names = self.gpu.atlas_names().to_vec();
            let index: HashMap<&str, u32> = names
                .iter()
                .enumerate()
                .map(|(i, n)| (n.as_str(), i as u32))
                .collect();
            let rect = index["rect"];
            for x in extras {
                let (ai, uv, pivot, color) = match index.get(x.atlas.as_str()) {
                    Some(&ai) => {
                        let a = self.gpu.atlas(&names[ai as usize]).expect("indexed");
                        let f = if (x.frame as usize) < a.frames.len() {
                            x.frame as usize
                        } else {
                            0
                        };
                        (
                            ai,
                            a.frames[f],
                            a.pivots.get(f).copied().unwrap_or([0.5, 0.5]),
                            [1.0, 1.0, 1.0, 1.0],
                        )
                    }
                    None => {
                        if !missing.contains(&x.atlas) {
                            missing.push(x.atlas.clone());
                        }
                        (rect, [0.0, 0.0, 1.0, 1.0], [0.5, 0.5], [1.0, 0.0, 1.0, 1.0])
                    }
                };
                // Tiles are placed by their centre whatever the frame's pivot.
                let _ = pivot;
                let zb = x.z.to_bits();
                let zo = if zb & 0x8000_0000 != 0 {
                    !zb
                } else {
                    zb | 0x8000_0000
                } as u64;
                instances.push((
                    (zo << 32) | ai as u64,
                    Instance {
                        pos_size: [x.pos[0], x.pos[1], x.size[0], x.size[1]],
                        rot_z: [0.0, x.z, 0.5, 0.5],
                        uv,
                        color,
                        extra: [0.0; 4],
                    },
                ));
            }
        }

        // Particles: one group of instances per pool, in view.
        let mut pflat: Vec<Instance> = Vec::new();
        let mut groups: Vec<(u64, usize, usize)> = Vec::new();
        if let Some(store) = particles {
            let box_half = rolled(half, angle);
            pflat.reserve(store.live());
            let names = self.gpu.atlas_names().to_vec();
            let index: HashMap<&str, u32> = names
                .iter()
                .enumerate()
                .map(|(i, n)| (n.as_str(), i as u32))
                .collect();
            for pool in store.pools.values() {
                if pool.is_empty() {
                    continue;
                }
                let fx = &*pool.effect;
                let (ai, uv, pivot, tint) = match index.get(fx.atlas.as_str()) {
                    Some(&ai) => {
                        let a = self.gpu.atlas(&names[ai as usize]).expect("indexed");
                        let f = if (fx.spec.frame as usize) < a.frames.len() {
                            fx.spec.frame as usize
                        } else {
                            0
                        };
                        let pivot = a.pivots.get(f).copied().unwrap_or([0.5, 0.5]);
                        (ai, a.frames[f], pivot, [1.0, 1.0, 1.0, 1.0])
                    }
                    None => {
                        if !missing.contains(&fx.atlas) {
                            missing.push(fx.atlas.clone());
                        }
                        (
                            index["rect"],
                            [0.0, 0.0, 1.0, 1.0],
                            [0.5, 0.5],
                            [1.0, 0.0, 1.0, 1.0],
                        )
                    }
                };
                // Between ticks: a particle was one step of its velocity back.
                let back = if pool.tick + 1 == tick && alpha < 1.0 {
                    pool.dt * (1.0 - alpha)
                } else {
                    0.0
                };
                let local = fx.spec.space == argus_assets::Space::Local;
                let o = if local {
                    [
                        pool.origin_prev[0] + (pool.origin[0] - pool.origin_prev[0]) * alpha,
                        pool.origin_prev[1] + (pool.origin[1] - pool.origin_prev[1]) * alpha,
                    ]
                } else {
                    [0.0, 0.0]
                };
                let z = fx.spec.z;
                let mi = match &fx.spec.material {
                    None => 0,
                    Some(n) => match mat_by_name.get(n.as_str()) {
                        Some(&i) => i,
                        None => {
                            if !missing_mats.contains(n) {
                                missing_mats.push(n.clone());
                            }
                            0
                        }
                    },
                };
                let extra = sprite_defaults.get(mi).copied().unwrap_or_default();
                let start = pflat.len();
                let (xs, ys, vxs, vys, lifes) =
                    (pool.x(), pool.y(), pool.vx(), pool.vy(), pool.life());
                for i in 0..xs.len() {
                    let t = lifes[i];
                    let li = lut_index(t).min(LUT - 1);
                    let s = fx.size[li];
                    let k = if back > 0.0 {
                        pool.speed_at(t) * back
                    } else {
                        0.0
                    };
                    let px = o[0] + xs[i] - vxs[i] * k;
                    let py = o[1] + ys[i] - vys[i] * k;
                    if (px - center[0]).abs() > box_half[0] + s
                        || (py - center[1]).abs() > box_half[1] + s
                    {
                        continue;
                    }
                    let c = fx.color[li];
                    pflat.push(Instance {
                        pos_size: [px, py, s, s],
                        rot_z: [fx.rotation[li], z, pivot[0], pivot[1]],
                        uv,
                        color: [
                            c[0] * tint[0],
                            c[1] * tint[1],
                            c[2] * tint[2],
                            c[3] * tint[3],
                        ],
                        extra,
                    });
                }
                if pflat.len() > start {
                    let zb = z.to_bits();
                    let zo = if zb & 0x8000_0000 != 0 {
                        !zb
                    } else {
                        zb | 0x8000_0000
                    } as u64;
                    groups.push((
                        (zo << 32) | ((mi as u64) << 16) | ai as u64,
                        start,
                        pflat.len(),
                    ));
                }
            }
            groups.sort_by_key(|g| g.0);
        }
        let particle_count = pflat.len();

        // Lit, what an emissive material draws glows: after the light.
        let mut glow_groups: Vec<(u64, usize, usize)> = Vec::new();
        if !emissive_mats.is_empty() {
            let is_glow = |k: u64| emissive_mats.contains(&(((k >> 16) & 0xffff) as usize));
            let (g, keep): (Vec<_>, Vec<_>) = instances.into_iter().partition(|x| is_glow(x.0));
            instances = keep;
            glow.extend(g);
            let (g, keep): (Vec<_>, Vec<_>) = groups.into_iter().partition(|x| is_glow(x.0));
            groups = keep;
            glow_groups = g;
        }
        let glowing = glow.len() + glow_groups.iter().map(|g| g.2 - g.1).sum::<usize>();
        let mut runs: Vec<Run> = Vec::new();
        let mut flat: Vec<Instance> =
            Vec::with_capacity(instances.len() + pflat.len() + glow.len());
        runs_of(&mut instances, &groups, &pflat, &mut flat, &mut runs);
        let mut glow_runs: Vec<Run> = Vec::new();
        runs_of(&mut glow, &glow_groups, &pflat, &mut flat, &mut glow_runs);
        // Each material drawn this frame: its uniform (its defaults, what
        // named entities give it now, where its textures' frames are).
        // The runs say which: a handful, however many sprites.
        let used: std::collections::HashSet<usize> = runs
            .iter()
            .chain(glow_runs.iter())
            .map(|r| r.material)
            .collect();
        for (i, why) in read_problems {
            if used.contains(&i) {
                problems.push(why);
            }
        }
        for (name, m) in &self.materials {
            if !used.contains(&m.index) {
                continue;
            }
            let mut bytes = m.spec.default_uniform(&m.layout);
            for f in &m.layout.fields {
                let put = |bytes: &mut Vec<u8>, xs: &[f32]| {
                    for (k, x) in xs.iter().enumerate().take(f.floats) {
                        bytes[f.offset + k * 4..f.offset + k * 4 + 4]
                            .copy_from_slice(&x.to_le_bytes());
                    }
                };
                match f.slot {
                    argus_material::Slot::Param(i) => {
                        let p = &m.spec.params[i];
                        let Some(src) = p.from.as_ref().filter(|s| !s.per_sprite()) else {
                            continue;
                        };
                        let who = src.entity.as_deref().unwrap_or_default();
                        let read = world
                            .entity(&format!("@{who}"))
                            .map_err(|e| e.message)
                            .and_then(|e| world.get_field(e, &src.path).map_err(|e| e.message));
                        match read {
                            Ok(v) => match param_value(&v, p.ty, src.axis) {
                                Some(xs) => put(&mut bytes, &xs),
                                None if matches!(v, Value::Str(ref s) if s.is_empty()) => {}
                                None => problems.push(format!(
                                    "{name}: params.{} from {}: a {} cannot give a {} parameter",
                                    p.name,
                                    src.text(),
                                    v.type_name(),
                                    p.ty.name()
                                )),
                            },
                            Err(why) => problems.push(format!(
                                "{name}: params.{} from {}: {why}",
                                p.name,
                                src.text()
                            )),
                        }
                    }
                    argus_material::Slot::TextureRect(i) | argus_material::Slot::TextureSize(i) => {
                        let t = &m.spec.params[i].texture;
                        let (rect, px) = match self.gpu.atlas(t) {
                            Some(a) => {
                                let r = a.frames.first().copied().unwrap_or([0.0, 0.0, 1.0, 1.0]);
                                (r, [r[2] * a.width as f32, r[3] * a.height as f32])
                            }
                            None => {
                                let why = if t.is_empty() {
                                    format!("{name}: params.{} names no texture; set its default to a sprite asset", m.spec.params[i].name)
                                } else {
                                    format!(
                                        "{name}: params.{}: {t:?} is not a sprite asset",
                                        m.spec.params[i].name
                                    )
                                };
                                if !problems.contains(&why) {
                                    problems.push(why);
                                }
                                ([0.0, 0.0, 1.0, 1.0], [1.0, 1.0])
                            }
                        };
                        if matches!(f.slot, argus_material::Slot::TextureRect(_)) {
                            put(&mut bytes, &rect);
                        } else {
                            put(&mut bytes, &px);
                        }
                    }
                }
            }
            self.gpu.set_material_uniform(m.index, &bytes);
        }

        let post = self.post_passes(world, camera);
        self.gpu.set_post(post);
        let lights = light_frame.as_ref().map_or(0, |f| f.lights.len());
        self.gpu.set_light(light_frame.map(|mut f| {
            f.glow = glow_runs;
            f
        }));
        let ui = self.ui_draw(world, &mut flat);
        let stats = self
            .gpu
            .draw(center, half, angle, self.clear, &flat, &runs, ui.as_ref())?;
        self.last = FrameReport {
            stats,
            missing_atlases: missing,
            interpolated,
            view_center: center,
            view_half: half,
            view_angle: angle,
            camera,
            particles: particle_count,
            lights,
            shadow_edges,
            glowing,
            missing_materials: missing_mats,
            material_problems: problems,
            ms: started.elapsed().as_secs_f64() * 1e3,
        };
        Ok(self.last.clone())
    }

    /// Draw the world's UI trees over it (phase 3 item 1), with this theme
    /// and these fonts; `None` draws none.
    pub fn set_ui(&mut self, shared: Option<std::rc::Rc<std::cell::RefCell<argus_ui::Shared>>>) {
        self.ui = shared;
    }

    /// The window's own bar (a mark's note being typed, a proposal on
    /// trial): drawn by the UI across the bottom of the canvas, over
    /// everything, in `rgba`; or none.
    pub fn set_bar(&mut self, bar: Option<(&str, [u8; 4])>) {
        self.bar = bar.map(|(t, c)| {
            (
                t.to_string(),
                [
                    c[0] as f32 / 255.0,
                    c[1] as f32 / 255.0,
                    c[2] as f32 / 255.0,
                    c[3] as f32 / 255.0,
                ],
            )
        });
    }

    /// How the UI canvas sits in the target, when there is a UI: what a
    /// host turns the mouse into canvas pixels with.
    pub fn ui_fit(&self) -> Option<argus_ui::Fit> {
        let sh = self.ui.as_ref()?.borrow();
        let (w, h) = self.gpu.size();
        Some(argus_ui::Fit::new(
            [sh.theme.width, sh.theme.height],
            [w as f32, h as f32],
        ))
    }

    /// The UI's quads as instances after the world's, in runs by atlas and
    /// scissor, and the view that puts the canvas in the target.
    fn ui_draw(&self, world: &World, flat: &mut Vec<Instance>) -> Option<UiDraw> {
        use argus_ui::draw::Paint;
        // With no UI, the theme's defaults (the bar may still be up).
        let default;
        let guard = self.ui.as_ref().map(|u| u.borrow());
        let shared: &argus_ui::Shared = match &guard {
            Some(g) => g,
            None => {
                default = argus_ui::Shared::default();
                &default
            }
        };
        let (w, h) = self.gpu.size();
        let fit = argus_ui::Fit::new(
            [shared.theme.width, shared.theme.height],
            [w as f32, h as f32],
        );
        let names = self.gpu.atlas_names();
        let frame_size = |name: &str| -> Option<[f32; 2]> {
            let a = self.gpu.atlas(name)?;
            let f = a.frames.first()?;
            Some([f[2].abs() * a.width as f32, f[3] * a.height as f32])
        };
        let mut quads = if guard.is_some() {
            argus_ui::draw::draw_list(world, shared, &frame_size)
        } else {
            Vec::new()
        };
        if let Some((text, color)) = &self.bar {
            quads.extend(argus_ui::draw::bar(shared, text, *color));
        }
        if quads.is_empty() {
            return None;
        }
        let lin = |c: f32| {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let full = [0.0, 0.0, 1.0, 1.0];
        let mut runs: Vec<(Run, [u32; 4])> = Vec::new();
        for q in quads {
            let (atlas, frame, sub, material) = match &q.paint {
                Paint::Flat => ("rect", 0u32, full, 0usize),
                Paint::Image { sprite, sub } => (&**sprite, 0, *sub, 0),
                Paint::Glyph { atlas, frame, sdf } => (
                    &**atlas,
                    *frame,
                    full,
                    if *sdf {
                        self.gpu.material_index("sdf_text").unwrap_or(0)
                    } else {
                        0
                    },
                ),
            };
            let Some(ai) = names.iter().position(|n| n == atlas) else {
                continue;
            };
            let Some(uv) = self
                .gpu
                .atlas(atlas)
                .and_then(|a| a.frames.get(frame as usize))
                .copied()
            else {
                continue;
            };
            let start = flat.len();
            flat.push(Instance {
                pos_size: [q.pos[0], -q.pos[1], q.size[0], q.size[1]],
                // Pivot at the top left (a fraction of the frame, y up).
                rot_z: [0.0, 0.0, 0.0, 1.0],
                uv: [
                    uv[0] + sub[0] * uv[2],
                    uv[1] + sub[1] * uv[3],
                    (sub[2] - sub[0]) * uv[2],
                    (sub[3] - sub[1]) * uv[3],
                ],
                color: [
                    lin(q.color[0]),
                    lin(q.color[1]),
                    lin(q.color[2]),
                    q.color[3],
                ],
                extra: [0.0; 4],
            });
            let tl = fit.to_window([q.clip[0], q.clip[1]]);
            let br = fit.to_window([q.clip[2], q.clip[3]]);
            let x0 = tl[0].max(0.0).floor();
            let y0 = tl[1].max(0.0).floor();
            let sc = [
                x0 as u32,
                y0 as u32,
                (br[0].min(w as f32).ceil() - x0).max(0.0) as u32,
                (br[1].min(h as f32).ceil() - y0).max(0.0) as u32,
            ];
            match runs.last_mut() {
                Some((r, s))
                    if r.atlas == ai
                        && r.material == material
                        && *s == sc
                        && r.start + r.count == start =>
                {
                    r.count += 1
                }
                _ => runs.push((
                    Run {
                        atlas: ai,
                        material,
                        start,
                        count: 1,
                    },
                    sc,
                )),
            }
        }
        Some(UiDraw {
            center: [fit.canvas[0] * 0.5, -fit.canvas[1] * 0.5],
            half: [w as f32 * 0.5 / fit.scale, h as f32 * 0.5 / fit.scale],
            runs,
        })
    }

    /// The drawing camera's post passes this frame (phase 2 item 6), in
    /// order: the ones doing something (a shake with trauma, a flash still
    /// fading, any pass with an amount).
    fn post_passes(&mut self, world: &World, camera: Option<Entity>) -> Vec<post::PostPass> {
        use argus_post::*;
        use post::{PostKind, PostPass};
        let Some(cam) = camera else {
            return Vec::new();
        };
        let strings = world.strings();
        let colors = &mut self.colors;
        let mut color = |id: StrId, default: [f32; 4]| {
            if id == StrId::EMPTY {
                return default;
            }
            *colors.entry(id).or_insert_with(|| {
                parse_color(&strings.resolve(id)).unwrap_or([1.0, 0.0, 1.0, 1.0])
            })
        };
        let pass = |kind, order, a: [f32; 4], b: [f32; 4]| PostPass {
            kind,
            order,
            a,
            b,
            palette: [[0.0; 4]; 8],
        };
        let mut out = Vec::new();
        if let Ok(s) = world.get::<PostShake>(cam) {
            if s.offset != Vec2::ZERO {
                out.push(pass(
                    PostKind::Shake,
                    s.order,
                    [s.offset.x, s.offset.y, 0.0, 0.0],
                    [0.0; 4],
                ));
            }
        }
        if let Ok(a) = world.get::<PostAberration>(cam) {
            if a.amount > 0.0 {
                out.push(pass(
                    PostKind::Aberration,
                    a.order,
                    [a.amount, 0.0, 0.0, 0.0],
                    [0.0; 4],
                ));
            }
        }
        if let Ok(p) = world.get::<PostPalette>(cam) {
            if p.amount > 0.0 {
                let text = if p.colors == StrId::EMPTY {
                    DEFAULT_PALETTE.to_string()
                } else {
                    strings.resolve(p.colors)
                };
                let cs: Vec<[f32; 4]> = text
                    .split_whitespace()
                    .filter_map(parse_color)
                    .take(post::MAX_PASSES)
                    .collect();
                if !cs.is_empty() {
                    let mut pp = pass(
                        PostKind::Palette,
                        p.order,
                        [p.amount, cs.len() as f32, 0.0, 0.0],
                        [0.0; 4],
                    );
                    for (i, c) in cs.iter().enumerate() {
                        pp.palette[i] = *c;
                    }
                    out.push(pp);
                }
            }
        }
        if let Ok(f) = world.get::<PostFlash>(cam) {
            if f.amount > 0.0 {
                out.push(pass(
                    PostKind::Flash,
                    f.order,
                    color(f.color, [1.0; 4]),
                    [f.amount, 0.0, 0.0, 0.0],
                ));
            }
        }
        if let Ok(v) = world.get::<PostVignette>(cam) {
            if v.amount > 0.0 {
                out.push(pass(
                    PostKind::Vignette,
                    v.order,
                    [v.amount, v.radius, v.softness, 0.0],
                    [0.0; 4],
                ));
            }
        }
        if let Ok(f) = world.get::<PostFade>(cam) {
            if f.amount > 0.0 {
                out.push(pass(
                    PostKind::Fade,
                    f.order,
                    color(f.color, [0.0, 0.0, 0.0, 1.0]),
                    [f.amount, 0.0, 0.0, 0.0],
                ));
            }
        }
        if let Ok(c) = world.get::<PostCrt>(cam) {
            if c.amount > 0.0 {
                out.push(pass(
                    PostKind::Crt,
                    c.order,
                    [c.amount, c.scanlines, c.lines, c.curvature],
                    [c.bleed, c.mask, 0.0, 0.0],
                ));
            }
        }
        out.sort_by_key(|p| p.order);
        out.truncate(post::MAX_PASSES);
        out
    }

    /// The last offscreen frame's pixels into `out` (`width * height * 4`
    /// bytes, RGBA8, rows top first): a frame stream's slot.
    pub fn capture_into(&self, out: &mut [u8]) -> argus_ecs::Result<()> {
        self.gpu.read_pixels_into(out)
    }

    /// The last offscreen frame's pixels.
    pub fn capture(&self) -> argus_ecs::Result<Image> {
        let (width, height) = self.gpu.size();
        Ok(Image {
            width,
            height,
            rgba: self.gpu.read_pixels()?,
        })
    }

    /// World point to pixel in the last frame's view (y down).
    pub fn to_pixel(&self, x: f32, y: f32) -> (f32, f32) {
        let (w, h) = self.gpu.size();
        let r = &self.last;
        // Into the view's frame: unroll by the view's angle.
        let (s, c) = r.view_angle.sin_cos();
        let (dx, dy) = (x - r.view_center[0], y - r.view_center[1]);
        let (vx, vy) = (dx * c + dy * s, -dx * s + dy * c);
        let px = vx / (2.0 * r.view_half[0]) * w as f32 + w as f32 / 2.0;
        let py = h as f32 / 2.0 - vy / (2.0 * r.view_half[1]) * h as f32;
        (px, py)
    }
}
