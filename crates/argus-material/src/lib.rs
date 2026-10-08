//! Argus materials, phase 1 item 11 (§13 "Materials and shaders").
//!
//! A material is a WGSL shader plus parameters declared in a sidecar with
//! types. The shader is the fragment half: it defines
//!
//! ```wgsl
//! fn material(in: MaterialInput, p: Params) -> vec4<f32> {
//!     return sample_atlas(in, in.uv) * in.color;
//! }
//! ```
//!
//! and the engine supplies the rest: the instanced sprite vertex shader,
//! `MaterialInput` (uv, place on the sprite, world position, tint, frame,
//! texel size, sim time), `sample_atlas`, the `Params` struct generated
//! from the sidecar, a `sample_<name>` helper for each texture parameter,
//! and the entry point. A parameter is the same for every sprite unless it
//! names a field with `from`: `from = "Dissolve.amount"` reads that field of
//! each sprite's entity (up to four numbers a sprite), and `from =
//! "@torch:Transform.pos"` reads a named entity's field once a frame.
//!
//! [`parse_material`] reads the files; [`check`] composes the shader and
//! runs it through naga's parser and validator, so an error comes back with
//! the line in the author's own file, with or without a GPU. The renderer
//! compiles the same [`compose`] output. [`builtins`] is the shipped
//! library, declared in the same format a game uses.

#![warn(missing_docs)]

use argus_ecs::{ArgusError, Component, Result, StrId, World};
use std::sync::OnceLock;
use toml_edit::{ImDocument, Item, Value as TV};

/// Error codes, block 54xx.
pub mod codes {
    /// A material's sidecar is malformed.
    pub const BAD_MATERIAL: u32 = 5401;
    /// A material's shader does not compile.
    pub const SHADER: u32 = 5402;
}

/// The engine's half of every material.
pub const PRELUDE: &str = include_str!("../materials/prelude.wgsl");

/// The shipped library, by name.
pub const BUILTIN_NAMES: [&str; 6] = ["unlit", "lit", "palette", "dissolve", "outline", "sdf_text"];

const BUILTIN_FILES: [(&str, &str, &str); 6] = [
    (
        "unlit",
        include_str!("../materials/unlit.toml"),
        include_str!("../materials/unlit.wgsl"),
    ),
    (
        "lit",
        include_str!("../materials/lit.toml"),
        include_str!("../materials/lit.wgsl"),
    ),
    (
        "palette",
        include_str!("../materials/palette.toml"),
        include_str!("../materials/palette.wgsl"),
    ),
    (
        "dissolve",
        include_str!("../materials/dissolve.toml"),
        include_str!("../materials/dissolve.wgsl"),
    ),
    (
        "outline",
        include_str!("../materials/outline.toml"),
        include_str!("../materials/outline.wgsl"),
    ),
    (
        "sdf_text",
        include_str!("../materials/sdf_text.toml"),
        include_str!("../materials/sdf_text.wgsl"),
    ),
];

/// How a material's colour meets what is already drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    /// Over it, by alpha.
    Alpha,
    /// Added to it, by alpha: glows and sparks.
    Add,
    /// Multiplied into it, by alpha: shadows and stains.
    Multiply,
}

impl Blend {
    /// Its name in a sidecar.
    pub fn name(self) -> &'static str {
        match self {
            Blend::Alpha => "alpha",
            Blend::Add => "add",
            Blend::Multiply => "multiply",
        }
    }
}

/// A parameter's type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamType {
    /// A number.
    F32,
    /// Two numbers.
    Vec2,
    /// Four numbers.
    Vec4,
    /// A colour, `#RRGGBB` or `#RRGGBBAA`, linear in the shader.
    Color,
    /// A sprite asset's texture, sampled with `sample_<name>(uv)`.
    Texture,
}

