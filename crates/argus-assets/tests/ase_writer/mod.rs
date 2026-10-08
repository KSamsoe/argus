//! A small Aseprite file writer for tests, following Aseprite's
//! `docs/ase-file-specs.md`, since there is no Aseprite here. The importer's
//! tests write files with it, and argus-player's release test includes it
//! by path. It needs no compression crate: `stored_zlib` wraps bytes in a
//! zlib stream of stored blocks, which any inflater reads.

#![allow(dead_code)]

/// A layer's flags.
pub const VISIBLE: u16 = 1;
/// The background layer.
pub const BACKGROUND: u16 = 8;
/// A reference layer.
pub const REFERENCE: u16 = 64;

/// Little-endian bytes.
#[derive(Default)]
pub struct Bytes(pub Vec<u8>);

impl Bytes {
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    pub fn i16(&mut self, v: i16) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    pub fn i32(&mut self, v: i32) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    pub fn zeros(&mut self, n: usize) -> &mut Self {
        self.0.extend(std::iter::repeat_n(0u8, n));
        self
    }
    pub fn raw(&mut self, b: &[u8]) -> &mut Self {
        self.0.extend_from_slice(b);
        self
    }
    pub fn string(&mut self, s: &str) -> &mut Self {
        self.u16(s.len() as u16).raw(s.as_bytes())
    }
}

