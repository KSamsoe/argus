//! The wgpu side: device, one sprite pipeline per material, atlases as
//! textures with bind groups, an instance buffer, and the two targets (a
//! window surface or an offscreen texture with CPU readback).

use argus_ecs::{ArgusError, Result};
use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

/// One instance as the shader reads it (80 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    /// x, y, width, height in world units.
    pub pos_size: [f32; 4],
    /// rotation (radians), z, pivot x, pivot y.
    pub rot_z: [f32; 4],
    /// Atlas rect in texture space: u, v, w, h.
    pub uv: [f32; 4],
    /// RGBA tint, linear.
    pub color: [f32; 4],
    /// The material's per-sprite values.
    pub extra: [f32; 4],
}

/// A run of instances drawn with one atlas and one material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    /// Index into [`Gpu::atlas_names`].
    pub atlas: usize,
    /// Index of the material ([`Gpu::material_index`]); 0 is the default.
    pub material: usize,
    /// First instance.
    pub start: usize,
    /// Instances.
    pub count: usize,
}

/// The UI's part of a frame (phase 3 item 1): drawn after the world and its
/// post passes, through a view that maps canvas pixels (y down, as
/// instances with y negated) to the window, each run inside a scissor.
#[derive(Clone, Debug, Default)]
pub struct UiDraw {
    /// The view's centre.
    pub center: [f32; 2],
    /// The view's half size.
    pub half: [f32; 2],
    /// Runs of the frame's instances, each with its scissor (x, y, width,
    /// height, in target pixels).
    pub runs: Vec<(Run, [u32; 4])>,
}

/// One compiled material: its pipeline, its uniform, and the textures its
/// parameters name.
struct GpuMaterial {
    name: String,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    uniform_bytes: Vec<u8>,
    textures: Vec<String>,
    bind: Option<(wgpu::BindGroup, u64)>,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct ViewUniform {
    center: [f32; 2],
    half: [f32; 2],
    pad: [f32; 4],
}

/// A loaded atlas: a texture plus its frame rectangles.
pub struct Atlas {
    /// Pixel size.
    pub width: u32,
    /// Pixel size.
    pub height: u32,
    /// Frame rects in texture space (u, v, w, h), frame 0 first.
    pub frames: Vec<[f32; 4]>,
    /// Each frame's pivot, a fraction of the frame, y up (0.5, 0.5 is the centre).
    pub pivots: Vec<[f32; 2]>,
    /// The texture it is on, which other atlases may share (a release
    /// pack's sprite pages, phase 5 item 1).
    pub page: u64,
    bind_group: wgpu::BindGroup,
    view: wgpu::TextureView,
}

/// A texture atlases are drawn from: one sprite's, or a page many share.
#[derive(Clone)]
pub struct Page {
    /// Its number, the same for every atlas on it.
    pub id: u64,
    /// Pixel size.
    pub width: u32,
    /// Pixel size.
    pub height: u32,
    bind_group: wgpu::BindGroup,
    view: wgpu::TextureView,
}

/// A page's pixels.
pub enum Texels<'a> {
    /// RGBA8, sRGB.
    Rgba(&'a [u8]),
    /// BC7 blocks, row by row (`argus_assets::bc7`).
    Bc7(&'a [u8]),
}

/// Where a frame goes.
enum Target {
    // A page draws into its canvas; offscreen frames are the desktop's.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    Offscreen {
        texture: wgpu::Texture,
        view: wgpu::TextureView,
        readback: wgpu::Buffer,
        padded_row: u32,
    },
    Surface {
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
    },
}

/// What one frame cost and drew.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawStats {
    /// Instances submitted.
    pub sprites: usize,
    /// Draw calls (one per run of instances sharing an atlas and a material).
    pub draw_calls: usize,
    /// Atlases bound.
    pub atlases: usize,
    /// Materials drawn with (the default counts).
    pub materials: usize,
}