impl ParamType {
    /// Its name in a sidecar.
    pub fn name(self) -> &'static str {
        match self {
            ParamType::F32 => "f32",
            ParamType::Vec2 => "vec2",
            ParamType::Vec4 => "vec4",
            ParamType::Color => "color",
            ParamType::Texture => "texture",
        }
    }
    /// How many numbers it holds.
    pub fn floats(self) -> usize {
        match self {
            ParamType::F32 => 1,
            ParamType::Vec2 => 2,
            ParamType::Vec4 | ParamType::Color => 4,
            ParamType::Texture => 0,
        }
    }
    fn wgsl(self) -> &'static str {
        match self {
            ParamType::F32 => "f32",
            ParamType::Vec2 => "vec2<f32>",
            _ => "vec4<f32>",
        }
    }
    fn parse(s: &str) -> Option<ParamType> {
        Some(match s {
            "f32" | "float" | "number" => ParamType::F32,
            "vec2" => ParamType::Vec2,
            "vec4" => ParamType::Vec4,
            "color" | "colour" => ParamType::Color,
            "texture" => ParamType::Texture,
            _ => return None,
        })
    }
}

/// Where a parameter's value comes from when it is not its default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    /// A named entity (`@torch`), read once a frame; none for each
    /// sprite's own entity.
    pub entity: Option<String>,
    /// `Component.field`.
    pub path: String,
    /// `.x` (0) or `.y` (1) of a vec2 field.
    pub axis: Option<u8>,
}

impl Source {
    /// Read per sprite.
    pub fn per_sprite(&self) -> bool {
        self.entity.is_none()
    }
    /// As written in a sidecar.
    pub fn text(&self) -> String {
        let axis = match self.axis {
            Some(0) => ".x",
            Some(_) => ".y",
            None => "",
        };
        match &self.entity {
            Some(e) => format!("@{e}:{}{axis}", self.path),
            None => format!("{}{axis}", self.path),
        }
    }
}

/// One declared parameter.
#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    /// Its name, and its field in `Params`.
    pub name: String,
    /// Its type.
    pub ty: ParamType,
    /// Its default, for numbers (a colour is linear RGBA).
    pub value: [f32; 4],
    /// Its default, for a texture: a sprite asset name.
    pub texture: String,
    /// Allowed values, for a number.
    pub range: Option<[f32; 2]>,
    /// What it does.
    pub doc: String,
    /// A field that supplies it.
    pub from: Option<Source>,
    /// `file:line` where it is declared.
    pub at: String,
}

/// A material, read and checked for shape.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialSpec {
    /// The asset name (`materials/glow`), or the built-in's name.
    pub name: String,
    /// Where it is declared: the sidecar, else the shader.
    pub file: String,
    /// The shader's file (`assets/materials/glow.wgsl`, `builtin:outline`).
    pub shader_file: String,
    /// The shader's text.
    pub source: String,
    /// How it blends.
    pub blend: Blend,
    /// Drawn after the scene's 2D lighting, at its own colour (phase 5
    /// item 3): glows and sparks the dark should not dim.
    pub emissive: bool,
    /// What it is for.
    pub doc: String,
    /// Its parameters, in declared order.
    pub params: Vec<Param>,
}

/// What a field of the `Params` uniform holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// A parameter's value.
    Param(usize),
    /// A texture parameter's frame-0 rectangle in its texture (u, v, w, h).
    TextureRect(usize),
    /// A texture parameter's frame-0 size in pixels.
    TextureSize(usize),
}

/// One field of the `Params` uniform.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    /// Its WGSL name.
    pub name: String,
    /// Its WGSL type.
    pub ty: &'static str,
    /// Byte offset.
    pub offset: usize,
    /// Numbers it holds.
    pub floats: usize,
    /// What goes in it.
    pub slot: Slot,
}

/// Where everything goes: the uniform's fields by WGSL layout rules, each
/// per-sprite parameter's place among the instance's four numbers, and the
/// texture bindings.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// The uniform's size in bytes, a multiple of 16.
    pub size: usize,
    /// Its fields.
    pub fields: Vec<Field>,
    /// Per-sprite parameters: (parameter, first of the instance's numbers).
    pub sprite: Vec<(usize, usize)>,
    /// Texture parameters, bound at 2, 3, ... of group 2 in this order.
    pub textures: Vec<usize>,
}

/// The whole shader for one material, and where the author's file sits in it.
#[derive(Clone, Debug, PartialEq)]
pub struct Composed {
    /// WGSL: the prelude, the generated `Params`, the file, the entry point.
    pub wgsl: String,
    /// The line of `wgsl` that is line 1 of the author's file.
    pub first_line: u32,
    /// Lines in the author's file.
    pub lines: u32,
    /// The uniform and per-sprite layout.
    pub layout: Layout,
}

