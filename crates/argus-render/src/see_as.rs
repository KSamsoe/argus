//! A frame as a colour-blind eye sees it (section 12: "a colour-blind
//! simulation post pass (section 13) lets the agent and the human check
//! readability without guessing"; phase 5 item 2). A way of looking, not
//! part of the game: `obs.frame {see_as}`, the editor's viewport, and
//! `Renderer::set_see_as` draw the whole frame, the UI included, into a
//! texture of its own and then through the kind's colour matrix into the
//! target. The matrices are Machado, Oliveira, and Fernandes's (2009) at
//! full severity, in linear RGB; achromatopsia keeps only the luminance.

use std::num::NonZeroU64;

/// A kind of colour vision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeeAs {
    /// No long-wavelength (red) cones.
    Protanopia,
    /// No medium-wavelength (green) cones: the commonest.
    Deuteranopia,
    /// No short-wavelength (blue) cones.
    Tritanopia,
    /// No colour at all.
    Achromatopsia,
}

impl SeeAs {
    /// Every kind, by name.
    pub const ALL: [SeeAs; 4] = [
        SeeAs::Protanopia,
        SeeAs::Deuteranopia,
        SeeAs::Tritanopia,
        SeeAs::Achromatopsia,
    ];

    /// Its name.
    pub fn name(self) -> &'static str {
        match self {
            SeeAs::Protanopia => "protanopia",
            SeeAs::Deuteranopia => "deuteranopia",
            SeeAs::Tritanopia => "tritanopia",
            SeeAs::Achromatopsia => "achromatopsia",
        }
    }

    /// From its name.
    pub fn parse(s: &str) -> Option<SeeAs> {
        SeeAs::ALL.into_iter().find(|k| k.name() == s)
    }

    /// The colour matrix, rows of linear RGB.
    pub fn matrix(self) -> [[f32; 3]; 3] {
        match self {
            SeeAs::Protanopia => [
                [0.152286, 1.052583, -0.204868],
                [0.114503, 0.786281, 0.099216],
                [-0.003882, -0.048116, 1.051998],
            ],
            SeeAs::Deuteranopia => [
                [0.367322, 0.860646, -0.227968],
                [0.280085, 0.672501, 0.047413],
                [-0.011820, 0.042940, 0.968881],
            ],
            SeeAs::Tritanopia => [
                [1.255528, -0.076749, -0.178779],
                [-0.078411, 0.930809, 0.147602],
                [0.004733, 0.691367, 0.303900],
            ],
            SeeAs::Achromatopsia => [[0.2126, 0.7152, 0.0722]; 3],
        }
    }
}

const SHADER: &str = r#"
struct M { r: vec4<f32>, g: vec4<f32>, b: vec4<f32> };
@group(0) @binding(0) var<uniform> m: M;
@group(0) @binding(1) var src: texture_2d<f32>;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let c = textureLoad(src, vec2<i32>(at.xy), 0);
    return vec4<f32>(dot(m.r.xyz, c.rgb), dot(m.g.xyz, c.rgb), dot(m.b.xyz, c.rgb), c.a);
}
"#;

/// The pass and the texture a frame is drawn into first.
pub struct ViewFilter {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    format: wgpu::TextureFormat,
    scene: Option<(u32, u32, wgpu::TextureView)>,
}

impl ViewFilter {
    /// For targets of `format`.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> ViewFilter {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("see as"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        // At an offset, as every uniform is (see gpu.rs `build`: WARP).
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("see as"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: NonZeroU64::new(48),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("see as"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("see as"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("see as"),
            size: 256,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        ViewFilter {
            pipeline,
            layout,
            uniform,
            format,
            scene: None,
        }
    }

    /// The texture a frame of `w` x `h` is drawn into first.
    pub fn target(&mut self, device: &wgpu::Device, w: u32, h: u32) -> wgpu::TextureView {
        if let Some((sw, sh, v)) = &self.scene {
            if (*sw, *sh) == (w, h) {
                return v.clone();
            }
        }
        let t = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("see as scene"),
            size: wgpu::Extent3d {
                width: w.max(1),
                height: h.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let v = t.create_view(&Default::default());
        self.scene = Some((w, h, v.clone()));
        v
    }

    /// The drawn frame through `kind`'s matrix into `out`.
    pub fn run(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        out: &wgpu::TextureView,
        kind: SeeAs,
    ) {
        let Some((_, _, scene)) = &self.scene else {
            return;
        };
        let m = kind.matrix();
        let mut bytes = Vec::with_capacity(48);
        for row in m {
            for x in [row[0], row[1], row[2], 0.0f32] {
                bytes.extend(x.to_le_bytes());
            }
        }
        queue.write_buffer(&self.uniform, 0, &bytes);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("see as"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.uniform,
                        offset: 0,
                        size: NonZeroU64::new(48),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(scene),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("see as"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: out,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind, &[0]);
        pass.draw(0..3, 0..1);
    }
}
