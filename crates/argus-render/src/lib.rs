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

pub mod gpu;
pub mod window;

use argus_ecs::{Component, Entity, GlobalTransform, StrId, Transform, Vec2, World};
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

pub use gpu::{DrawStats, Gpu, Instance};

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
        }
    }
}

/// The view: an entity with a `Camera` and a position frames the scene.
/// Item 7 adds follow, dead zones, shake.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "The view: world units visible vertically, and whether this camera is the one drawing"
)]
pub struct Camera {
    /// World units visible from bottom to top.
    #[argus(doc = "World units visible vertically", range(0.01, 100000.0), tune)]
    pub height: f32,
    /// The camera that draws (the first active one wins).
    #[argus(doc = "Whether this camera draws")]
    pub active: bool,
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            height: 40.0,
            active: true,
        }
    }
}

/// Register the renderer's components.
pub fn register(world: &mut World) -> argus_ecs::Result<()> {
    world.register::<Sprite>()?;
    world.register::<Camera>()?;
    Ok(())
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
            std::fs::create_dir_all(d).map_err(|x| e(&x))?;
        }
        let file = std::fs::File::create(path).map_err(|x| e(&x))?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), self.width, self.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(|x| e(&x))?;
        w.write_image_data(&self.rgba).map_err(|x| e(&x))?;
        Ok(())
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
    /// Wall time to build and submit, ms.
    pub ms: f64,
}

/// One entity's position and rotation as of a sim tick: entity, x, y, rot, tick.
type PrevPose = Option<(Entity, f32, f32, f32, u64)>;

/// A renderer over a target: builds a frame from a world and draws it.
pub struct Renderer {
    gpu: Gpu,
    /// Positions as of the previous sim tick by entity index, for interpolation (§6).
    prev: Vec<PrevPose>,
    colors: HashMap<StrId, [f32; 4]>,
    /// Clear colour, linear RGBA.
    pub clear: [f32; 4],
    last: FrameReport,
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
    pub fn offscreen(width: u32, height: u32, software: bool) -> argus_ecs::Result<Renderer> {
        Ok(Self::wrap(Gpu::offscreen(width, height, software)?))
    }

    /// Into a window.
    pub fn windowed(
        window: std::sync::Arc<winit::window::Window>,
        software: bool,
    ) -> argus_ecs::Result<Renderer> {
        Ok(Self::wrap(Gpu::windowed(window, software)?))
    }

    fn wrap(gpu: Gpu) -> Renderer {
        Renderer {
            gpu,
            prev: Vec::new(),
            colors: HashMap::new(),
            clear: [0.06, 0.06, 0.08, 1.0],
            last: FrameReport::default(),
        }
    }

    /// The GPU side.
    pub fn gpu(&self) -> &Gpu {
        &self.gpu
    }

    /// The GPU side, mutably (atlas loading, resize).
    pub fn gpu_mut(&mut self) -> &mut Gpu {
        &mut self.gpu
    }