fn align(n: usize, a: usize) -> usize {
    n.div_ceil(a) * a
}

impl MaterialSpec {
    /// The uniform and per-sprite layout.
    pub fn layout(&self) -> Layout {
        let mut fields = Vec::new();
        let mut at = 0usize;
        let mut push = |name: String, ty: &'static str, floats: usize, slot: Slot| {
            let a = match floats {
                1 => 4,
                2 => 8,
                _ => 16,
            };
            at = align(at, a);
            fields.push(Field {
                name,
                ty,
                offset: at,
                floats,
                slot,
            });
            at += floats * 4;
        };
        for (i, p) in self.params.iter().enumerate() {
            if p.ty != ParamType::Texture {
                push(p.name.clone(), p.ty.wgsl(), p.ty.floats(), Slot::Param(i));
            }
        }
        let mut textures = Vec::new();
        for (i, p) in self.params.iter().enumerate() {
            if p.ty == ParamType::Texture {
                push(
                    format!("{}_rect", p.name),
                    "vec4<f32>",
                    4,
                    Slot::TextureRect(i),
                );
                push(
                    format!("{}_size", p.name),
                    "vec2<f32>",
                    2,
                    Slot::TextureSize(i),
                );
                textures.push(i);
            }
        }
        let mut sprite = Vec::new();
        let mut slot = 0;
        for (i, p) in self.params.iter().enumerate() {
            if p.from.as_ref().is_some_and(Source::per_sprite) {
                sprite.push((i, slot));
                slot += p.ty.floats();
            }
        }
        Layout {
            size: align(at.max(16), 16),
            fields,
            sprite,
            textures,
        }
    }

    /// Every parameter's default as the uniform's bytes (texture rects and
    /// sizes zero until the renderer knows them).
    pub fn default_uniform(&self, layout: &Layout) -> Vec<u8> {
        let mut out = vec![0u8; layout.size];
        for f in &layout.fields {
            if let Slot::Param(i) = f.slot {
                for k in 0..f.floats {
                    let b = self.params[i].value[k].to_le_bytes();
                    out[f.offset + k * 4..f.offset + k * 4 + 4].copy_from_slice(&b);
                }
            }
        }
        out
    }

    /// A parameter by name.
    pub fn param(&self, name: &str) -> Option<&Param> {
        self.params.iter().find(|p| p.name == name)
    }
}

fn lines(s: &str) -> u32 {
    s.matches('\n').count() as u32
}

/// The whole shader: the prelude, the generated parameters, the author's
/// file, and the entry point that calls `material`.
pub fn compose(spec: &MaterialSpec) -> Composed {
    let layout = spec.layout();
    let mut s = String::with_capacity(PRELUDE.len() + spec.source.len() + 2048);
    s.push_str(PRELUDE);
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s.push_str(&format!(
        "\n// ---- the parameters of {} ----\nstruct Params {{\n",
        spec.name
    ));
    if layout.fields.is_empty() {
        s.push_str("    _none: vec4<f32>,\n");
    }
    for f in &layout.fields {
        s.push_str(&format!("    {}: {},\n", f.name, f.ty));
    }
    // Padded to the uniform's size, a multiple of 16 bytes: WebGL2 binds
    // nothing smaller (a material of one or two numbers failed there).
    let end = layout
        .fields
        .iter()
        .map(|f| f.offset + f.floats * 4)
        .max()
        .unwrap_or(16);
    for k in 0..layout.size.saturating_sub(end) / 4 {
        s.push_str(&format!("    _pad{k}: f32,\n"));
    }
    s.push_str("};\n@group(2) @binding(0) var<uniform> params: Params;\n@group(2) @binding(1) var material_samp: sampler;\n");
    for (k, &i) in layout.textures.iter().enumerate() {
        let n = &spec.params[i].name;
        s.push_str(&format!(
            "@group(2) @binding({b}) var {n}_tex: texture_2d<f32>;\n\
             // {n} at a uv from 0 to 1 across its frame 0, wrapping.\n\
             fn sample_{n}(uv: vec2<f32>) -> vec4<f32> {{\n    \
             let r = params.{n}_rect;\n    \
             return textureSampleLevel({n}_tex, material_samp, r.xy + fract(uv) * r.zw, 0.0);\n}}\n",
            b = k + 2
        ));
    }
    s.push_str(&format!("// ---- {} ----\n", spec.shader_file));
    let first_line = lines(&s) + 1;
    s.push_str(&spec.source);
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s.push_str("// ---- the engine's entry point ----\n@fragment\nfn fs_main(v: VOut) -> @location(0) vec4<f32> {\n    var p = params;\n");
    for &(i, slot) in &layout.sprite {
        let prm = &spec.params[i];
        let n = prm.ty.floats();
        let comp = if n == 4 {
            "v.extra".to_string()
        } else {
            format!("v.extra.{}", &"xyzw"[slot..slot + n])
        };
        s.push_str(&format!("    p.{} = {comp};\n", prm.name));
    }
    s.push_str(
        "    let texel = 1.0 / vec2<f32>(textureDimensions(atlas_tex));\n    \
         let m = MaterialInput(v.uv, v.local, v.world, v.color, v.frame, texel, view.roll.z);\n    \
         let c = material(m, p);\n",
    );
    match spec.blend {
        // Multiply wants the colour premultiplied, so a clear pixel leaves
        // what is under it alone.
        Blend::Multiply => s.push_str("    return vec4<f32>(c.rgb * c.a, c.a);\n}\n"),
        _ => s.push_str("    return c;\n}\n"),
    }
    Composed {
        wgsl: s,
        first_line,
        lines: lines(&spec.source) + 1,
        layout,
    }
}

