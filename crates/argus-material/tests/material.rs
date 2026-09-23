//! §13 Materials and shaders: a WGSL shader plus typed parameters in a
//! sidecar; the shipped library; compile errors with the author's line
//! numbers.

use argus_material::{
    builtin, builtins, check, compose, parse_material, parse_source, Blend, ParamType, Slot,
    Source, BUILTIN_NAMES,
};

fn no_load(p: &str) -> argus_ecs::Result<(String, String)> {
    Err(argus_ecs::ArgusError::new(1, format!("no file {p}")))
}

fn material(
    side: Option<&str>,
    wgsl: Option<&str>,
) -> argus_ecs::Result<argus_material::MaterialSpec> {
    parse_material(
        "materials/m",
        side.map(|t| ("assets/materials/m.toml", t)),
        wgsl.map(|t| ("assets/materials/m.wgsl", t)),
        &mut no_load,
    )
    .map(|(s, _)| s)
}

const PLAIN: &str = "fn material(in: MaterialInput, p: Params) -> vec4<f32> {\n    return sample_atlas(in, in.uv) * in.color;\n}\n";

/// The engine ships lit, unlit, palette, dissolve, and outline, each read
/// from the same sidecar format a game writes and each compiling.
#[test]
fn the_library_is_five_materials_that_compile() {
    let names: Vec<&str> = builtins().iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, BUILTIN_NAMES);
    for b in builtins() {
        check(b).unwrap_or_else(|e| panic!("{}: {}", b.name, e.message));
        assert!(!b.doc.is_empty(), "{} says what it is for", b.name);
        assert_eq!(b.shader_file, format!("builtin:{}", b.name));
    }
    let d = builtin("dissolve").unwrap();
    let amount = d.param("amount").unwrap();
    assert_eq!(
        amount.from,
        Some(Source {
            entity: None,
            path: "Dissolve.amount".into(),
            axis: None
        })
    );
    assert_eq!(amount.range, Some([0.0, 1.0]));
    assert_eq!(
        builtin("outline").unwrap().param("color").unwrap().ty,
        ParamType::Color
    );
    assert_eq!(
        builtin("palette").unwrap().param("palette").unwrap().ty,
        ParamType::Texture
    );
    assert!(builtin("glow").is_none());
}

/// Parameters are typed: numbers, vectors, colours (linear in the shader),
/// and textures, laid out in the uniform by WGSL's rules.
#[test]
fn parameters_are_typed_and_laid_out_by_wgsl_rules() {
    let spec = material(
        Some("type = \"material\"\nblend = \"add\"\ndoc = \"A glow\"\n\n[params.power]\ntype = \"f32\"\ndefault = 2\nrange = [0, 8]\n\n[params.dir]\ntype = \"vec2\"\ndefault = [1, 0]\n\n[params.tint]\ntype = \"color\"\ndefault = \"#FF000080\"\n\n[params.noise]\ntype = \"texture\"\ndefault = \"fx/noise\"\n\n[params.spin]\ntype = \"f32\"\ndefault = 0\nfrom = \"Glow.spin\"\n"),
        Some(PLAIN),
    )
    .unwrap();
    assert_eq!(spec.blend, Blend::Add);
    assert_eq!(spec.doc, "A glow");
    let tint = spec.param("tint").unwrap();
    assert_eq!(tint.value, [1.0, 0.0, 0.0, 128.0 / 255.0]);
    assert_eq!(spec.param("noise").unwrap().texture, "fx/noise");
    let l = spec.layout();
    let at: Vec<(&str, usize)> = l
        .fields
        .iter()
        .map(|f| (f.name.as_str(), f.offset))
        .collect();
    // f32 at 0, vec2 aligned to 8, vec4 to 16, the second f32 after it,
    // then the texture's rect (16) and size (8).
    assert_eq!(
        at,
        [
            ("power", 0),
            ("dir", 8),
            ("tint", 16),
            ("spin", 32),
            ("noise_rect", 48),
            ("noise_size", 64)
        ]
    );
    assert_eq!(l.size, 80);
    assert_eq!(l.fields[4].slot, Slot::TextureRect(3));
    assert_eq!(
        l.sprite,
        [(4, 0)],
        "spin is per sprite, the first of the four numbers"
    );
    assert_eq!(l.textures, [3]);
    let bytes = spec.default_uniform(&l);
    assert_eq!(f32::from_le_bytes(bytes[0..4].try_into().unwrap()), 2.0);
    assert_eq!(f32::from_le_bytes(bytes[8..12].try_into().unwrap()), 1.0);
    let c = check(&spec).unwrap();
    assert!(c.wgsl.contains("fn sample_noise(uv: vec2<f32>)"));
    assert!(c.wgsl.contains("p.spin = v.extra.x;"));
}

