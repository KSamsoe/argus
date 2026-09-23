// Lit by an ambient colour and one point light that fades to nothing at
// its radius.
fn material(in: MaterialInput, p: Params) -> vec4<f32> {
    let c = sample_atlas(in, in.uv) * in.color;
    let d = distance(in.world, p.light);
    let k = clamp(1.0 - d / max(p.radius, 0.0001), 0.0, 1.0);
    let light = p.ambient.rgb + p.light_color.rgb * (k * k * p.intensity);
    return vec4<f32>(c.rgb * light, c.a);
}
