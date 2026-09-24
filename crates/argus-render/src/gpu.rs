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
    bind_group: wgpu::BindGroup,
    view: wgpu::TextureView,
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
    format: wgpu::TextureFormat,
    view_buf: wgpu::Buffer,
    view_bind: wgpu::BindGroup,
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
}

fn err(e: impl std::fmt::Display) -> ArgusError {
    ArgusError::new(crate::codes::GPU, e.to_string())
}

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

impl Gpu {
    #[cfg(not(target_arch = "wasm32"))]
    fn instance(software: bool) -> wgpu::Instance {
        let _ = software;
        wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY | wgpu::Backends::GL,
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
                    required_features: wgpu::Features::empty(),
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
        let target = Target::Offscreen {
            texture,
            view,
            readback,
            padded_row,
        };
        Self::build(adapter, device, queue, FORMAT, target, width, height)
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
        let view_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("view"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                // The fragment half reads the sim time from it.
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
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
        let view_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("view"),
            size: std::mem::size_of::<ViewUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("view"),
            layout: &view_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: view_buf.as_entire_binding(),
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
        let mut gpu = Gpu {
            device,
            queue,
            adapter: format!("{} ({:?})", info.name, info.backend),
            software,
            format,
            view_buf,
            view_bind,
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
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
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
        if let Target::Surface { surface, config } = &mut self.target {
            if width > 0 && height > 0 {
                config.width = width;
                config.height = height;
                surface.configure(&self.device, config);
                self.width = width;
                self.height = height;
            }
        }
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

    /// Register (or replace) an atlas with frame rects and pivots.
    pub fn add_atlas_frames(
        &mut self,
        name: &str,
        width: u32,
        height: u32,
        rgba: &[u8],
        frames: Vec<[f32; 4]>,
        pivots: Vec<[f32; 2]>,
    ) {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(name),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
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
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&Default::default());
        self.atlas_gen += 1;
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(name),
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
        if !self.atlases.contains_key(name) {
            self.atlas_order.push(name.to_string());
        }
        self.atlases.insert(
            name.to_string(),
            Atlas {
                width,
                height,
                frames,
                pivots,
                bind_group,
                view,
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
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        view_center: [f32; 2],
        view_half: [f32; 2],
        view_angle: f32,
        clear: [f32; 4],
        instances: &[Instance],
        runs: &[Run],
    ) -> Result<DrawStats> {
        let mut used: Vec<usize> = runs.iter().map(|r| r.material).collect();
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
                    view: &view,
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
            pass.set_bind_group(0, &self.view_bind, &[]);
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
                    pass.set_bind_group(2, bg, &[]);
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

    /// The last offscreen frame as tightly packed RGBA8 rows, top row first.
    pub fn read_pixels(&self) -> Result<Vec<u8>> {
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
        let row = (self.width * 4) as usize;
        let mut out = Vec::with_capacity(row * self.height as usize);
        for y in 0..self.height as usize {
            let start = y * *padded_row as usize;
            out.extend_from_slice(&data[start..start + row]);
        }
        drop(data);
        readback.unmap();
        Ok(out)
    }

    /// The surface's format (for information).
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }
}
