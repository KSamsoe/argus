//! 2D lighting on the GPU (§12, phase 5 item 3). With lighting on, the world
//! is drawn into the post stack's texture as with a post pass; then:
//!
//! 1. A light map the size of the target is cleared to the ambient light.
//! 2. Each light is added into it, a quad over its radius with its falloff
//!    and cone. A light that casts shadows first draws its shadow (the
//!    triangles argus-light swept from the edges facing away from it) into
//!    the stencil with its own number, and is then drawn only where the
//!    stencil does not hold that number: up to 255 lights a pass, no clear
//!    between them.
//! 3. The picture is multiplied by the light map (the composite), before
//!    the post passes, so a vignette or a flash sees the lit picture.
//! 4. What glows is drawn over that, unlit: sprites with Emissive, and
//!    everything drawn with an emissive material.
//!
//! A normal-mapped sprite (phase 5 item 3, "normal-mapped sprites
//! optional"): when one is drawn, the world's sprites are drawn again into
//! a normal buffer first, each sprite's normals turned and mirrored with
//! it (flat, and saying so, for a sprite without a map), and each light
//! reads it: a pixel facing the light is brighter than a flat one, a pixel
//! facing away darker, by how high the light is over the picture.
//!
//! The light map is half floats where the device can draw and blend them,
//! so lights add past white; else sRGB bytes holding half the light, so it
//! still reaches twice white and keeps its dark steps fine. With lighting
//! off none of this exists: no texture, no pipeline, no pass.

use crate::gpu::Run;

/// One light as the shader reads it (48 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightInstance {
    /// World x, y, radius, falloff exponent.
    pub pos_radius: [f32; 4],
    /// Linear colour times intensity (and flicker), and the height above
    /// the picture (normal maps).
    pub color: [f32; 4],
    /// The cone: direction (cos, sin), and the cosines of its outer and
    /// inner edges; an outer cosine of -1 or less is a point light.
    pub cone: [f32; 4],
}

/// What one frame lights with.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LightFrame {
    /// The ambient light, linear.
    pub ambient: [f32; 3],
    /// The lights in reach.
    pub lights: Vec<LightInstance>,
    /// Each light's shadow: a range of `shadow` (start, count), empty for
    /// a light that casts none.
    pub shadow_of: Vec<(u32, u32)>,
    /// Every shadow's triangles, world positions.
    pub shadow: Vec<[f32; 2]>,
    /// What glows: runs of the frame's instances drawn after the light.
    pub glow: Vec<Run>,
}

const SHADER: &str = r#"
struct View {
    center: vec2<f32>,
    half: vec2<f32>,
    roll: vec4<f32>,
};
@group(0) @binding(0) var<uniform> view: View;

fn to_clip(world: vec2<f32>) -> vec4<f32> {
    let rel = world - view.center;
    let unrolled = vec2<f32>(rel.x * view.roll.x + rel.y * view.roll.y, -rel.x * view.roll.y + rel.y * view.roll.x);
    return vec4<f32>(unrolled / view.half, 0.0, 1.0);
}

@vertex
fn vs_shadow(@location(0) p: vec2<f32>) -> @builtin(position) vec4<f32> {
    return to_clip(p);
}

@fragment
fn fs_shadow() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0);
}

struct LightIn {
    @location(0) pos_radius: vec4<f32>,
    @location(1) color: vec4<f32>,
    @location(2) cone: vec4<f32>,
};

struct LightOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec2<f32>,
    @location(1) @interpolate(flat) pos_radius: vec4<f32>,
    @location(2) @interpolate(flat) color: vec4<f32>,
    @location(3) @interpolate(flat) cone: vec4<f32>,
};

@vertex
fn vs_light(@builtin(vertex_index) vi: u32, l: LightIn) -> LightOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    var o: LightOut;
    o.world = l.pos_radius.xy + corners[vi] * l.pos_radius.z;
    o.clip = to_clip(o.world);
    o.pos_radius = l.pos_radius;
    o.color = l.color;
    o.cone = l.cone;
    return o;
}

