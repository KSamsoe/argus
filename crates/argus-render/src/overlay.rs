//! A picture drawn over the frame at the bottom of the target, pixel for
//! pixel: the line the window shows while a mark's note is typed (phase 2
//! item 2), before the UI tree exists (phase 3). The picture is made on
//! the CPU with the annotation font (`annotate::status_bar`).

use crate::Image;

const SHADER: &str = r#"
struct Rect { r: vec4<f32> };
@group(0) @binding(0) var<uniform> rect: Rect;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct V {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> V {
    var c = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let p = c[i];
    var v: V;
    v.pos = vec4<f32>(mix(rect.r.x, rect.r.z, p.x), mix(rect.r.y, rect.r.w, p.y), 0.0, 1.0);
    v.uv = vec2<f32>(p.x, 1.0 - p.y);
    return v;
}

@fragment
fn fs(v: V) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, v.uv);
}
"#;

pub(crate) struct Overlay {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    bind: Option<wgpu::BindGroup>,
    size: (u32, u32),
}

impl Overlay {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Overlay {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("overlay"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("overlay"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("overlay"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay"),
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
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay rect"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("overlay"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        Overlay {
            pipeline,
            layout,
            uniform,
            sampler,
            bind: None,
            size: (0, 0),
        }
    }

    /// Show `img`, or nothing.
    pub fn set(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, img: Option<&Image>) {
        let Some(img) = img.filter(|i| i.width > 0 && i.height > 0) else {
            self.bind = None;
            return;
        };
        let size = wgpu::Extent3d {
            width: img.width,
            height: img.height,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("overlay"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &img.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(img.width * 4),
                rows_per_image: Some(img.height),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.bind = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("overlay"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        }));
        self.size = (img.width, img.height);
    }

    /// Place the picture at the bottom left of a `width` by `height`
    /// target, one texel a pixel. Call before the pass that draws it.
    pub fn place(&self, queue: &wgpu::Queue, width: u32, height: u32) {
        if self.bind.is_none() {
            return;
        }
        let (w, h) = (width.max(1) as f32, height.max(1) as f32);
        let r = [
            -1.0f32,
            -1.0,
            -1.0 + 2.0 * self.size.0 as f32 / w,
            -1.0 + 2.0 * self.size.1 as f32 / h,
        ];
        queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&r));
    }

    /// Draw it, if there is one.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(b) = &self.bind {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, b, &[]);
            pass.draw(0..6, 0..1);
        }
    }
}
