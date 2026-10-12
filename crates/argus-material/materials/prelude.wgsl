// The engine's half of every material (argus-material). Instanced sprite
// quads: every instance is a position, size, rotation, atlas rect, tint,
// and four numbers for its material. Culling happens here on the GPU: an
// instance whose bounding circle misses the view collapses to a degenerate
// triangle and rasterises nothing (design section 17, "GPU-side culling").

struct View {
    center: vec2<f32>,
    half: vec2<f32>,
    // cos and sin of the view's roll (camera shake), sim seconds, unused.
    roll: vec4<f32>,
};

@group(0) @binding(0) var<uniform> view: View;
@group(1) @binding(0) var atlas_tex: texture_2d<f32>;
@group(1) @binding(1) var atlas_samp: sampler;

struct Inst {
    @location(0) pos_size: vec4<f32>,
    @location(1) rot_z: vec4<f32>,  // rotation, z, pivot x, pivot y
    @location(2) uv: vec4<f32>,
    @location(3) color: vec4<f32>,
    @location(4) extra: vec4<f32>,  // the material's per-sprite values
    @location(5) tile: vec4<f32>,   // repeats across and down; unused
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) local: vec2<f32>,
    @location(3) world: vec2<f32>,
    @location(4) @interpolate(flat) frame: vec4<f32>,
    @location(5) @interpolate(flat) extra: vec4<f32>,
    // How many times the frame repeats across and down the sprite.
    @location(6) @interpolate(flat) tile: vec2<f32>,
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
    out.extra = inst.extra;
    out.tile = inst.tile.xy;
    // A repeated frame runs on past its rect, from the top left; the
    // fragment brings it back in (tiled_uv).
    out.uv = vec2<f32>(
        inst.uv.x + (c.x + 0.5) * inst.tile.x * inst.uv.z,
        inst.uv.y + (0.5 - c.y) * inst.tile.y * inst.uv.w,
    );
    // A mirrored frame has a negative width; the rect is its bounds.
    let u0 = min(inst.uv.x, inst.uv.x + inst.uv.z);
    let u1 = max(inst.uv.x, inst.uv.x + inst.uv.z);
    out.frame = vec4<f32>(u0, inst.uv.y, u1, inst.uv.y + inst.uv.w);
    out.local = vec2<f32>(c.x + 0.5, c.y + 0.5);

    // Conservative: a pivot at a corner puts the whole quad on one side.
    let radius = length(size);
    let d = abs(pos - view.center);
    let cr = abs(view.roll.x);
    let sr = abs(view.roll.y);
    let reach = vec2<f32>(cr * view.half.x + sr * view.half.y, sr * view.half.x + cr * view.half.y);
    if (d.x > reach.x + radius || d.y > reach.y + radius) {
        out.clip = vec4<f32>(0.0, 0.0, 0.0, 0.0);
        return out;
    }
    let r = inst.rot_z.x;
    let cs = cos(r);
    let sn = sin(r);
    // The pivot (a fraction of the frame, y up) sits on the position.
    let pivot = inst.rot_z.zw;
    let local = vec2<f32>((c.x + 0.5 - pivot.x) * size.x, (c.y + 0.5 - pivot.y) * size.y);
    let world = pos + vec2<f32>(local.x * cs - local.y * sn, local.x * sn + local.y * cs);
    out.world = world;
    let rel = world - view.center;
    let unrolled = vec2<f32>(rel.x * view.roll.x + rel.y * view.roll.y, -rel.x * view.roll.y + rel.y * view.roll.x);
    let ndc = unrolled / view.half;
    out.clip = vec4<f32>(ndc.x, ndc.y, 0.0, 1.0);
    return out;
}

// Where in the atlas a fragment samples: for a repeated frame, the uv
// brought back inside the frame's rect, so the frame tiles the sprite; for
// one drawn once, the uv as it is. No branch: a material may take
// derivatives after it (sdf_text does), and WARP lost its device over one.
fn tiled_uv(v: VOut) -> vec2<f32> {
    let lo = v.frame.xy;
    let size = max(v.frame.zw - v.frame.xy, vec2<f32>(1e-8));
    let r = (v.uv - lo) / size;
    return select(lo + (r - floor(r)) * size, v.uv, v.tile == vec2<f32>(1.0));
}

// What a material's fragment function is given.
struct MaterialInput {
    // Where in the atlas this fragment samples.
    uv: vec2<f32>,
    // Where on the sprite: 0 to 1 across, y up.
    local: vec2<f32>,
    // Where in the world.
    world: vec2<f32>,
    // The sprite's tint, linear RGBA.
    color: vec4<f32>,
    // The sprite's frame in the atlas: u min, v min, u max, v max.
    frame: vec4<f32>,
    // One texel of the atlas, in uv.
    texel: vec2<f32>,
    // Sim seconds.
    time: f32,
};

// The atlas at a uv, held inside the sprite's own frame so a tap beside it
// never reads the next frame. Sampled at level 0, so it may be called
// anywhere, branches included.
fn sample_atlas(in: MaterialInput, uv: vec2<f32>) -> vec4<f32> {
    let lo = in.frame.xy + in.texel * 0.5;
    let hi = max(in.frame.zw - in.texel * 0.5, lo);
    return textureSampleLevel(atlas_tex, atlas_samp, clamp(uv, lo, hi), 0.0);
}

// A linear colour channel as sRGB (0 to 1), and back.
fn srgb_encode(x: f32) -> f32 {
    if (x <= 0.0031308) {
        return x * 12.92;
    }
    return 1.055 * pow(x, 1.0 / 2.4) - 0.055;
}

fn srgb_decode(x: f32) -> f32 {
    if (x <= 0.04045) {
        return x / 12.92;
    }
    return pow((x + 0.055) / 1.055, 2.4);
}