fn reach(i: LightOut) -> f32 {
    let d = i.world - i.pos_radius.xy;
    let dist = length(d);
    var k = pow(clamp(1.0 - dist / max(i.pos_radius.z, 0.0001), 0.0, 1.0), i.pos_radius.w);
    if (i.cone.z > -1.0) {
        let c = dot(d / max(dist, 0.00001), i.cone.xy);
        k = k * smoothstep(i.cone.z, max(i.cone.w, i.cone.z + 0.0001), c);
    }
    return k;
}

@fragment
fn fs_light(i: LightOut) -> @location(0) vec4<f32> {
    return vec4<f32>(i.color.rgb * reach(i), 1.0);
}

// The normal buffer: a world-space normal (y up) as colour, alpha 1 where
// a normal-mapped sprite drew it.
@group(1) @binding(0) var normal_buf: texture_2d<f32>;

@fragment
fn fs_light_n(i: LightOut) -> @location(0) vec4<f32> {
    var k = reach(i);
    let n = textureLoad(normal_buf, vec2<i32>(i.clip.xy), 0);
    if (n.a > 0.5) {
        let nv = normalize(n.xyz * 2.0 - vec3<f32>(1.0));
        // Toward the light, which stands `height` over the picture.
        let l = normalize(vec3<f32>(i.pos_radius.xy - i.world, max(i.color.w, 0.0001)));
        // As bright as a flat pixel when the normal is flat.
        k = k * clamp(dot(nv, l) / max(l.z, 0.2), 0.0, 2.0);
    }
    return vec4<f32>(i.color.rgb * k, 1.0);
}

struct Comp {
    k: vec4<f32>,
};
@group(0) @binding(0) var<uniform> comp: Comp;
@group(0) @binding(1) var scene_tex: texture_2d<f32>;
@group(0) @binding(2) var light_tex: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;

struct V {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> V {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    var v: V;
    v.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    v.uv = vec2<f32>(x, y);
    return v;
}

@fragment
fn fs_composite(v: V) -> @location(0) vec4<f32> {
    let c = textureSampleLevel(scene_tex, samp, v.uv, 0.0);
    let l = textureSampleLevel(light_tex, samp, v.uv, 0.0).rgb * comp.k.x;
    return vec4<f32>(c.rgb * l, c.a);
}
"#;

/// The world's sprites again, writing their normals (a separate module:
/// its bindings are the sprites').
const NORMAL_SHADER: &str = r#"
struct View {
    center: vec2<f32>,
    half: vec2<f32>,
    roll: vec4<f32>,
};
@group(0) @binding(0) var<uniform> view: View;
@group(1) @binding(0) var atlas_tex: texture_2d<f32>;
@group(1) @binding(1) var atlas_samp: sampler;
@group(2) @binding(0) var normal_tex: texture_2d<f32>;
@group(2) @binding(1) var normal_samp: sampler;

struct Inst {
    @location(0) pos_size: vec4<f32>,
    @location(1) rot_z: vec4<f32>,
    @location(2) uv: vec4<f32>,
    @location(3) color: vec4<f32>,
    @location(4) extra: vec4<f32>,
};

struct NOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // cos and sin of the sprite's rotation; its mirroring in x and y.
    @location(1) @interpolate(flat) turn: vec4<f32>,
};

