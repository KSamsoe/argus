// Instanced sprite quads. One draw per atlas; every instance is a
// position, size, rotation, atlas rect, and tint. Culling happens here on
// the GPU: an instance whose bounding circle misses the view collapses to
// a degenerate triangle and rasterises nothing (design section 17,
// "GPU-side culling").

struct View {
    center: vec2<f32>,
    half: vec2<f32>,
    pad: vec4<f32>,
};

@group(0) @binding(0) var<uniform> view: View;
@group(1) @binding(0) var atlas_tex: texture_2d<f32>;
@group(1) @binding(1) var atlas_samp: sampler;

struct Inst {
    @location(0) pos_size: vec4<f32>,
    @location(1) rot_z: vec4<f32>,
    @location(2) uv: vec4<f32>,
    @location(3) color: vec4<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: Inst) -> VOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-0.5, -0.5), vec2<f32>(0.5, -0.5), vec2<f32>(0.5, 0.5),
        vec2<f32>(-0.5, -0.5), vec2<f32>(0.5, 0.5), vec2<f32>(-0.5, 0.5),
    );
    let c = corners[vi];
    let pos = inst.pos_size.xy;
    let size = inst.pos_size.zw;
    var out: VOut;
    out.color = inst.color;
    out.uv = vec2<f32>(inst.uv.x + (c.x + 0.5) * inst.uv.z, inst.uv.y + (0.5 - c.y) * inst.uv.w);

    let radius = 0.5 * length(size);
    let d = abs(pos - view.center);
    if (d.x > view.half.x + radius || d.y > view.half.y + radius) {
        out.clip = vec4<f32>(0.0, 0.0, 0.0, 0.0);
        return out;
    }
    let r = inst.rot_z.x;
    let cs = cos(r);
    let sn = sin(r);
    let local = vec2<f32>(c.x * size.x, c.y * size.y);
    let world = pos + vec2<f32>(local.x * cs - local.y * sn, local.x * sn + local.y * cs);
    let ndc = (world - view.center) / view.half;
    out.clip = vec4<f32>(ndc.x, ndc.y, 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let t = textureSample(atlas_tex, atlas_samp, in.uv);
    return t * in.color;
}
