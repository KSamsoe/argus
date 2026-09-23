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
    /// A frame fading out over this one (an animator's crossfade).
    #[argus(
        doc = "A frame of the same atlas drawn over this one at weight blend, fading out (written by an Animator's crossfade)"
    )]
    pub blend_frame: u32,
    /// Weight of `blend_frame`, 0 to 1.
    #[argus(doc = "Weight of blend_frame, 0 (none) to 1", range(0, 1))]
    pub blend: f32,
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
        }
    }
}

/// The view comes from a camera entity (item 7, `argus-camera`): the active
/// camera with the highest priority, its position plus its shake.
pub use argus_camera::{Camera, CameraShake};

/// Register the renderer's components (and the camera's, which it reads).
pub fn register(world: &mut World) -> argus_ecs::Result<()> {
    world.register::<Sprite>()?;
    argus_camera::register(world)?;
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
    /// View roll in radians (camera shake).
    pub view_angle: f32,
    /// The camera that drew, if any.
    pub camera: Option<Entity>,
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
            seen: Vec::new(),
            seen_tick: None,
            cam_prev: None,
            cam_seen: None,
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
                        rot_z: [rot, s.z, pivot[0], pivot[1]],
                        uv,
                        color,
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
                            ((zo + 1) << 32) | ai as u64,
                            Instance {
                                pos_size: [x, y, s.size.x * scale.x, s.size.y * scale.y],
                                rot_z: [rot, s.z, bp[0], bp[1]],
                                uv: buv,
                                color: [color[0], color[1], color[2], color[3] * w],
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
                    },
                ));
            }
        }

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
            ms: started.elapsed().as_secs_f64() * 1e3,
        };
        Ok(self.last.clone())
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