@vertex
fn vs_normal(@builtin(vertex_index) vi: u32, inst: Inst) -> NOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-0.5, -0.5), vec2<f32>(0.5, -0.5), vec2<f32>(0.5, 0.5),
        vec2<f32>(-0.5, -0.5), vec2<f32>(0.5, 0.5), vec2<f32>(-0.5, 0.5),
    );
    let c = corners[vi];
    let size = inst.pos_size.zw;
    var o: NOut;
    o.uv = vec2<f32>(inst.uv.x + (c.x + 0.5) * inst.uv.z, inst.uv.y + (0.5 - c.y) * inst.uv.w);
    let r = inst.rot_z.x;
    let cs = cos(r);
    let sn = sin(r);
    let pivot = inst.rot_z.zw;
    let local = vec2<f32>((c.x + 0.5 - pivot.x) * size.x, (c.y + 0.5 - pivot.y) * size.y);
    let world = inst.pos_size.xy + vec2<f32>(local.x * cs - local.y * sn, local.x * sn + local.y * cs);
    let rel = world - view.center;
    let unrolled = vec2<f32>(rel.x * view.roll.x + rel.y * view.roll.y, -rel.x * view.roll.y + rel.y * view.roll.x);
    o.clip = vec4<f32>(unrolled / view.half, 0.0, 1.0);
    let fx = select(1.0, -1.0, (inst.uv.z < 0.0) != (size.x < 0.0));
    let fy = select(1.0, -1.0, size.y < 0.0);
    o.turn = vec4<f32>(cs, sn, fx, fy);
    return o;
}

@fragment
fn fs_normal(v: NOut) -> @location(0) vec4<f32> {
    let base = textureSampleLevel(atlas_tex, atlas_samp, v.uv, 0.0);
    if (base.a < 0.5) {
        discard;
    }
    let s = textureSampleLevel(normal_tex, normal_samp, v.uv, 0.0);
    var n = s.xyz * 2.0 - vec3<f32>(1.0);
    n.x = n.x * v.turn.z;
    n.y = n.y * v.turn.w;
    let r = vec2<f32>(n.x * v.turn.x - n.y * v.turn.y, n.x * v.turn.y + n.y * v.turn.x);
    return vec4<f32>(vec3<f32>(r, n.z) * 0.5 + vec3<f32>(0.5), s.a);
}
"#;

const STENCIL: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;

/// The normal buffer's format: normals are not colours.
pub(crate) const NORMALS: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Uniform bytes the composite takes, padded for the offset alignment.
const UNIFORM: u64 = 256;

pub(crate) struct LightPass {
    /// The light map's format, and what it holds of the light.
    format: wgpu::TextureFormat,
    scale: f32,
    shadow: wgpu::RenderPipeline,
    lit: wgpu::RenderPipeline,
    lit_shadowed: wgpu::RenderPipeline,
    lit_n: wgpu::RenderPipeline,
    lit_shadowed_n: wgpu::RenderPipeline,
    normal: wgpu::RenderPipeline,
    nbuf_layout: wgpu::BindGroupLayout,
    composite: wgpu::RenderPipeline,
    comp_layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    /// The light map, the stencil, and the normal buffer, at a size.
    targets: Option<(
        (u32, u32),
        wgpu::TextureView,
        wgpu::TextureView,
        wgpu::TextureView,
    )>,
    lights: wgpu::Buffer,
    lights_cap: usize,
    verts: wgpu::Buffer,
    verts_cap: usize,
}

fn stencil_state(
    compare: wgpu::CompareFunction,
    pass: wgpu::StencilOperation,
) -> wgpu::DepthStencilState {
    let face = wgpu::StencilFaceState {
        compare,
        fail_op: wgpu::StencilOperation::Keep,
        depth_fail_op: wgpu::StencilOperation::Keep,
        pass_op: pass,
    };
    wgpu::DepthStencilState {
        format: STENCIL,
        depth_write_enabled: false,
        depth_compare: wgpu::CompareFunction::Always,
        stencil: wgpu::StencilState {
            front: face,
            back: face,
            read_mask: 0xff,
            write_mask: 0xff,
        },
        bias: wgpu::DepthBiasState::default(),
    }
}

