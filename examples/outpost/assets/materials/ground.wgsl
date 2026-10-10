// Grass repeated across a valley of any size: the tile once every
// p.cells world units, however big the sprite is.
fn material(in: MaterialInput, p: Params) -> vec4<f32> {
    let at = in.local * p.size / p.cells;
    return sample_tile(vec2<f32>(at.x, 1.0 - at.y));
}
