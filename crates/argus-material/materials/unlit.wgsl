// The sprite's frame times its tint: what a sprite without a material draws.
fn material(in: MaterialInput, p: Params) -> vec4<f32> {
    return sample_atlas(in, in.uv) * in.color;
}