fn chain(e: &dyn std::error::Error) -> String {
    let mut out = e.to_string();
    let mut cur = e.source();
    while let Some(s) = cur {
        let t = s.to_string();
        if !out.contains(&t) {
            out.push_str(": ");
            out.push_str(&t);
        }
        cur = s.source();
    }
    out
}

/// Compose a material and check it with naga's parser and validator. An
/// error names the line of the author's file where it is.
pub fn check(spec: &MaterialSpec) -> Result<Composed> {
    let c = compose(spec);
    let fail = |message: String, labels: Vec<String>, loc: Option<naga::SourceLocation>| {
        let line = loc.map(|l| l.line_number);
        let (at, msg) = match line {
            Some(l) if l >= c.first_line && l < c.first_line + c.lines => {
                let ul = l - c.first_line + 1;
                let col = loc.map(|l| l.line_position).unwrap_or(1);
                (format!("{}:{ul}", spec.shader_file), format!("{}:{ul}:{col}: {message}", spec.shader_file))
            }
            Some(l) if l >= c.first_line + c.lines => (
                spec.shader_file.clone(),
                format!("{}: {message} (in the engine's entry point: the file must define fn material(in: MaterialInput, p: Params) -> vec4<f32>)", spec.shader_file),
            ),
            _ => (
                spec.file.clone(),
                format!("{}: {message} (in the engine's code for this material: a name in the file may clash with the prelude's, or a parameter's)", spec.shader_file),
            ),
        };
        let mut msg = msg;
        for l in labels {
            // naga's own handles ("naga::Expression [21]") mean nothing to an author.
            if !l.is_empty() && !l.starts_with("naga::") && !msg.contains(&l) {
                msg.push_str(&format!("; {l}"));
            }
        }
        ArgusError::new(codes::SHADER, msg)
            .hint("MaterialInput has uv, local, world, color, frame, texel, and time; sample_atlas(in, uv) reads the sprite's atlas; each parameter is p.<name>, and a texture parameter t is sample_t(uv).")
            .at(Some(at))
    };
    let module = naga::front::wgsl::parse_str(&c.wgsl).map_err(|e| {
        let labels = e.labels().map(|(_, l)| l.to_string()).collect();
        fail(e.message().to_string(), labels, e.location(&c.wgsl))
    })?;
    let mut v = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    );
    v.validate(&module).map_err(|e| {
        let labels = e.spans().map(|(_, l)| l.clone()).collect();
        fail(chain(e.as_inner()), labels, e.location(&c.wgsl))
    })?;
    Ok(c)
}

// ----- the sidecar ------------------------------------------------------

