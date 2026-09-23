// Recolours indexed art: the red channel of each texel, as the byte it was
// drawn with, picks a column of the palette texture, and the row picks the
// palette. Greyscale art with as many shades as the palette has columns
// maps shade to column.
fn material(in: MaterialInput, p: Params) -> vec4<f32> {
    let c = sample_atlas(in, in.uv);
    if (c.a <= 0.0) {
        return vec4<f32>(0.0);
    }
    let w = max(p.palette_size.x, 1.0);
    let h = max(p.palette_size.y, 1.0);
    let byte = round(srgb_encode(c.r) * 255.0);
    let col = min(floor(byte * w / 256.0), w - 1.0);
    let row = clamp(floor(p.row), 0.0, h - 1.0);
    let pc = sample_palette(vec2<f32>((col + 0.5) / w, (row + 0.5) / h));
    return vec4<f32>(pc.rgb, pc.a * c.a) * in.color;
}
