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
    // wide, so the edge is one pixel soft at any size. It is the field's
    // slope here (the four texels' blend, per texel) times the texels a
    // pixel spans, not fwidth(dist): two pixels either side of a stem
    // thinner than they are apart read the field rising and falling again
    // between them, fwidth saw no change, the blend closed to a hard edge,
    // and a stem both read just under the edge vanished (Outpost's HUD at
    // 640 x 360 lost the middle of the t in "colonists").
    let slope = vec2<f32>(mix(b - a, d - c, f.y), mix(c - a, d - b, f.x));
    let change = abs(dot(slope, dpdx(t))) + abs(dot(slope, dpdy(t)));
    let w = max(change, 0.0001) * 0.5 * p.softness;
    let alpha = smoothstep(p.edge - w, p.edge + w, dist);
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
