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

pub mod annotate;
pub mod gpu;
mod overlay;
pub mod post;
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

pub use annotate::{annotate, status_bar, Mark};
pub use gpu::{DrawStats, Gpu, Instance, Run};
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
        doc = "Material to draw with: an asset (materials/glow) or a built-in (unlit, lit, palette, dissolve, outline); empty for the plain sprite"
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

/// The built-in atlas of the pixel font the game's own text is drawn with.
pub const FONT_ATLAS: &str = "argus/font";
/// Where the game's text sorts: over everything in the world.
const TEXT_Z: f32 = 1.0e6;

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
    /// Compiled materials by name.
    materials: HashMap<String, MatState>,
    /// Materials the GPU refused, with why (their last good version draws).
    material_errors: BTreeMap<String, String>,
    /// Milliseconds the last material compile took.
    compile_ms: f64,
    /// Clear colour, linear RGBA.
    pub clear: [f32; 4],
    last: FrameReport,
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
            materials: HashMap::new(),
            material_errors: BTreeMap::new(),
            compile_ms: 0.0,
            clear: [0.06, 0.06, 0.08, 1.0],
            last: FrameReport::default(),
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
    /// the asset (`sprites/hero`), with its frames and pivots.
    pub fn load_catalog(&mut self, imp: &argus_assets::Importer) -> argus_ecs::Result<Vec<String>> {
        let names: Vec<String> = imp.sprites().map(|(n, _)| n.clone()).collect();
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
        for name in names {
            let Some(s) = imp.sprites().find(|(n, _)| *n == name).map(|(_, s)| s) else {
                self.gpu.remove_atlas(name);
                continue;
            };
            let (w, h, rgba) = argus_assets::decode_png(&s.texture)?;
            let (tw, th) = (w.max(1) as f32, h.max(1) as f32);
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
            self.gpu.add_atlas_frames(name, w, h, &rgba, frames, pivots);
        }
        Ok(())
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

    /// The game's own text (phase 2, from a mark: Hollow's signs said their
    /// words to the agent only): each shown `Describe`'s text in the pixel
    /// font on a dark plate, centred over its entity's sprite, or at the
    /// top left of the screen for the HUD (role `hud`, or no Transform),
    /// one line each in reading order. Placed on whole screen pixels at 2
    /// pixels a font pixel on a target 600 tall or more, else 1, so the
    /// font stays sharp as the camera moves.
    fn text_instances(
        &self,
        world: &World,
        center: [f32; 2],
        half: [f32; 2],
        (_, h): (u32, u32),
        problems: &mut Vec<String>,
    ) -> Vec<(u64, Instance)> {
        let Some(dc) = world.component_id_of::<Describe>() else {
            return Vec::new();
        };
        let names = self.gpu.atlas_names();
        let (Some(fi), Some(ri)) = (
            names.iter().position(|n| n == FONT_ATLAS),
            names.iter().position(|n| n == "rect"),
        ) else {
            return Vec::new();
        };
        let font = self.gpu.atlas(FONT_ATLAS).expect("indexed");
        let ppu = h as f32 / (half[1] * 2.0).max(1e-6);
        let s = if h >= 600 { 2.0 } else { 1.0 };
        let (left, bottom) = (center[0] - half[0], center[1] - half[1]);
        let hud_role = world.strings().intern("hud");
        let mut world_text: Vec<(f32, f32, String)> = Vec::new();
        let mut hud: Vec<(i32, String)> = Vec::new();
        for e in world.query().has(dc).entities() {
            let Ok(d) = world.get::<Describe>(e) else {
                continue;
            };
            if !d.shown {
                continue;
            }
            let text = semantic::fill(world, e, &world.strings().resolve(d.text), problems);
            if text.is_empty() {
                continue;
            }
            let pos = world
                .get::<GlobalTransform>(e)
                .map(|g| (g.pos, g.scale))
                .or_else(|_| world.get::<Transform>(e).map(|t| (t.pos, t.scale)));
            match pos {
                Ok((p, scale)) if d.role != hud_role => {
                    let top = world
                        .get::<Sprite>(e)
                        .map(|sp| sp.size.y * scale.y * 0.5)
                        .unwrap_or(0.0);
                    world_text.push((p.x, p.y + top, text));
                }
                _ => hud.push((d.order, text)),
            }
        }
        hud.sort_by_key(|(o, _)| *o);
        let mut out = Vec::new();
        let key = |z: f32, atlas: usize| -> u64 {
            let zb = z.to_bits();
            let zo = if zb & 0x8000_0000 != 0 {
                !zb
            } else {
                zb | 0x8000_0000
            } as u64;
            (zo << 32) | atlas as u64
        };
        // A block: its bottom left corner in screen pixels (y up).
        let mut block = |bx: f32, by: f32, text: &str| {
            let n = text.chars().count() as f32;
            let (bw, bh) = ((n * 6.0 + 2.0) * s, 10.0 * s);
            let at = |px: f32, py: f32| [left + px / ppu, bottom + py / ppu];
            let c = at(bx + bw / 2.0, by + bh / 2.0);
            out.push((
                key(TEXT_Z, ri),
                Instance {
                    pos_size: [c[0], c[1], bw / ppu, bh / ppu],
                    rot_z: [0.0, TEXT_Z, 0.5, 0.5],
                    uv: [0.0, 0.0, 1.0, 1.0],
                    color: [0.004, 0.004, 0.012, 0.8],
                    extra: [0.0; 4],
                },
            ));
            for (i, ch) in text.chars().enumerate() {
                if ch == ' ' {
                    continue;
                }
                let g = at(bx + (1.0 + 6.0 * i as f32 + 3.0) * s, by + 5.0 * s);
                out.push((
                    key(TEXT_Z + 1.0, fi),
                    Instance {
                        pos_size: [g[0], g[1], 6.0 * s / ppu, 8.0 * s / ppu],
                        rot_z: [0.0, TEXT_Z + 1.0, 0.5, 0.5],
                        uv: font.frames[annotate::font_frame(ch)],
                        color: [0.87, 0.81, 0.58, 1.0],
                        extra: [0.0; 4],
                    },
                ));
            }
        };
        for (x, top, text) in &world_text {
            let n = text.chars().count() as f32;
            let bx = ((x - left) * ppu - (n * 6.0 + 2.0) * s / 2.0).round();
            let by = ((top - bottom) * ppu + 4.0 * s).round();
            block(bx, by, text);
        }
        for (i, (_, text)) in hud.iter().enumerate() {
            let by = h as f32 - 4.0 * s - (i as f32 + 1.0) * 12.0 * s;
            block(4.0 * s, by.round(), text);
        }
        out
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
            let strings = world.strings();
            // StrId -> atlas index (u32::MAX for a missing atlas), resolved once per name.
            let mut atlas_of: HashMap<StrId, u32> = HashMap::new();
            let mut kind_of: HashMap<StrId, u32> = HashMap::new();
            let prev = &self.prev;
            let colors = &mut self.colors;
            let gpu = &self.gpu;
            let mat_of = &mut mat_of;
            let missing_mats = &mut missing_mats;
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
                instances.push((
                    (zo << 32) | mk | ai as u64,
                    Instance {
                        pos_size: [x, y, s.size.x * scale.x, s.size.y * scale.y],
                        rot_z: [rot, s.z, pivot[0], pivot[1]],
                        uv,
                        color,
                        extra,
                    },
                ));
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
                            ((zo + 1) << 32) | mk | ai as u64,
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

        // Text the player reads (Describe.shown), over everything. A hole
        // that names nothing is the semantic frame's to report.
        let mut holes = Vec::new();
        instances.extend(self.text_instances(world, center, half, (w, h), &mut holes));

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

        // Back to front by z, atlas runs within equal z: one draw per run.
        // Particle groups go in after the sprites with the same key.
        instances.sort_unstable_by_key(|x| x.0);
        let mut runs: Vec<Run> = Vec::new();
        let mut flat: Vec<Instance> = Vec::with_capacity(instances.len() + pflat.len());
        let mut run_key = u64::MAX;
        // The low 32 bits of a key are the material (high 16) and the atlas.
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
        for (key, inst) in &instances {
            while g < groups.len() && groups[g].0 < *key {
                let (k, a, b) = groups[g];
                open(k & 0xffff_ffff, b - a, flat.len(), &mut runs);
                flat.extend_from_slice(&pflat[a..b]);
                g += 1;
            }
            open(*key & 0xffff_ffff, 1, flat.len(), &mut runs);
            flat.push(*inst);
        }
        for &(k, a, b) in &groups[g..] {
            open(k & 0xffff_ffff, b - a, flat.len(), &mut runs);
            flat.extend_from_slice(&pflat[a..b]);
        }
        // Each material drawn this frame: its uniform (its defaults, what
        // named entities give it now, where its textures' frames are).
        // The runs say which: a handful, however many sprites.
        let used: std::collections::HashSet<usize> = runs.iter().map(|r| r.material).collect();
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
        let stats = self
            .gpu
            .draw(center, half, angle, self.clear, &flat, &runs)?;
        self.last = FrameReport {
            stats,
            missing_atlases: missing,
            interpolated,
            view_center: center,
            view_half: half,
            view_angle: angle,
            camera,
            particles: particle_count,
            missing_materials: missing_mats,
            material_problems: problems,
            ms: started.elapsed().as_secs_f64() * 1e3,
        };
        Ok(self.last.clone())
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
        out.sort_by_key(|p| p.order);
        out.truncate(post::MAX_PASSES);
        out
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