const KEYS: &[&str] = &["type", "shader", "blend", "emissive", "doc", "params"];
const PARAM_KEYS: &[&str] = &["type", "default", "range", "doc", "from"];
const RESERVED: &[&str] = &[
    "fn", "let", "var", "const", "struct", "if", "else", "for", "loop", "while", "return", "break",
    "continue", "switch", "case", "default", "discard", "true", "false", "type", "alias", "enable",
    "override", "in", "out", "params", "p", "v", "m", "c", "view", "material", "texel", "_none",
];

const HINT: &str = "A material is <name>.wgsl defining fn material(in: MaterialInput, p: Params) -> vec4<f32>, with an optional <name>.toml: type = \"material\", shader (a built-in: unlit, lit, palette, dissolve, outline; or a .wgsl path), blend (alpha, add, multiply), emissive (true: drawn after the 2D lighting, unlit), doc, and [params.<name>] with type (f32, vec2, vec4, color, texture), default, range, doc, and from (\"Component.field\" per sprite, \"@id:Component.field\" per frame).";

fn bad(at: String, msg: impl Into<String>) -> ArgusError {
    ArgusError::new(codes::BAD_MATERIAL, msg.into())
        .hint(HINT)
        .at(Some(at))
}

fn line_of(text: &str, item: &Item) -> u32 {
    let at = item.span().map(|s| s.start).unwrap_or(0).min(text.len());
    text[..at].matches('\n').count() as u32 + 1
}

fn num(v: &TV) -> Option<f32> {
    v.as_float()
        .map(|f| f as f32)
        .or_else(|| v.as_integer().map(|i| i as f32))
}

/// `#RRGGBB` or `#RRGGBBAA` as linear RGBA.
pub fn parse_color(s: &str) -> Option<[f32; 4]> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 && h.len() != 8 {
        return None;
    }
    let v = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    let (r, g, b) = (v(0)?, v(2)?, v(4)?);
    let a = if h.len() == 8 { v(6)? } else { 255 };
    let lin = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    Some([lin(r), lin(g), lin(b), a as f32 / 255.0])
}

fn ident(s: &str) -> bool {
    let mut ch = s.chars();
    matches!(ch.next(), Some(c) if c.is_ascii_lowercase() || c == '_')
        && ch.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !s.starts_with("__")
}

/// Read `from`: `Component.field`, `Component.vec.x`, or `@id:` before either.
pub fn parse_source(s: &str) -> Option<Source> {
    let (entity, rest) = match s.strip_prefix('@') {
        Some(r) => {
            let (e, p) = r.split_once(':')?;
            if e.is_empty() {
                return None;
            }
            (Some(e.to_string()), p)
        }
        None => (None, s),
    };
    let parts: Vec<&str> = rest.split('.').collect();
    let ok = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    let (path, axis) = match parts.as_slice() {
        [c, f] if ok(c) && ok(f) => (format!("{c}.{f}"), None),
        [c, f, "x"] if ok(c) && ok(f) => (format!("{c}.{f}"), Some(0)),
        [c, f, "y"] if ok(c) && ok(f) => (format!("{c}.{f}"), Some(1)),
        _ => return None,
    };
    Some(Source { entity, path, axis })
}

/// Load a `.wgsl` a sidecar names by path: `(file, text)`.
pub type Loader<'a> = dyn FnMut(&str) -> Result<(String, String)> + 'a;