    /// Load every `assets/*.png` under `root` as an atlas named by its
    /// stem, with `<stem>.atlas.toml` beside it when present. Returns
    /// the names loaded; a bad file is an error naming it.
    pub fn load_atlases(&mut self, root: &Path) -> argus_ecs::Result<Vec<String>> {
        let dir = root.join("assets");
        let mut names = Vec::new();
        let Ok(rd) = std::fs::read_dir(&dir) else {
            return Ok(names);
        };
        let mut files: Vec<_> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "png"))
            .collect();
        files.sort();
        for p in files {
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            let side = dir.join(format!("{name}.atlas.toml"));
            self.gpu
                .load_atlas(&name, &p, if side.is_file() { Some(&side) } else { None })?;
            names.push(name);
        }
        Ok(names)
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
        let started = Instant::now();
        let (w, h) = self.gpu.size();
        let aspect = w as f32 / h.max(1) as f32;

        // The view: the first active camera, else the origin.
        let mut center = [0.0f32, 0.0];
        let mut height = 40.0f32;
        if let Some(cam) = world.component_id_of::<Camera>() {
            let mut chosen = false;
            let mut q = world.query().has(cam);
            if let Some(t) = world.component_id_of::<Transform>() {
                q = q.has(t);
            }
            for e in q.entities() {
                let c = world.get::<Camera>(e).unwrap();
                if !c.active || chosen {
                    continue;
                }
                let (x, y, _) = self.position(world, e);
                center = [x, y];
                height = c.height.max(0.01);
                chosen = true;
            }
        }
        let half = [height * 0.5 * aspect, height * 0.5];

        // Instances from every visible sprite, straight from the columns:
        // two passes, entities with a GlobalTransform and entities without.
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
                let (ai, uv, mut color) = if s.atlas == StrId::EMPTY {
                    let k = *kind_of.entry(s.kind).or_insert_with(|| {
                        if &*strings.get(s.kind) == "circle" {
                            circle
                        } else {
                            rect
                        }
                    });
                    (k, [0.0, 0.0, 1.0, 1.0], [1.0, 1.0, 1.0, 1.0])
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
                        (rect, [0.0, 0.0, 1.0, 1.0], [1.0, 0.0, 1.0, 1.0])
                    } else {
                        let a = gpu.atlas(&names[ai as usize]).expect("indexed");
                        let uv = a
                            .frames
                            .get(s.frame as usize)
                            .copied()
                            .unwrap_or(a.frames[0]);
                        (ai, uv, [1.0, 1.0, 1.0, 1.0])
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
                if s.flip_x {
                    uv = [uv[0] + uv[2], uv[1], -uv[2], uv[3]];
                }
                // Sort key: z as an order-preserving integer, then atlas.
                let zb = s.z.to_bits();
                let zo = if zb & 0x8000_0000 != 0 {
                    !zb
                } else {
                    zb | 0x8000_0000
                } as u64;
                instances.push((
                    (zo << 32) | ai as u64,
                    Instance {
                        pos_size: [x, y, s.size.x * scale.x, s.size.y * scale.y],
                        rot_z: [rot, s.z, 0.0, 0.0],
                        uv,
                        color,
                    },
                ));
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
        self.prev = next_prev;

        // Back to front by z, atlas runs within equal z: one draw per run.
        instances.sort_unstable_by_key(|x| x.0);
        let names = self.gpu.atlas_names().to_vec();
        let mut runs: Vec<(String, usize, usize)> = Vec::new();
        for (i, (key, _)) in instances.iter().enumerate() {
            let ai = (*key & 0xffff_ffff) as usize;
            let name = names.get(ai).cloned().unwrap_or_else(|| "rect".to_string());
            match runs.last_mut() {
                Some((n, _, count)) if *n == name => *count += 1,
                _ => runs.push((name, i, 1)),
            }
        }
        let flat: Vec<Instance> = instances.iter().map(|x| x.1).collect();
        let stats = self.gpu.draw(center, half, self.clear, &flat, &runs)?;
        self.last = FrameReport {
            stats,
            missing_atlases: missing,
            interpolated,
            view_center: center,
            view_half: half,
            ms: started.elapsed().as_secs_f64() * 1e3,
        };
        Ok(self.last.clone())
    }

    fn position(&self, world: &World, e: Entity) -> (f32, f32, f32) {
        if let Ok(g) = world.get::<GlobalTransform>(e) {
            return (g.pos.x, g.pos.y, g.rot);
        }
        match world.get::<Transform>(e) {
            Ok(t) => (t.pos.x, t.pos.y, t.rot),
            Err(_) => (0.0, 0.0, 0.0),
        }
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
        let px = (x - r.view_center[0]) / (2.0 * r.view_half[0]) * w as f32 + w as f32 / 2.0;
        let py = h as f32 / 2.0 - (y - r.view_center[1]) / (2.0 * r.view_half[1]) * h as f32;
        (px, py)
    }
}
