// Text from a distance field (phase 3 item 1): the atlas holds how far each
// texel is from a glyph's edge, 0.5 on it, so the edge is found between
// texels at any size and stays sharp. The atlas's sampler is nearest (for
// pixel art), so the four texels around are blended here.
fn material(in: MaterialInput, p: Params) -> vec4<f32> {
    let t = in.uv / in.texel - 0.5;
    let f = fract(t);
    let base = (floor(t) + 0.5) * in.texel;
    let a = sample_atlas(in, base).a;
    let b = sample_atlas(in, base + vec2<f32>(in.texel.x, 0.0)).a;
    let c = sample_atlas(in, base + vec2<f32>(0.0, in.texel.y)).a;
    let d = sample_atlas(in, base + in.texel).a;
    let dist = mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
    // How much the field changes across a screen pixel: the blend is that
    // wide, so the edge is one pixel soft at any size.
    let w = max(fwidth(dist), 0.0001) * 0.5 * p.softness;
    let alpha = smoothstep(p.edge - w, p.edge + w, dist);
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