/// Read a material: its sidecar (`file`, `text`) if it has one and its own
/// `<name>.wgsl` (`file`, `text`) if there is one. `load` reads a `.wgsl`
/// the sidecar names by path. Mistakes are errors with `file:line`; unknown
/// keys come back as warnings `(message, line)`. The shader is not
/// compiled here: [`check`] does that.
pub fn parse_material(
    name: &str,
    sidecar: Option<(&str, &str)>,
    shader: Option<(&str, &str)>,
    load: &mut Loader<'_>,
) -> Result<(MaterialSpec, Vec<(String, u32)>)> {
    let mut warnings = Vec::new();
    let Some((file, text)) = sidecar else {
        let (sf, src) = shader.ok_or_else(|| {
            bad(
                name.to_string(),
                format!("{name} has neither a shader nor a sidecar"),
            )
        })?;
        return Ok((
            MaterialSpec {
                name: name.to_string(),
                file: sf.to_string(),
                shader_file: sf.to_string(),
                source: src.to_string(),
                blend: Blend::Alpha,
                emissive: false,
                doc: String::new(),
                params: Vec::new(),
            },
            warnings,
        ));
    };
    let doc = ImDocument::parse(text.to_string()).map_err(|e| {
        let line = e
            .span()
            .map(|s| text[..s.start.min(text.len())].matches('\n').count() as u32 + 1)
            .unwrap_or(1);
        bad(
            format!("{file}:{line}"),
            format!("not valid TOML: {}", e.message()),
        )
    })?;
    for (k, i) in doc.as_table().iter() {
        if !KEYS.contains(&k) {
            warnings.push((format!("unknown key {k:?}, ignored"), line_of(text, i)));
        }
    }
    let at = |i: &Item| format!("{file}:{}", line_of(text, i));
    let line_at = |k: &str| doc.get(k).map(&at).unwrap_or_else(|| format!("{file}:1"));

    // The shader: this name's .wgsl, a built-in (with its parameters), or a path.
    let mut base: Option<&'static MaterialSpec> = None;
    let (shader_file, source) = match doc.get("shader") {
        None => match shader {
            Some((f, t)) => (f.to_string(), t.to_string()),
            None => {
                return Err(bad(
                    format!("{file}:1"),
                    format!("{name} has no shader: add {name}.wgsl beside the sidecar, or shader = a built-in ({}) or a .wgsl path", BUILTIN_NAMES.join(", ")),
                ))
            }
        },
        Some(item) => {
            let Some(s) = item.as_str() else {
                return Err(bad(at(item), "shader is a string: a built-in material's name or a .wgsl path"));
            };
            if let Some(b) = builtin(s) {
                if let Some((f, _)) = shader {
                    warnings.push((format!("{f} is ignored: shader names the built-in {s:?}"), line_of(text, item)));
                }
                base = Some(b);
                (b.shader_file.clone(), b.source.clone())
            } else if s.ends_with(".wgsl") {
                load(s).map_err(|e| bad(at(item), format!("shader = {s:?}: {}", e.message)))?
            } else {
                return Err(bad(
                    at(item),
                    format!("shader = {s:?} is neither a built-in ({}) nor a .wgsl path", BUILTIN_NAMES.join(", ")),
                ));
            }
        }
    };
    let blend = match doc.get("blend") {
        None => base.map_or(Blend::Alpha, |b| b.blend),
        Some(i) => match i.as_str() {
            Some("alpha") => Blend::Alpha,
            Some("add") => Blend::Add,
            Some("multiply") => Blend::Multiply,
            _ => return Err(bad(at(i), "blend is alpha, add, or multiply")),
        },
    };
    let emissive = match doc.get("emissive") {
        None => base.is_some_and(|b| b.emissive),
        Some(i) => i
            .as_bool()
            .ok_or_else(|| bad(at(i), "emissive is true or false"))?,
    };
    let mdoc = doc
        .get("doc")
        .and_then(|d| d.as_str())
        .map(str::to_string)
        .or_else(|| base.map(|b| b.doc.clone()))
        .unwrap_or_default();

    let mut params: Vec<Param> = base.map(|b| b.params.clone()).unwrap_or_default();
    if let Some(item) = doc.get("params") {
        let table = item
            .as_table_like()
            .ok_or_else(|| bad(line_at("params"), "params is a table: [params.<name>]"))?;
        for (pname, pitem) in table.iter() {
            let pat = at(pitem);
            let pt = pitem.as_table_like().ok_or_else(|| {
                bad(
                    pat.clone(),
                    format!("params.{pname} is a table with type, default, range, doc, from"),
                )
            })?;
            for (k, i) in pt.iter() {
                if !PARAM_KEYS.contains(&k) {
                    warnings.push((
                        format!("params.{pname}: unknown key {k:?}, ignored"),
                        line_of(text, i),
                    ));
                }
            }
            let key_at = |k: &str| pt.get(k).map(&at).unwrap_or_else(|| pat.clone());
            if !ident(pname) || RESERVED.contains(&pname) {
                return Err(bad(pat, format!("params.{pname}: a parameter's name is a lower-case WGSL name that is not reserved (letters, digits, _)")));
            }
            let existing = params.iter().position(|p| p.name == pname);
            if base.is_some() && existing.is_none() {
                let names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
                return Err(bad(
                    pat,
                    format!(
                        "params.{pname}: {} has no such parameter; its parameters are {}",
                        shader_file,
                        if names.is_empty() {
                            "none".to_string()
                        } else {
                            names.join(", ")
                        }
                    ),
                ));
            }
            let ty = match pt.get("type") {
                Some(i) => {
                    let t = i.as_str().and_then(ParamType::parse).ok_or_else(|| {
                        bad(
                            at(i),
                            format!("params.{pname}: type is f32, vec2, vec4, color, or texture"),
                        )
                    })?;
                    if let Some(e) = existing {
                        if params[e].ty != t {
                            return Err(bad(
                                at(i),
                                format!(
                                    "params.{pname} is a {} in {}; a variant keeps the type",
                                    params[e].ty.name(),
                                    shader_file
                                ),
                            ));
                        }
                    }
                    t
                }
                None => match existing {
                    Some(e) => params[e].ty,
                    None => {
                        return Err(bad(
                            pat,
                            format!(
                                "params.{pname} needs a type: f32, vec2, vec4, color, or texture"
                            ),
                        ))
                    }
                },
            };
            let mut p = match existing {
                Some(e) => params[e].clone(),
                None => Param {
                    name: pname.to_string(),
                    ty,
                    value: [0.0; 4],
                    texture: String::new(),
                    range: None,
                    doc: String::new(),
                    from: None,
                    at: pat.clone(),
                },
            };
            p.at = pat.clone();
            if let Some(i) = pt.get("doc") {
                p.doc = i.as_str().unwrap_or_default().to_string();
            }
            if let Some(i) = pt.get("range") {
                let r = i
                    .as_array()
                    .filter(|a| a.len() == 2)
                    .and_then(|a| Some([num(a.get(0)?)?, num(a.get(1)?)?]));
                match r {
                    Some(r) if ty == ParamType::F32 && r[0] <= r[1] => p.range = Some(r),
                    _ => {
                        return Err(bad(
                            at(i),
                            format!("params.{pname}: range is [min, max] and only for an f32"),
                        ))
                    }
                }
            }
            if let Some(i) = pt.get("default") {
                let v = i.as_value();
                let wrong = || {
                    bad(
                        at(i),
                        format!(
                            "params.{pname}: default must be {}",
                            match ty {
                                ParamType::F32 => "a number",
                                ParamType::Vec2 => "[x, y]",
                                ParamType::Vec4 => "[x, y, z, w]",
                                ParamType::Color => "\"#RRGGBB\" or \"#RRGGBBAA\"",
                                ParamType::Texture => "a sprite asset's name",
                            }
                        ),
                    )
                };
                match ty {
                    ParamType::F32 => p.value = [v.and_then(num).ok_or_else(wrong)?, 0.0, 0.0, 0.0],
                    ParamType::Vec2 | ParamType::Vec4 => {
                        let n = ty.floats();
                        let xs: Vec<f32> = v
                            .and_then(|v| v.as_array())
                            .map(|a| a.iter().filter_map(num).collect())
                            .unwrap_or_default();
                        if xs.len() != n || v.and_then(|v| v.as_array()).map(|a| a.len()) != Some(n)
                        {
                            return Err(wrong());
                        }
                        p.value = [0.0; 4];
                        p.value[..n].copy_from_slice(&xs);
                    }
                    ParamType::Color => {
                        p.value = v
                            .and_then(|v| v.as_str())
                            .and_then(parse_color)
                            .ok_or_else(wrong)?
                    }
                    ParamType::Texture => {
                        p.texture = v.and_then(|v| v.as_str()).ok_or_else(wrong)?.to_string()
                    }
                }
            }
            if let (Some(r), ParamType::F32) = (p.range, ty) {
                if p.value[0] < r[0] || p.value[0] > r[1] {
                    return Err(bad(
                        key_at("default"),
                        format!(
                            "params.{pname}: default {} is outside its range [{}, {}]",
                            p.value[0], r[0], r[1]
                        ),
                    ));
                }
            }
            if let Some(i) = pt.get("from") {
                let s = i.as_str().unwrap_or_default();
                let src = parse_source(s).ok_or_else(|| {
                    bad(at(i), format!("params.{pname}: from = {s:?} is not \"Component.field\", \"Component.vec.x\", or \"@id:Component.field\""))
                })?;
                if ty == ParamType::Texture {
                    return Err(bad(
                        at(i),
                        format!(
                            "params.{pname}: a texture comes from its default, not from a field"
                        ),
                    ));
                }
                if src.axis.is_some() && ty != ParamType::F32 {
                    return Err(bad(
                        at(i),
                        format!(
                            "params.{pname}: .x or .y gives one number, and {pname} is a {}",
                            ty.name()
                        ),
                    ));
                }
                p.from = Some(src);
            }
            match existing {
                Some(e) => params[e] = p,
                None => params.push(p),
            }
        }
    }
    // Four numbers a sprite, in declared order.
    let mut used = 0;
    for p in &params {
        if p.from.as_ref().is_some_and(Source::per_sprite) {
            used += p.ty.floats();
            if used > 4 {
                return Err(bad(
                    p.at.clone(),
                    format!("params.{}: a sprite carries four numbers for its material, and the per-sprite parameters up to {} take {used}", p.name, p.name),
                ));
            }
        }
    }
    let spec = MaterialSpec {
        name: name.to_string(),
        file: file.to_string(),
        shader_file,
        source,
        blend,
        emissive,
        doc: mdoc,
        params,
    };
    Ok((spec, warnings))
}