/// The renderer proper.
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// Adapter name and backend, for `render.status`.
    pub adapter: String,
    /// True when the software fallback adapter was chosen.
    pub software: bool,
    /// True when the device samples BC7 textures (`TEXTURE_COMPRESSION_BC`).
    pub bc: bool,
    format: wgpu::TextureFormat,
    /// The next page's number.
    pages: u64,
    /// The world's view and the UI's, a slot each, bound at an offset.
    view_buf: wgpu::Buffer,
    view_bind: wgpu::BindGroup,
    /// Where the UI's view starts in `view_buf` (the device's offset
    /// alignment).
    ui_view_at: u32,
    view_layout: wgpu::BindGroupLayout,
    atlas_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    atlases: HashMap<String, Atlas>,
    atlas_order: Vec<String>,
    /// Bumped whenever an atlas is added, replaced, or removed, so material
    /// bind groups naming one are rebuilt.
    atlas_gen: u64,
    materials: Vec<Option<GpuMaterial>>,
    material_names: HashMap<String, usize>,
    /// Sim seconds, for shaders.
    time: f32,
    instance_buf: wgpu::Buffer,
    instance_cap: usize,
    target: Target,
    width: u32,
    height: u32,
    /// A picture over the frame's bottom edge (a mark's note prompt).
    /// The post passes of the next frame (phase 2 item 6).
    post: Vec<crate::post::PostPass>,
    /// The frame as a colour-blind eye sees it, when asked (phase 5 item
    /// 2), and the pass that does it.
    see_as: Option<crate::see_as::SeeAs>,
    view_filter: Option<crate::see_as::ViewFilter>,
    /// Their pipelines and textures, made when first used.
    post_stack: Option<crate::post::PostStack>,
}

fn err(e: impl std::fmt::Display) -> ArgusError {
    ArgusError::new(crate::codes::GPU, e.to_string())
}

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// A view uniform's size, bound at an offset into the view buffer.
const VIEW_SIZE: Option<std::num::NonZeroU64> =
    std::num::NonZeroU64::new(std::mem::size_of::<ViewUniform>() as u64);