/// A shader alone is a material with no parameters; a sidecar can take a
/// built-in's shader and change its defaults, which is a variant.
#[test]
fn a_bare_shader_and_a_variant_of_a_built_in() {
    let bare = material(None, Some(PLAIN)).unwrap();
    assert!(bare.params.is_empty());
    assert_eq!(bare.file, "assets/materials/m.wgsl");
    check(&bare).unwrap();
    let thick = material(
        Some("shader = \"outline\"\n\n[params.width]\ndefault = 3\n\n[params.color]\ndefault = \"#FF0000\"\n"),
        None,
    )
    .unwrap();
    assert_eq!(thick.shader_file, "builtin:outline");
    assert_eq!(thick.param("width").unwrap().value[0], 3.0);
    assert_eq!(thick.param("color").unwrap().value, [1.0, 0.0, 0.0, 1.0]);
    assert!(
        thick.param("color").unwrap().from.is_some(),
        "still per sprite"
    );
    check(&thick).unwrap();
    let glow = material(Some("shader = \"unlit\"\nblend = \"add\"\n"), None).unwrap();
    assert_eq!(glow.blend, Blend::Add);
    // A variant can only change what the built-in has.
    let e = material(
        Some("shader = \"outline\"\n\n[params.glow]\ndefault = 1\n"),
        None,
    )
    .unwrap_err();
    assert!(
        e.message.contains("its parameters are color, width"),
        "{}",
        e.message
    );
    assert_eq!(e.r#ref.as_deref(), Some("assets/materials/m.toml:3"));
    // A shader by path is read through the loader and watched.
    let (s, _) = parse_material(
        "materials/red",
        Some((
            "assets/materials/red.toml",
            "shader = \"materials/common.wgsl\"\n",
        )),
        None,
        &mut |p: &str| Ok((format!("assets/{p}"), PLAIN.to_string())),
    )
    .unwrap();
    assert_eq!(s.shader_file, "assets/materials/common.wgsl");
}

/// Mistakes in the sidecar fail with the line.
#[test]
fn sidecar_mistakes_have_lines() {
    for (text, line, words) in [
        ("[params.x]\ntype = \"f16\"\n", 2, "type is f32, vec2"),
        ("[params.x]\ndefault = 1\n", 1, "needs a type"),
        ("[params.x]\ntype = \"f32\"\ndefault = \"one\"\n", 3, "default must be a number"),
        ("[params.x]\ntype = \"vec2\"\ndefault = [1]\n", 3, "default must be [x, y]"),
        ("[params.x]\ntype = \"color\"\ndefault = \"red\"\n", 3, "#RRGGBB"),
        ("[params.x]\ntype = \"f32\"\ndefault = 5\nrange = [0, 1]\n", 3, "outside its range"),
        ("[params.x]\ntype = \"vec2\"\nrange = [0, 1]\n", 3, "only for an f32"),
        ("[params.x]\ntype = \"f32\"\nfrom = \"health\"\n", 3, "is not \"Component.field\""),
        ("[params.x]\ntype = \"texture\"\nfrom = \"A.b\"\n", 3, "a texture comes from its default"),
        ("[params.x]\ntype = \"vec2\"\nfrom = \"A.b.x\"\n", 3, ".x or .y gives one number"),
        ("[params.fn]\ntype = \"f32\"\n", 1, "not reserved"),
        ("[params.Big]\ntype = \"f32\"\n", 1, "lower-case"),
        ("blend = \"screen\"\n", 1, "alpha, add, or multiply"),
        ("shader = \"glow\"\n", 1, "neither a built-in"),
        ("[params.a]\ntype = \"color\"\nfrom = \"A.a\"\n\n[params.b]\ntype = \"f32\"\nfrom = \"A.b\"\n", 5, "four numbers"),
    ] {
        let e = material(Some(text), Some(PLAIN)).expect_err(text);
        assert!(e.message.contains(words), "{text}: {}", e.message);
        assert_eq!(e.r#ref.as_deref(), Some(format!("assets/materials/m.toml:{line}").as_str()), "{text}: {}", e.message);
    }
    let e = material(Some("doc = \"no shader\"\n"), None).unwrap_err();
    assert!(e.message.contains("has no shader"), "{}", e.message);
    let (_, warnings) = parse_material(
        "m",
        Some((
            "m.toml",
            "colour = 1\n[params.x]\ntype = \"f32\"\ndefualt = 1\n",
        )),
        Some(("m.wgsl", PLAIN)),
        &mut no_load,
    )
    .unwrap();
    assert_eq!(
        warnings,
        [
            ("unknown key \"colour\", ignored".to_string(), 1),
            ("params.x: unknown key \"defualt\", ignored".to_string(), 4)
        ]
    );
}

/// A shader that does not compile fails with the line and column in the
/// author's file, from naga, with no GPU.
#[test]
fn shader_errors_name_the_line_in_the_authors_file() {
    let cases = [
        // Parse error on line 2.
        ("fn material(in: MaterialInput, p: Params) -> vec4<f32> {\n    let a = 1.0 +;\n    return vec4<f32>(a);\n}\n", Some(2)),
        // Type error (validation) on line 3.
        ("fn material(in: MaterialInput, p: Params) -> vec4<f32> {\n    let a = 1.0;\n    let b: i32 = a;\n    return vec4<f32>(a);\n}\n", Some(3)),
        // An unknown parameter on line 2.
        ("fn material(in: MaterialInput, p: Params) -> vec4<f32> {\n    return vec4<f32>(p.glow);\n}\n", Some(2)),
        // No material function: the entry point says what is missing.
        ("fn shade(in: MaterialInput) -> vec4<f32> {\n    return in.color;\n}\n", None),
    ];
    for (src, line) in cases {
        let spec = material(
            Some("[params.power]\ntype = \"f32\"\ndefault = 1\n"),
            Some(src),
        )
        .unwrap();
        let e = check(&spec).expect_err(src);
        assert_eq!(e.code, argus_material::codes::SHADER);
        match line {
            Some(l) => {
                assert_eq!(
                    e.r#ref.as_deref(),
                    Some(format!("assets/materials/m.wgsl:{l}").as_str()),
                    "{}",
                    e.message
                );
                assert!(
                    e.message
                        .starts_with(&format!("assets/materials/m.wgsl:{l}:")),
                    "{}",
                    e.message
                );
            }
            None => assert!(
                e.message.contains("must define fn material"),
                "{}",
                e.message
            ),
        }
    }
    let c = compose(&material(None, Some(PLAIN)).unwrap());
    let first: Vec<&str> = c
        .wgsl
        .lines()
        .skip(c.first_line as usize - 1)
        .take(1)
        .collect();
    assert_eq!(
        first,
        ["fn material(in: MaterialInput, p: Params) -> vec4<f32> {"]
    );
}

#[test]
fn sources_read_per_sprite_or_from_a_named_entity() {
    assert_eq!(
        parse_source("Outline.color"),
        Some(Source {
            entity: None,
            path: "Outline.color".into(),
            axis: None
        })
    );
    assert_eq!(
        parse_source("Transform.pos.y"),
        Some(Source {
            entity: None,
            path: "Transform.pos".into(),
            axis: Some(1)
        })
    );
    let s = parse_source("@torch:Transform.pos").unwrap();
    assert!(!s.per_sprite());
    assert_eq!(s.text(), "@torch:Transform.pos");
    for bad in ["", "A", "A.b.c", "@:A.b", "@x", "A.b.z", "A..b"] {
        assert!(parse_source(bad).is_none(), "{bad}");
    }
}
