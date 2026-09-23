// A line of colour around the sprite's opaque pixels, drawn on the clear
// pixels next to them.
fn material(in: MaterialInput, p: Params) -> vec4<f32> {
    let c = sample_atlas(in, in.uv) * in.color;
    if (c.a >= 0.5) {
        return c;
    }
    let r = in.texel * p.width;
    var near = 0.0;
    for (var i = 0; i < 8; i = i + 1) {
        let a = f32(i) * 0.7853982;
        let o = vec2<f32>(cos(a), sin(a)) * r;
        near = max(near, sample_atlas(in, in.uv + o).a);
    }
    if (near >= 0.5) {
        return p.color;
    }
    return c;
}
