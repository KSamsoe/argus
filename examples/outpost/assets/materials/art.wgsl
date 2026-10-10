// Outpost's art: the sprite's frame as it is drawn, without the tint.
// The colony's systems still give its entities their old shapes' colours
// (a tree's colour says standing or stump); the art, not the colour, is what
// is seen.
fn material(in: MaterialInput, p: Params) -> vec4<f32> {
    return sample_atlas(in, in.uv);
}