/// A user property's value.
pub enum Prop {
    Bool(bool),
    I32(i32),
    Str(&'static str),
}

/// One slice key: frame, bounds, an optional 9-patch centre, an optional
/// pivot.
pub struct Key {
    pub frame: u32,
    pub bounds: [i32; 4],
    pub center: Option<[i32; 4]>,
    pub pivot: Option<[i32; 2]>,
}

/// A chunk: its type and its data.
type Chunk = (u16, Vec<u8>);

/// A file being written: frames of chunks.
pub struct Ase {
    pub width: u16,
    pub height: u16,
    /// 32, 16, or 8.
    pub depth: u16,
    /// 1: layer opacity is valid; 2: a group's too.
    pub flags: u32,
    pub transparent: u8,
    /// Each frame's duration and chunks.
    frames: Vec<(u16, Vec<Chunk>)>,
}

/// `raw` as a zlib stream of stored (uncompressed) blocks.
pub fn stored_zlib(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    if blocks.is_empty() {
        blocks.push(&[]);
    }
    let last = blocks.len() - 1;
    for (i, b) in blocks.iter().enumerate() {
        out.push(u8::from(i == last));
        let n = b.len() as u16;
        out.extend(n.to_le_bytes());
        out.extend((!n).to_le_bytes());
        out.extend_from_slice(b);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    out.extend(((b << 16) | a).to_be_bytes());
    out
}

impl Ase {
    pub fn new(width: u16, height: u16, depth: u16) -> Ase {
        Ase {
            width,
            height,
            depth,
            flags: 3,
            transparent: 0,
            frames: Vec::new(),
        }
    }

    /// Start a frame of `ms`.
    pub fn frame(&mut self, ms: u16) -> &mut Self {
        self.frames.push((ms, Vec::new()));
        self
    }

    /// A chunk in the current frame.
    pub fn chunk(&mut self, ty: u16, data: Vec<u8>) -> &mut Self {
        self.frames
            .last_mut()
            .expect("a frame first")
            .1
            .push((ty, data));
        self
    }

    /// A layer: kind 0 image, 1 group, 2 tilemap.
    pub fn layer(
        &mut self,
        name: &str,
        flags: u16,
        kind: u16,
        level: u16,
        blend: u16,
        opacity: u8,
    ) -> &mut Self {
        let mut b = Bytes::default();
        b.u16(flags)
            .u16(kind)
            .u16(level)
            .u16(0)
            .u16(0)
            .u16(blend)
            .u8(opacity)
            .zeros(3)
            .string(name);
        if kind == 2 {
            b.u32(0);
        }
        self.chunk(0x2004, b.0)
    }

    /// A visible image layer at the top level.
    pub fn image_layer(&mut self, name: &str) -> &mut Self {
        self.layer(name, VISIBLE, 0, 0, 0, 255)
    }

    fn cel_head(layer: u16, x: i16, y: i16, opacity: u8, ty: u16, z: i16) -> Bytes {
        let mut b = Bytes::default();
        b.u16(layer)
            .i16(x)
            .i16(y)
            .u8(opacity)
            .u16(ty)
            .i16(z)
            .zeros(5);
        b
    }

    /// A raw cel: `px` in the file's depth, row by row.
    pub fn cel(&mut self, layer: u16, x: i16, y: i16, w: u16, h: u16, px: &[u8]) -> &mut Self {
        self.cel_full(layer, x, y, 255, 0, w, h, px)
    }

    /// A raw cel with its opacity and z-index.
    #[allow(clippy::too_many_arguments)]
    pub fn cel_full(
        &mut self,
        layer: u16,
        x: i16,
        y: i16,
        opacity: u8,
        z: i16,
        w: u16,
        h: u16,
        px: &[u8],
    ) -> &mut Self {
        let mut b = Self::cel_head(layer, x, y, opacity, 0, z);
        b.u16(w).u16(h).raw(px);
        self.chunk(0x2005, b.0)
    }

    /// A compressed cel: `zlib` is the pixels' zlib stream.
    pub fn cel_zlib(
        &mut self,
        layer: u16,
        x: i16,
        y: i16,
        w: u16,
        h: u16,
        zlib: &[u8],
    ) -> &mut Self {
        let mut b = Self::cel_head(layer, x, y, 255, 2, 0);
        b.u16(w).u16(h).raw(zlib);
        self.chunk(0x2005, b.0)
    }

    /// A cel linked to the same layer's cel in `frame`.
    pub fn cel_link(&mut self, layer: u16, frame: u16) -> &mut Self {
        let mut b = Self::cel_head(layer, 0, 0, 255, 1, 0);
        b.u16(frame);
        self.chunk(0x2005, b.0)
    }

    /// Tags: (from, to, direction, repeat, name).
    pub fn tags(&mut self, tags: &[(u16, u16, u8, u16, &str)]) -> &mut Self {
        let mut b = Bytes::default();
        b.u16(tags.len() as u16).zeros(8);
        for &(from, to, dir, repeat, name) in tags {
            b.u16(from)
                .u16(to)
                .u8(dir)
                .u16(repeat)
                .zeros(6)
                .raw(&[255, 0, 0])
                .u8(0)
                .string(name);
        }
        self.chunk(0x2018, b.0)
    }

    /// A new palette chunk with these colours from index 0.
    pub fn palette(&mut self, colours: &[[u8; 4]]) -> &mut Self {
        let mut b = Bytes::default();
        b.u32(colours.len() as u32)
            .u32(0)
            .u32(colours.len() as u32 - 1)
            .zeros(8);
        for (i, c) in colours.iter().enumerate() {
            // The first entry has a name, to read past.
            if i == 0 {
                b.u16(1).raw(c).string("clear");
            } else {
                b.u16(0).raw(c);
            }
        }
        self.chunk(0x2019, b.0)
    }

    /// An old palette chunk (0x0004 with 0 to 255, or 0x0011 with 0 to 63):
    /// one packet of these colours from index 0.
    pub fn old_palette(&mut self, ty: u16, colours: &[[u8; 3]]) -> &mut Self {
        let mut b = Bytes::default();
        b.u16(1).u8(0).u8(colours.len() as u8);
        for c in colours {
            b.raw(c);
        }
        self.chunk(ty, b.0)
    }

    /// A slice.
    pub fn slice(&mut self, name: &str, keys: &[Key]) -> &mut Self {
        let nine = keys.iter().any(|k| k.center.is_some());
        let pivot = keys.iter().any(|k| k.pivot.is_some());
        let mut b = Bytes::default();
        b.u32(keys.len() as u32)
            .u32(u32::from(nine) | (u32::from(pivot) << 1))
            .u32(0)
            .string(name);
        for k in keys {
            b.u32(k.frame)
                .i32(k.bounds[0])
                .i32(k.bounds[1])
                .u32(k.bounds[2] as u32)
                .u32(k.bounds[3] as u32);
            if nine {
                let c = k.center.unwrap_or_default();
                b.i32(c[0]).i32(c[1]).u32(c[2] as u32).u32(c[3] as u32);
            }
            if pivot {
                let p = k.pivot.unwrap_or_default();
                b.i32(p[0]).i32(p[1]);
            }
        }
        self.chunk(0x2022, b.0)
    }

    /// User data for the chunk before it: text, a colour, user properties.
    pub fn user_data(&mut self, text: Option<&str>, props: &[(&str, Prop)]) -> &mut Self {
        let mut b = Bytes::default();
        let flags = u32::from(text.is_some()) | 2 | if props.is_empty() { 0 } else { 4 };
        b.u32(flags);
        if let Some(t) = text {
            b.string(t);
        }
        b.raw(&[1, 2, 3, 255]);
        if !props.is_empty() {
            let mut p = Bytes::default();
            // A map of an extension's first, which the importer skips.
            p.u32(2).u32(7).u32(1).string("ext").u16(0x0005).u16(9);
            p.u32(0).u32(props.len() as u32);
            for (name, v) in props {
                p.string(name);
                match v {
                    Prop::Bool(x) => p.u16(0x0001).u8(u8::from(*x)),
                    Prop::I32(x) => p.u16(0x0006).i32(*x),
                    Prop::Str(s) => p.u16(0x000D).string(s),
                };
            }
            b.u32(p.0.len() as u32 + 4).raw(&p.0);
        }
        self.chunk(0x2020, b.0)
    }

    /// The file's bytes.
    pub fn bytes(&self) -> Vec<u8> {
        let mut body = Bytes::default();
        for (ms, chunks) in &self.frames {
            let mut c = Bytes::default();
            for (ty, data) in chunks {
                c.u32(data.len() as u32 + 6).u16(*ty).raw(data);
            }
            body.u32(c.0.len() as u32 + 16)
                .u16(0xF1FA)
                .u16(chunks.len().min(0xFFFF) as u16)
                .u16(*ms)
                .zeros(2)
                .u32(chunks.len() as u32)
                .raw(&c.0);
        }
        let mut h = Bytes::default();
        h.u32(128 + body.0.len() as u32)
            .u16(0xA5E0)
            .u16(self.frames.len() as u16)
            .u16(self.width)
            .u16(self.height)
            .u16(self.depth)
            .u32(self.flags)
            .u16(100)
            .u32(0)
            .u32(0)
            .u8(self.transparent)
            .zeros(3)
            .u16(0)
            .u8(1)
            .u8(1)
            .i16(0)
            .i16(0)
            .u16(16)
            .u16(16)
            .zeros(84);
        assert_eq!(h.0.len(), 128);
        h.raw(&body.0);
        h.0
    }
}

/// `w` x `h` of one RGBA colour.
pub fn fill(w: usize, h: usize, c: [u8; 4]) -> Vec<u8> {
    c.repeat(w * h)
}