fn buffer(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size.max(16),
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl LightPass {
    pub fn new(
        device: &wgpu::Device,
        f16: bool,
        view_layout: &wgpu::BindGroupLayout,
        atlas_layout: &wgpu::BindGroupLayout,
        target: wgpu::TextureFormat,
    ) -> LightPass {
        // Half floats where the device draws into them and blends them.
        let (format, scale) = if f16 {
            (wgpu::TextureFormat::Rgba16Float, 1.0)
        } else {
            (wgpu::TextureFormat::Rgba8UnormSrgb, 0.5)
        };
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("light"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let world_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("light"),
            bind_group_layouts: &[view_layout],
            push_constant_ranges: &[],
        });
        let shadow = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("light shadow"),
            layout: Some(&world_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_shadow"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_shadow"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::empty(),
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(stencil_state(
                wgpu::CompareFunction::Always,
                wgpu::StencilOperation::Replace,
            )),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let add = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let nbuf_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("normal buffer"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let normals_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("light normals"),
            bind_group_layouts: &[view_layout, &nbuf_layout],
            push_constant_ranges: &[],
        });
        let light = |label: &str, compare: wgpu::CompareFunction, normals: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(if normals { &normals_layout } else { &world_layout }),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_light"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<LightInstance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4],
                    }],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(if normals { "fs_light_n" } else { "fs_light" }),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(add),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(stencil_state(compare, wgpu::StencilOperation::Keep)),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let lit = light("light", wgpu::CompareFunction::Always, false);
        let lit_shadowed = light("light shadowed", wgpu::CompareFunction::NotEqual, false);
        let lit_n = light("light normals", wgpu::CompareFunction::Always, true);
        let lit_shadowed_n = light(
            "light shadowed normals",
            wgpu::CompareFunction::NotEqual,
            true,
        );
        let nmodule = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("normals"),
            source: wgpu::ShaderSource::Wgsl(NORMAL_SHADER.into()),
        });
        let npl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("normals"),
            bind_group_layouts: &[view_layout, atlas_layout, atlas_layout],
            push_constant_ranges: &[],
        });
        let normal = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("normals"),
            layout: Some(&npl),
            vertex: wgpu::VertexState {
                module: &nmodule,
                entry_point: Some("vs_normal"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<crate::gpu::Instance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &nmodule,
                entry_point: Some("fs_normal"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: NORMALS,
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
        let comp_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("light composite"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    // At an offset, as every uniform here (gpu.rs `build`).
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let comp_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("light composite"),
            bind_group_layouts: &[&comp_layout],
            push_constant_ranges: &[],
        });
        let composite = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("light composite"),
            layout: Some(&comp_pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_full"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_composite"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target,
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
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("light composite"),
            size: UNIFORM,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("light"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        LightPass {
            format,
            scale,
            shadow,
            lit,
            lit_shadowed,
            lit_n,
            lit_shadowed_n,
            normal,
            nbuf_layout,
            composite,
            comp_layout,
            uniform,
            sampler,
            targets: None,
            lights: buffer(device, "lights", 64 * 48),
            lights_cap: 64,
            verts: buffer(device, "shadows", 4096 * 8),
            verts_cap: 4096,
        }
    }

    /// The light map's format (half floats, or sRGB bytes at half).
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// The pipeline the normal pass draws the sprites with.
    pub fn normal_pipeline(&self) -> &wgpu::RenderPipeline {
        &self.normal
    }

    /// The normal buffer at this size, for the normal pass to draw into.
    pub fn normal_target(&mut self, device: &wgpu::Device, w: u32, h: u32) -> wgpu::TextureView {
        self.targets(device, w, h);
        self.targets.as_ref().expect("made").3.clone()
    }

    /// The light map, the stencil, and the normal buffer, made (again) at
    /// this size.
    fn targets(&mut self, device: &wgpu::Device, w: u32, h: u32) {
        if self.targets.as_ref().map(|t| t.0) == Some((w, h)) {
            return;
        }
        let make = |format, label| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: w.max(1),
                        height: h.max(1),
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        self.targets = Some((
            (w, h),
            make(self.format, "light map"),
            make(STENCIL, "light stencil"),
            make(NORMALS, "normal buffer"),
        ));
    }

    /// Light the scene: the light map from `frame`, then the scene times
    /// the light into `out`. The view is bound at offset 0 of `view_bind`.
    /// With `normals`, each light reads the normal buffer the normal pass
    /// drew this frame.
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view_bind: &wgpu::BindGroup,
        frame: &LightFrame,
        scene: &wgpu::TextureView,
        out: &wgpu::TextureView,
        (w, h): (u32, u32),
        normals: bool,
    ) {
        self.targets(device, w, h);
        let s = self.scale;
        // The lights, their colours at the map's scale.
        let lights: Vec<LightInstance> = frame
            .lights
            .iter()
            .map(|l| {
                let mut l = *l;
                l.color = [l.color[0] * s, l.color[1] * s, l.color[2] * s, l.color[3]];
                l
            })
            .collect();
        if lights.len() > self.lights_cap {
            self.lights_cap = lights.len().next_power_of_two();
            self.lights = buffer(device, "lights", (self.lights_cap * 48) as u64);
        }
        if !lights.is_empty() {
            queue.write_buffer(&self.lights, 0, bytemuck::cast_slice(&lights));
        }
        if frame.shadow.len() > self.verts_cap {
            self.verts_cap = frame.shadow.len().next_power_of_two();
            self.verts = buffer(device, "shadows", (self.verts_cap * 8) as u64);
        }
        if !frame.shadow.is_empty() {
            queue.write_buffer(&self.verts, 0, bytemuck::cast_slice(&frame.shadow));
        }
        let (_, map, stencil, nbuf) = self.targets.as_ref().expect("made");
        let nbind = normals.then(|| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("normal buffer"),
                layout: &self.nbuf_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(nbuf),
                }],
            })
        });
        let (lit, lit_shadowed) = if normals {
            (&self.lit_n, &self.lit_shadowed_n)
        } else {
            (&self.lit, &self.lit_shadowed)
        };
        // 255 shadowed lights a pass: a light's shadow is its own number
        // in the stencil, so none needs clearing until the numbers run out.
        let mut first = true;
        let mut i = 0;
        while first || i < lights.len() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("light map"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: map,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if first {
                            wgpu::LoadOp::Clear(wgpu::Color {
                                r: (frame.ambient[0] * s) as f64,
                                g: (frame.ambient[1] * s) as f64,
                                b: (frame.ambient[2] * s) as f64,
                                a: 1.0,
                            })
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: stencil,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0),
                        store: wgpu::StoreOp::Discard,
                    }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            first = false;
            pass.set_bind_group(0, view_bind, &[0]);
            if let Some(b) = &nbind {
                pass.set_bind_group(1, b, &[]);
            }
            let mut number = 0u32;
            while i < lights.len() {
                let (start, count) = frame.shadow_of.get(i).copied().unwrap_or((0, 0));
                if count > 0 {
                    if number == 255 {
                        break;
                    }
                    number += 1;
                    pass.set_stencil_reference(number);
                    pass.set_pipeline(&self.shadow);
                    pass.set_vertex_buffer(0, self.verts.slice(..));
                    pass.draw(start..start + count, 0..1);
                    pass.set_pipeline(lit_shadowed);
                } else {
                    pass.set_pipeline(lit);
                }
                pass.set_vertex_buffer(0, self.lights.slice(..));
                pass.draw(0..6, i as u32..i as u32 + 1);
                i += 1;
            }
        }
        // The scene times the light.
        let mut k = [0f32; 4];
        k[0] = 1.0 / s;
        queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&k));
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("light composite"),
            layout: &self.comp_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.uniform,
                        offset: 0,
                        size: std::num::NonZeroU64::new(16),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(scene),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(map),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("light composite"),
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
        pass.set_pipeline(&self.composite);
        pass.set_bind_group(0, &bind, &[0]);
        pass.draw(0..3, 0..1);
    }
}