impl Gpu {
    #[cfg(not(target_arch = "wasm32"))]
    fn instance(software: bool) -> wgpu::Instance {
        let _ = software;
        // ARGUS_BACKEND (vulkan, dx12, metal, gl) takes that backend only:
        // to find which one a picture depends on.
        let one = std::env::var("ARGUS_BACKEND").ok().and_then(|b| {
            match b.to_ascii_lowercase().as_str() {
                "vulkan" => Some(wgpu::Backends::VULKAN),
                "dx12" => Some(wgpu::Backends::DX12),
                "metal" => Some(wgpu::Backends::METAL),
                "gl" => Some(wgpu::Backends::GL),
                _ => None,
            }
        });
        wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: one.unwrap_or(wgpu::Backends::PRIMARY | wgpu::Backends::GL),
            ..Default::default()
        })
    }

    async fn adapter_and_device(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'static>>,
        software: bool,
    ) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
        #[cfg(target_arch = "wasm32")]
        let _ = software;
        #[cfg(target_arch = "wasm32")]
        let software = false;
        // ARGUS_SOFTWARE=1 takes the software rasteriser for every renderer:
        // CI runs the render tests on it as a machine with no GPU would
        // (section 2; phase 5 item 0).
        #[cfg(not(target_arch = "wasm32"))]
        let software = software || std::env::var("ARGUS_SOFTWARE").is_ok_and(|v| v == "1");
        let mut opts = wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: software,
            compatible_surface: surface,
        };
        let mut adapter = instance.request_adapter(&opts).await;
        if adapter.is_none() && !software {
            // No hardware: take the software rasteriser rather than nothing
            // (design section 9: identical in CI with the software adapter).
            opts.force_fallback_adapter = true;
            adapter = instance.request_adapter(&opts).await;
        }
        let adapter = adapter.ok_or_else(|| {
            ArgusError::new(crate::codes::NO_ADAPTER, "no graphics adapter, hardware or software")
                .hint("On Windows the DirectX 12 WARP rasteriser is the fallback; on Linux install lavapipe or Mesa's llvmpipe.")
        })?;
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("argus"),
                    // BC7 sprite pages (phase 5 item 1) where the adapter
                    // reads them; without, a page is decoded on the CPU.
                    required_features: adapter.features() & wgpu::Features::TEXTURE_COMPRESSION_BC,
                    // WebGL2 has lower limits than any native backend.
                    required_limits: if adapter.get_info().backend == wgpu::Backend::Gl {
                        wgpu::Limits::downlevel_webgl2_defaults()
                    } else {
                        wgpu::Limits::downlevel_defaults()
                    }
                    .using_resolution(adapter.limits()),
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await
            .map_err(err)?;
        Ok((adapter, device, queue))
    }

    /// An offscreen renderer of the given size.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn offscreen(width: u32, height: u32, software: bool) -> Result<Gpu> {
        let instance = Self::instance(software);
        let (adapter, device, queue) =
            pollster::block_on(Self::adapter_and_device(&instance, None, software))?;
        let target = Self::offscreen_target(&device, width, height);
        Self::build(adapter, device, queue, FORMAT, target, width, height)
    }

    /// The texture an offscreen frame is drawn into and the buffer it is
    /// read back through.
    fn offscreen_target(device: &wgpu::Device, width: u32, height: u32) -> Target {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let padded_row = (width * 4).div_ceil(256) * 256;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (padded_row * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Target::Offscreen {
            texture,
            view,
            readback,
            padded_row,
        }
    }

    /// A renderer presenting into a canvas (phase 2 item 1). The browser
    /// cannot block, so this is async; WebGPU when the browser has it, else
    /// WebGL2. A WebGPU canvas offers no sRGB format, so frames are drawn
    /// through an sRGB view of it, the colours native gets.
    #[cfg(target_arch = "wasm32")]
    pub async fn web(canvas: web_sys::HtmlCanvasElement, webgl: bool) -> Result<Gpu> {
        let (width, height) = (canvas.width().max(1), canvas.height().max(1));
        let desc = wgpu::InstanceDescriptor {
            backends: if webgl {
                wgpu::Backends::GL
            } else {
                wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL
            },
            ..Default::default()
        };
        let instance = if webgl {
            wgpu::Instance::new(&desc)
        } else {
            wgpu::util::new_instance_with_webgpu_detection(&desc).await
        };
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(err)?;
        let (adapter, device, queue) =
            Self::adapter_and_device(&instance, Some(&surface), false).await?;
        let caps = surface.get_capabilities(&adapter);
        let base = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let format = base.add_srgb_suffix();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: base,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: if format == base { vec![] } else { vec![format] },
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        let target = Target::Surface { surface, config };
        Self::build(adapter, device, queue, format, target, width, height)
    }

    /// A renderer presenting into a window.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn windowed(window: Arc<winit::window::Window>, software: bool) -> Result<Gpu> {
        let instance = Self::instance(software);
        let size = window.inner_size();
        let surface = instance.create_surface(window).map_err(err)?;
        let (adapter, device, queue) = pollster::block_on(Self::adapter_and_device(
            &instance,
            Some(&surface),
            software,
        ))?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        let (w, h) = (config.width, config.height);
        let target = Target::Surface { surface, config };
        Self::build(adapter, device, queue, format, target, w, h)
    }

    fn build(
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        target: Target,
        width: u32,
        height: u32,
    ) -> Result<Gpu> {
        let info = adapter.get_info();
        let software = info.device_type == wgpu::DeviceType::Cpu;
        // Every uniform is bound at an offset (a root constant buffer on
        // DirectX 12), never through the descriptor heap: on WARP, the
        // software rasteriser CI draws with (section 9), a constant buffer
        // bound through the heap read the one in the next bind group made
        // with the same layout, so the world drew with the UI's view and
        // nothing showed (found by phase 5 item 0's image goldens).
        let view_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("view"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                // The fragment half reads the sim time from it.
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: VIEW_SIZE,
                },
                count: None,
            }],
        });
        let atlas_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("atlas"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let ui_view_at = (std::mem::size_of::<ViewUniform>() as u32)
            .next_multiple_of(device.limits().min_uniform_buffer_offset_alignment);
        let view_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("view"),
            size: (ui_view_at as usize + std::mem::size_of::<ViewUniform>()) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("view"),
            layout: &view_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &view_buf,
                    offset: 0,
                    size: VIEW_SIZE,
                }),
            }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nearest"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let instance_cap = 4096;
        let instance_buf = Self::make_instance_buffer(&device, instance_cap);
        let bc = device
            .features()
            .contains(wgpu::Features::TEXTURE_COMPRESSION_BC);
        let mut gpu = Gpu {
            device,
            queue,
            adapter: format!("{} ({:?})", info.name, info.backend),
            software,
            bc,
            format,
            pages: 0,
            view_buf,
            view_bind,
            ui_view_at,
            view_layout,
            atlas_layout,
            sampler,
            atlases: HashMap::new(),
            atlas_order: Vec::new(),
            atlas_gen: 0,
            materials: Vec::new(),
            material_names: HashMap::new(),
            time: 0.0,
            instance_buf,
            instance_cap,
            target,
            width,
            height,
            post: Vec::new(),
            see_as: None,
            view_filter: None,
            post_stack: None,
        };
        // Built-in atlases for described-shape placeholders (section 14).
        gpu.add_atlas_rgba(
            "rect",
            1,
            1,
            &[255, 255, 255, 255],
            vec![[0.0, 0.0, 1.0, 1.0]],
        );
        let n = 64u32;
        let mut circle = vec![0u8; (n * n * 4) as usize];
        for y in 0..n {
            for x in 0..n {
                let dx = x as f32 + 0.5 - n as f32 / 2.0;
                let dy = y as f32 + 0.5 - n as f32 / 2.0;
                let inside = dx * dx + dy * dy <= (n as f32 / 2.0).powi(2);
                let i = ((y * n + x) * 4) as usize;
                circle[i..i + 4].copy_from_slice(&[255, 255, 255, if inside { 255 } else { 0 }]);
            }
        }
        gpu.add_atlas_rgba("circle", n, n, &circle, vec![[0.0, 0.0, 1.0, 1.0]]);
        // The pixel font: the UI's built-in font.
        let (fw, fh, font, frames) = crate::annotate::font_atlas();
        let pivots = vec![[0.5, 0.5]; frames.len()];
        gpu.add_atlas_frames(crate::FONT_ATLAS, fw, fh, &font, frames, pivots);
        // The default material, index 0: the built-in unlit, what a sprite
        // with no material draws.
        let unlit = argus_material::builtin("unlit").expect("the library has unlit");
        gpu.set_material("", unlit)?;
        Ok(gpu)
    }

    /// Compile a material (or replace one of the same name, keeping its
    /// index). The shader is composed exactly as `argus_material::check`
    /// composed it at import; anything the GPU still refuses is returned
    /// and the old version, if any, stays.
    pub fn set_material(
        &mut self,
        name: &str,
        spec: &argus_material::MaterialSpec,
    ) -> Result<usize> {
        let composed = argus_material::compose(spec);
        let layout = &composed.layout;
        // The GPU's own check of a material. A browser cannot block on it;
        // there the importer's naga check is the one that runs.
        #[cfg(not(target_arch = "wasm32"))]
        self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(name),
                source: wgpu::ShaderSource::Wgsl(composed.wgsl.as_str().into()),
            });
        let mut entries = vec![
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                // At an offset, as the view is (see `build`): two materials
                // with the same kinds of parameters share a layout.
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ];
        for k in 0..layout.textures.len() {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: 2 + k as u32,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
        }
        let mat_layout = self
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(name),
                entries: &entries,
            });
        let pipe_layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(name),
                bind_group_layouts: &[&self.view_layout, &self.atlas_layout, &mat_layout],
                push_constant_ranges: &[],
            });
        let blend = match spec.blend {
            argus_material::Blend::Alpha => wgpu::BlendState::ALPHA_BLENDING,
            argus_material::Blend::Add => wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::SrcAlpha,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::OVER,
            },
            argus_material::Blend::Multiply => wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Dst,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::OVER,
            },
        };
        let pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(name),
            layout: Some(&pipe_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Instance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4],
                }],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview: None,
            cache: None,
        });
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(e) = pollster::block_on(self.device.pop_error_scope()) {
            return Err(ArgusError::new(
                argus_material::codes::SHADER,
                format!("{}: the GPU refused the material: {e}", spec.shader_file),
            )
            .at(Some(spec.shader_file.clone())));
        }
        let uniform_bytes = spec.default_uniform(layout);
        let uniform = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(name),
            size: layout.size as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&uniform, 0, &uniform_bytes);
        let m = GpuMaterial {
            name: name.to_string(),
            pipeline,
            layout: mat_layout,
            uniform,
            uniform_bytes,
            textures: layout
                .textures
                .iter()
                .map(|&i| spec.params[i].texture.clone())
                .collect(),
            bind: None,
        };
        let index = match self.material_names.get(name) {
            Some(&i) => i,
            None => {
                let i = self
                    .materials
                    .iter()
                    .position(Option::is_none)
                    .unwrap_or(self.materials.len());
                if i == self.materials.len() {
                    self.materials.push(None);
                }
                self.material_names.insert(name.to_string(), i);
                i
            }
        };
        self.materials[index] = Some(m);
        Ok(index)
    }

    /// Forget a material (its asset went away).
    pub fn remove_material(&mut self, name: &str) -> bool {
        match self.material_names.remove(name) {
            Some(i) => {
                self.materials[i] = None;
                true
            }
            None => false,
        }
    }

    /// A material's index by name ("" is the default).
    pub fn material_index(&self, name: &str) -> Option<usize> {
        self.material_names.get(name).copied()
    }

    /// Every material's name, by index (the default is "").
    pub fn material_names(&self) -> Vec<(usize, String)> {
        let mut v: Vec<(usize, String)> = self
            .materials
            .iter()
            .enumerate()
            .filter_map(|(i, m)| m.as_ref().map(|m| (i, m.name.clone())))
            .collect();
        v.sort();
        v
    }

    /// Write a material's uniform (its parameters) when it changed.
    pub fn set_material_uniform(&mut self, index: usize, bytes: &[u8]) {
        if let Some(Some(m)) = self.materials.get_mut(index) {
            if m.uniform_bytes != bytes && bytes.len() == m.uniform_bytes.len() {
                self.queue.write_buffer(&m.uniform, 0, bytes);
                m.uniform_bytes = bytes.to_vec();
            }
        }
    }

    /// Sim seconds, which every material reads as `in.time`.
    pub fn set_time(&mut self, t: f32) {
        self.time = t;
    }

    /// The material's bind group, rebuilt when an atlas it names changed.
    fn material_bind(&mut self, index: usize) {
        let gen = self.atlas_gen;
        let Some(Some(m)) = self.materials.get(index) else {
            return;
        };
        if m.bind.as_ref().is_some_and(|b| b.1 == gen) {
            return;
        }
        let white = &self.atlases["rect"].view;
        let views: Vec<&wgpu::TextureView> = m
            .textures
            .iter()
            .map(|t| self.atlases.get(t).map_or(white, |a| &a.view))
            .collect();
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: m.uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&self.sampler),
            },
        ];
        for (k, v) in views.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry {
                binding: 2 + k as u32,
                resource: wgpu::BindingResource::TextureView(v),
            });
        }
        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&m.name),
            layout: &m.layout,
            entries: &entries,
        });
        if let Some(Some(m)) = self.materials.get_mut(index) {
            m.bind = Some((bg, gen));
        }
    }

    fn make_instance_buffer(device: &wgpu::Device, cap: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances"),
            size: (cap * std::mem::size_of::<Instance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Target size in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Resize a window target (no-op offscreen).
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 || (width, height) == (self.width, self.height) {
            return;
        }
        match &mut self.target {
            Target::Surface { surface, config } => {
                config.width = width;
                config.height = height;
                surface.configure(&self.device, config);
            }
            // A new texture on the same device: far cheaper than a new
            // renderer (a mark draws its frame and its clip at two sizes).
            Target::Offscreen { .. } => {
                self.target = Self::offscreen_target(&self.device, width, height);
            }
        }
        self.width = width;
        self.height = height;
    }

    /// Register an atlas from RGBA8 pixels with explicit frame rects,
    /// every pivot at the frame's centre.
    pub fn add_atlas_rgba(
        &mut self,
        name: &str,
        width: u32,
        height: u32,
        rgba: &[u8],
        frames: Vec<[f32; 4]>,
    ) {
        let pivots = vec![[0.5, 0.5]; frames.len()];
        self.add_atlas_frames(name, width, height, rgba, frames, pivots);
    }

    /// Register (or replace) an atlas with frame rects and pivots, on a
    /// texture of its own.
    pub fn add_atlas_frames(
        &mut self,
        name: &str,
        width: u32,
        height: u32,
        rgba: &[u8],
        frames: Vec<[f32; 4]>,
        pivots: Vec<[f32; 2]>,
    ) {
        let page = self.add_page(name, width, height, Texels::Rgba(rgba));
        self.add_atlas_on(name, &page, frames, pivots);
    }

    /// A texture for atlases to share. BC7 blocks go to the GPU as they
    /// are where it reads them, and are decoded here where it does not (a
    /// block this decoder does not read shows magenta).
    pub fn add_page(&mut self, label: &str, width: u32, height: u32, texels: Texels) -> Page {
        let decoded;
        let (format, data, row, rows) = match texels {
            Texels::Rgba(rgba) => (FORMAT, rgba, 4 * width, height),
            Texels::Bc7(blocks) if self.bc => (
                wgpu::TextureFormat::Bc7RgbaUnormSrgb,
                blocks,
                16 * (width / 4),
                height / 4,
            ),
            Texels::Bc7(blocks) => {
                decoded = argus_assets::bc7::decode(width, height, blocks)
                    .unwrap_or_else(|| [255u8, 0, 255, 255].repeat((width * height) as usize));
                (FORMAT, decoded.as_slice(), 4 * width, height)
            }
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(rows),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&Default::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &self.atlas_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.pages += 1;
        Page {
            id: self.pages,
            width,
            height,
            bind_group,
            view,
        }
    }

    /// Register (or replace) an atlas drawn from `page`.
    pub fn add_atlas_on(
        &mut self,
        name: &str,
        page: &Page,
        frames: Vec<[f32; 4]>,
        pivots: Vec<[f32; 2]>,
    ) {
        self.atlas_gen += 1;
        if !self.atlases.contains_key(name) {
            self.atlas_order.push(name.to_string());
        }
        self.atlases.insert(
            name.to_string(),
            Atlas {
                width: page.width,
                height: page.height,
                frames,
                pivots,
                page: page.id,
                bind_group: page.bind_group.clone(),
                view: page.view.clone(),
            },
        );
    }

    /// Forget an atlas (its asset went away).
    pub fn remove_atlas(&mut self, name: &str) -> bool {
        self.atlas_gen += 1;
        self.atlas_order.retain(|n| n != name);
        self.atlases.remove(name).is_some()
    }

    /// An atlas by name.
    pub fn atlas(&self, name: &str) -> Option<&Atlas> {
        self.atlases.get(name)
    }

    /// Atlas names in registration order.
    pub fn atlas_names(&self) -> &[String] {
        &self.atlas_order
    }

    /// Draw one frame: `runs` are runs of `instances`, already ordered,
    /// each with one atlas and one material. The view is rolled by
    /// `view_angle` radians (camera shake). Clear colour is linear RGBA.
    /// `ui`'s runs (of the same instances) are drawn after the post passes,
    /// over everything.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        view_center: [f32; 2],
        view_half: [f32; 2],
        view_angle: f32,
        clear: [f32; 4],
        instances: &[Instance],
        runs: &[Run],
        ui: Option<&UiDraw>,
    ) -> Result<DrawStats> {
        let ui = ui.filter(|u| !u.runs.is_empty());
        let mut used: Vec<usize> = runs
            .iter()
            .chain(ui.iter().flat_map(|u| u.runs.iter().map(|(r, _)| r)))
            .map(|r| r.material)
            .collect();
        used.sort_unstable();
        used.dedup();
        for &m in &used {
            self.material_bind(m);
        }
        if instances.len() > self.instance_cap {
            self.instance_cap = instances.len().next_power_of_two();
            self.instance_buf = Self::make_instance_buffer(&self.device, self.instance_cap);
        }
        if !instances.is_empty() {
            self.queue
                .write_buffer(&self.instance_buf, 0, bytemuck::cast_slice(instances));
        }
        self.queue.write_buffer(
            &self.view_buf,
            0,
            bytemuck::bytes_of(&ViewUniform {
                center: view_center,
                half: view_half,
                pad: [view_angle.cos(), view_angle.sin(), self.time, 0.0],
            }),
        );
        if let Some(u) = ui {
            self.queue.write_buffer(
                &self.view_buf,
                self.ui_view_at as u64,
                bytemuck::bytes_of(&ViewUniform {
                    center: u.center,
                    half: u.half,
                    pad: [1.0, 0.0, self.time, 0.0],
                }),
            );
        }
        let (frame, view) = match &self.target {
            Target::Offscreen { view, .. } => (None, view.clone()),
            Target::Surface { surface, .. } => {
                let f = surface.get_current_texture().map_err(err)?;
                let v = f.texture.create_view(&wgpu::TextureViewDescriptor {
                    format: Some(self.format),
                    ..Default::default()
                });
                (Some(f), v)
            }
        };
        // Seen as a colour-blind eye sees it: the whole frame, the UI too,
        // into a texture of its own, then through the matrix into the
        // target (phase 5 item 2).
        let see_as = self.see_as;
        let (view, target) = match see_as {
            Some(_) => {
                let (w, h) = (self.width, self.height);
                let f = self.view_filter.get_or_insert_with(|| {
                    crate::see_as::ViewFilter::new(&self.device, self.format)
                });
                (f.target(&self.device, w, h), Some(view))
            }
            None => (view, None),
        };
        // With post passes the world goes into the stack's texture first.
        let post_on = !self.post.is_empty();
        let scene = if post_on {
            let (w, h) = (self.width, self.height);
            let stack = self
                .post_stack
                .get_or_insert_with(|| crate::post::PostStack::new(&self.device, self.format));
            stack.scene(&self.device, w, h)
        } else {
            view.clone()
        };
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        let mut stats = DrawStats {
            sprites: instances.len(),
            ..Default::default()
        };
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sprites"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &scene,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear[0] as f64,
                            g: clear[1] as f64,
                            b: clear[2] as f64,
                            a: clear[3] as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &self.view_bind, &[0]);
            pass.set_vertex_buffer(0, self.instance_buf.slice(..));
            let mut bound = std::collections::HashSet::new();
            let mut drawn_with = std::collections::HashSet::new();
            let (mut cur_mat, mut cur_atlas) = (usize::MAX, usize::MAX);
            for r in runs {
                let Some(a) = self
                    .atlas_order
                    .get(r.atlas)
                    .and_then(|n| self.atlases.get(n))
                else {
                    continue;
                };
                // A material that is gone draws with the default.
                let mi = match self.materials.get(r.material) {
                    Some(Some(m)) if m.bind.is_some() => r.material,
                    _ => 0,
                };
                if mi != cur_mat {
                    let Some(Some(m)) = self.materials.get(mi) else {
                        continue;
                    };
                    let Some((bg, _)) = &m.bind else {
                        continue;
                    };
                    pass.set_pipeline(&m.pipeline);
                    pass.set_bind_group(2, bg, &[0]);
                    cur_mat = mi;
                }
                if r.atlas != cur_atlas {
                    pass.set_bind_group(1, &a.bind_group, &[]);
                    cur_atlas = r.atlas;
                }
                pass.draw(0..6, r.start as u32..(r.start + r.count) as u32);
                stats.draw_calls += 1;
                bound.insert(r.atlas);
                drawn_with.insert(mi);
            }
            stats.atlases = bound.len();
            stats.materials = drawn_with.len();
        }
        if post_on {
            if let Some(stack) = &self.post_stack {
                stack.run(
                    &self.device,
                    &self.queue,
                    &mut encoder,
                    &self.post,
                    &view,
                    (self.width, self.height),
                );
            }
        }
        // The UI over the world and its passes: a flash or a shake does not
        // touch it (phase 3 item 1).
        if let Some(u) = ui {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &self.view_bind, &[self.ui_view_at]);
            pass.set_vertex_buffer(0, self.instance_buf.slice(..));
            let (mut cur_mat, mut cur_atlas) = (usize::MAX, usize::MAX);
            for (r, sc) in &u.runs {
                let Some(a) = self
                    .atlas_order
                    .get(r.atlas)
                    .and_then(|n| self.atlases.get(n))
                else {
                    continue;
                };
                let mi = match self.materials.get(r.material) {
                    Some(Some(m)) if m.bind.is_some() => r.material,
                    _ => 0,
                };
                if mi != cur_mat {
                    let Some(Some(m)) = self.materials.get(mi) else {
                        continue;
                    };
                    let Some((bg, _)) = &m.bind else {
                        continue;
                    };
                    pass.set_pipeline(&m.pipeline);
                    pass.set_bind_group(2, bg, &[0]);
                    cur_mat = mi;
                }
                if r.atlas != cur_atlas {
                    pass.set_bind_group(1, &a.bind_group, &[]);
                    cur_atlas = r.atlas;
                }
                let x = sc[0].min(self.width);
                let y = sc[1].min(self.height);
                let w = sc[2].min(self.width - x);
                let h = sc[3].min(self.height - y);
                if w == 0 || h == 0 {
                    continue;
                }
                pass.set_scissor_rect(x, y, w, h);
                pass.draw(0..6, r.start as u32..(r.start + r.count) as u32);
                stats.draw_calls += 1;
            }
        }
        if let (Some(kind), Some(target), Some(f)) = (see_as, &target, &self.view_filter) {
            f.run(&self.device, &self.queue, &mut encoder, target, kind);
        }
        if let Target::Offscreen {
            texture,
            readback,
            padded_row,
            ..
        } = &self.target
        {
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(*padded_row),
                        rows_per_image: Some(self.height),
                    },
                },
                wgpu::Extent3d {
                    width: self.width,
                    height: self.height,
                    depth_or_array_layers: 1,
                },
            );
        }
        self.queue.submit(Some(encoder.finish()));
        if let Some(f) = frame {
            f.present();
        }
        Ok(stats)
    }

    /// Draw the next frames as a colour-blind eye sees them (`None`: as
    /// they are).
    pub fn set_see_as(&mut self, kind: Option<crate::see_as::SeeAs>) {
        self.see_as = kind;
    }

    /// How the frames are seen.
    pub fn see_as(&self) -> Option<crate::see_as::SeeAs> {
        self.see_as
    }

    /// The post passes the next frames run, in order (none: the world is
    /// drawn straight into the target).
    pub fn set_post(&mut self, passes: Vec<crate::post::PostPass>) {
        self.post = passes;
    }

    /// Wait for the GPU to finish what it was given (a measurement's).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wait(&self) {
        self.device.poll(wgpu::Maintain::Wait);
    }

    /// The last offscreen frame as tightly packed RGBA8 rows, top row first.
    pub fn read_pixels(&self) -> Result<Vec<u8>> {
        let mut out = vec![0; (self.width * self.height * 4) as usize];
        self.read_pixels_into(&mut out)?;
        Ok(out)
    }

    /// [`Gpu::read_pixels`] into a buffer of exactly `width * height * 4`
    /// bytes (a frame stream's slot, phase 4 item 1), with no allocation.
    pub fn read_pixels_into(&self, out: &mut [u8]) -> Result<()> {
        let row = (self.width * 4) as usize;
        if out.len() != row * self.height as usize {
            return Err(ArgusError::new(
                crate::codes::GPU,
                format!(
                    "a {}x{} frame needs {} bytes; the buffer has {}",
                    self.width,
                    self.height,
                    row * self.height as usize,
                    out.len()
                ),
            ));
        }
        let Target::Offscreen {
            readback,
            padded_row,
            ..
        } = &self.target
        else {
            return Err(ArgusError::new(
                crate::codes::GPU,
                "pixels can only be read back from an offscreen target",
            ));
        };
        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .map_err(|_| ArgusError::new(crate::codes::GPU, "readback never completed"))?
            .map_err(err)?;
        let data = slice.get_mapped_range();
        for y in 0..self.height as usize {
            let start = y * *padded_row as usize;
            out[y * row..(y + 1) * row].copy_from_slice(&data[start..start + row]);
        }
        drop(data);
        readback.unmap();
        Ok(())
    }

    /// The surface's format (for information).
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }
}
