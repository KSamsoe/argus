// Breaks the sprite up into value noise as amount goes from 0 (whole) to
// 1 (gone), with a glowing edge where it breaks.
fn dissolve_hash(q: vec2<f32>) -> f32 {
    return fract(sin(dot(q, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn dissolve_noise(x: vec2<f32>) -> f32 {
    let i = floor(x);
    let f = fract(x);
    let u = f * f * (3.0 - 2.0 * f);
    let a = dissolve_hash(i);
    let b = dissolve_hash(i + vec2<f32>(1.0, 0.0));
    let c = dissolve_hash(i + vec2<f32>(0.0, 1.0));
    let d = dissolve_hash(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn material(in: MaterialInput, p: Params) -> vec4<f32> {
    let c = sample_atlas(in, in.uv) * in.color;
    let n = dissolve_noise(in.local * p.scale);
    // The cut runs from below every value (nothing gone) to above every
    // value (everything gone), the edge just ahead of it.
    let cut = p.amount * (1.0 + p.edge);
    if (n < cut - p.edge) {
        discard;
    }
    if (n < cut) {
        return vec4<f32>(p.edge_color.rgb, c.a);
    }
    return c;
}