/// The shipped library, read and checked once. A game shadows one by
/// making an asset of the same name.
pub fn builtins() -> &'static [MaterialSpec] {
    static B: OnceLock<Vec<MaterialSpec>> = OnceLock::new();
    B.get_or_init(|| {
        BUILTIN_FILES
            .iter()
            .map(|(name, toml, wgsl)| {
                let shader_file = format!("builtin:{name}");
                let side = format!("builtin:{name}.toml");
                let (spec, _) = parse_material(
                    name,
                    Some((&side, toml)),
                    Some((&shader_file, wgsl)),
                    &mut |p: &str| {
                        Err(ArgusError::new(
                            codes::BAD_MATERIAL,
                            format!("a built-in names {p}"),
                        ))
                    },
                )
                .unwrap_or_else(|e| panic!("built-in material {name}: {}", e.message));
                spec
            })
            .collect()
    })
}

/// A built-in by name.
pub fn builtin(name: &str) -> Option<&'static MaterialSpec> {
    // The library's own sidecars name no shader, so reading them never
    // comes back here.
    if !BUILTIN_NAMES.contains(&name) {
        return None;
    }
    builtins().iter().find(|b| b.name == name)
}

// ----- the library's per-sprite components --------------------------------

/// Per-sprite value for the `dissolve` material.
#[derive(Component, Clone, Copy, Debug, PartialEq, Default)]
#[repr(C)]
#[argus(doc = "How far a sprite drawn with the dissolve material has dissolved")]
pub struct Dissolve {
    /// 0 whole, 1 gone.
    #[argus(doc = "0 is whole, 1 is gone; tween it", range(0, 1), tune)]
    pub amount: f32,
}

/// Per-sprite value for the `outline` material.
#[derive(Component, Clone, Copy, Debug, PartialEq, Default)]
#[repr(C)]
#[argus(doc = "The line colour of a sprite drawn with the outline material")]
pub struct Outline {
    /// `#RRGGBB` or `#RRGGBBAA`; empty for the material's default.
    #[argus(doc = "The line's colour, #RRGGBB or #RRGGBBAA; empty for the material's default")]
    pub color: StrId,
}

/// Per-sprite value for the `palette` material.
#[derive(Component, Clone, Copy, Debug, PartialEq, Default)]
#[repr(C)]
#[argus(doc = "Which palette a sprite drawn with the palette material uses")]
pub struct Palette {
    /// Row of the palette texture, from 0.
    #[argus(doc = "The palette texture's row, from 0", range(0, 4096), tune)]
    pub row: u32,
}

/// Register the library's per-sprite components.
pub fn register(world: &mut World) -> Result<()> {
    world.register::<Dissolve>()?;
    world.register::<Outline>()?;
    world.register::<Palette>()?;
    Ok(())
}
