//! The post stack on the GPU (§13, phase 2 item 6): the world is drawn into
//! a texture, then each pass reads the one before and writes the next, the
//! last into the target; a full-screen triangle and one small fragment
//! shader each. The passes come from the drawing camera's post components
//! (argus-post), built by the renderer each frame; with none, the world is
//! drawn straight into the target and this costs nothing.

/// Which pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PostKind {
    /// The picture moves: `a.xy` the offset, a share of the screen's height.
    Shake,
    /// Red and blue apart toward the edges: `a.x` at the corners.
    Aberration,
    /// The palette colour for each pixel's brightness: `a.x` how much,
    /// `a.y` how many colours, `palette` dark to light.
    Palette,
    /// Toward a colour: `a` the colour (linear), `b.x` how much.
    Flash,
    /// Darker toward the edges: `a.x` how dark, `a.y` where it starts,
    /// `a.z` how gradual, shares of the screen's height.
    Vignette,
    /// Toward a colour, as `Flash`.
    Fade,
}

impl PostKind {
    const ALL: [PostKind; 6] = [
        PostKind::Shake,
        PostKind::Aberration,
        PostKind::Palette,
        PostKind::Flash,
        PostKind::Vignette,
        PostKind::Fade,
    ];

    fn entry(self) -> &'static str {
        match self {
            PostKind::Shake => "shake",
            PostKind::Aberration => "aberration",
            PostKind::Palette => "palette",
            PostKind::Flash => "flash",
            PostKind::Vignette => "vignette",
            PostKind::Fade => "fade",
        }
    }

    /// The pass's name, as the semantic frame says it.
    pub fn name(self) -> &'static str {
        self.entry()
    }
}

/// One pass as the GPU runs it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PostPass {
    /// Which.
    pub kind: PostKind,
    /// Place in the stack: lower runs first.
    pub order: i32,
    /// Parameters, by kind.
    pub a: [f32; 4],
    /// More parameters, by kind.
    pub b: [f32; 4],
    /// A palette's colours, linear, dark to light.
    pub palette: [[f32; 4]; 8],
}

/// Passes one frame can run.
pub const MAX_PASSES: usize = 8;

const SHADER: &str = r#"
struct P {
    a: vec4<f32>,
    b: vec4<f32>,
    size: vec4<f32>,
    pal: array<vec4<f32>, 8>,
};
@group(0) @binding(0) var<uniform> p: P;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct V {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> V {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    var v: V;
    v.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    v.uv = vec2<f32>(x, y);
    return v;
}

fn at(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(src, samp, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
}

// A share of the screen's height (y up) as a texture offset (y down).
fn share(v: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(v.x * p.size.y * p.size.z, -v.y);
}

@fragment
fn shake(v: V) -> @location(0) vec4<f32> {
    return at(v.uv - share(p.a.xy));
}

@fragment
fn aberration(v: V) -> @location(0) vec4<f32> {
    let d = (v.uv - vec2<f32>(0.5)) * 2.0;
    let o = vec2<f32>(d.x * p.size.y * p.size.z, d.y) * p.a.x;
    let c = at(v.uv);
    return vec4<f32>(at(v.uv + o).r, c.g, at(v.uv - o).b, c.a);
}

@fragment
fn palette(v: V) -> @location(0) vec4<f32> {
    let c = at(v.uv);
    let n = max(i32(p.a.y), 1);
    let l = pow(clamp(dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722)), 0.0, 1.0), 1.0 / 2.2);
    let k = clamp(i32(l * f32(n)), 0, n - 1);
    return vec4<f32>(mix(c.rgb, p.pal[k].rgb, p.a.x), c.a);
}

@fragment
fn flash(v: V) -> @location(0) vec4<f32> {
    let c = at(v.uv);
    return vec4<f32>(mix(c.rgb, p.a.rgb, p.b.x), c.a);
}

@fragment
fn vignette(v: V) -> @location(0) vec4<f32> {
    let c = at(v.uv);
    let d = length((v.uv - vec2<f32>(0.5)) * vec2<f32>(p.size.x * p.size.w, 1.0));
    let k = smoothstep(p.a.y, p.a.y + max(p.a.z, 0.0001), d);
    return vec4<f32>(c.rgb * (1.0 - p.a.x * k), c.a);
}

@fragment
fn fade(v: V) -> @location(0) vec4<f32> {
    let c = at(v.uv);
    return vec4<f32>(mix(c.rgb, p.a.rgb, p.b.x), c.a);
}
"#;

/// Uniform bytes a pass takes (176, padded for the offset alignment).
const UNIFORM: u64 = 256;

pub(crate) struct PostStack {
    format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipelines: Vec<(PostKind, wgpu::RenderPipeline)>,
    uniforms: Vec<wgpu::Buffer>,
    targets: Option<((u32, u32), [wgpu::TextureView; 2])>,
}

impl PostStack {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> PostStack {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("post"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    // At an offset (a root constant buffer on DirectX 12):
                    // through the descriptor heap, WARP gave every pass the
                    // last pass's numbers (see gpu.rs `build`).
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
            label: Some("post"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipelines = PostKind::ALL
            .iter()
            .map(|&k| {
                let p = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(k.entry()),
                    layout: Some(&pl),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some("vs"),
                        buffers: &[],
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some(k.entry()),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: None,
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
                (k, p)
            })
            .collect();
        let uniforms = (0..MAX_PASSES)
            .map(|_| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("post params"),
                    size: UNIFORM,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            })
            .collect();
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("post"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        PostStack {
            format,
            layout,
            sampler,
            pipelines,
            uniforms,
            targets: None,
        }
    }

    /// The texture the world is drawn into, made (again) at this size.
    pub fn scene(&mut self, device: &wgpu::Device, w: u32, h: u32) -> wgpu::TextureView {
        if self.targets.as_ref().map(|(s, _)| *s) != Some((w, h)) {
            let make = || {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some("post target"),
                        size: wgpu::Extent3d {
                            width: w.max(1),
                            height: h.max(1),
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: self.format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    })
                    .create_view(&wgpu::TextureViewDescriptor::default())
            };
            self.targets = Some(((w, h), [make(), make()]));
        }
        self.targets.as_ref().expect("made").1[0].clone()
    }

    /// Run the passes from the scene texture into `out`.
    pub fn run(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        passes: &[PostPass],
        out: &wgpu::TextureView,
        (w, h): (u32, u32),
    ) {
        let Some((_, views)) = &self.targets else {
            return;
        };
        let n = passes.len().min(MAX_PASSES);
        for (i, p) in passes.iter().take(n).enumerate() {
            let (src, last) = (&views[i % 2], i + 1 == n);
            let dst = if last { out } else { &views[(i + 1) % 2] };
            let mut bytes: Vec<f32> = Vec::with_capacity(44);
            bytes.extend(p.a);
            bytes.extend(p.b);
            bytes.extend([
                w as f32,
                h as f32,
                1.0 / w.max(1) as f32,
                1.0 / h.max(1) as f32,
            ]);
            for c in p.palette {
                bytes.extend(c);
            }
            queue.write_buffer(&self.uniforms[i], 0, bytemuck::cast_slice(&bytes));
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("post"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniforms[i].as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(src),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            let pipeline = &self
                .pipelines
                .iter()
                .find(|(k, _)| *k == p.kind)
                .expect("every kind has a pipeline")
                .1;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(p.kind.entry()),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dst,
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
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[0]);
            pass.draw(0..3, 0..1);
        }
    }
}
