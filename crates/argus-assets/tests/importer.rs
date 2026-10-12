//! §14: sidecars an agent can write (frames and pivots, clip lists and
//! rates, gain and variation sets), a cache in `.argus/`, every import
//! reported with warnings ("frame 7 is empty", "texture not
//! power-of-two, padding"), a watched tree, and a described shape the
//! human replaces later without touching a reference.

use argus_assets::{decode_png, encode_png, Asset, Importer};

fn project(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("argus-assets-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    dir
}

fn write(root: &std::path::Path, rel: &str, text: &str) {
    let p = root.join("assets").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// A w x h sheet, opaque except for frame `empty` of a cols x rows grid.
fn sheet(
    root: &std::path::Path,
    rel: &str,
    w: u32,
    h: u32,
    cols: u32,
    rows: u32,
    empty: Option<u32>,
) {
    let mut px = vec![0u8; (w * h * 4) as usize];
    let (fw, fh) = (w / cols, h / rows);
    for y in 0..h {
        for x in 0..w {
            let f = (y / fh) * cols + x / fw;
            if Some(f) == empty {
                continue;
            }
            let i = ((y * w + x) * 4) as usize;
            px[i..i + 4].copy_from_slice(&[40 + f as u8 * 20, 200, 90, 255]);
        }
    }
    encode_png(&root.join("assets").join(rel), w, h, &px).unwrap();
}

fn messages(e: &argus_assets::Entry) -> Vec<String> {
    e.warnings.iter().map(|w| w.message.clone()).collect()
}

fn sprite(e: &argus_assets::Entry) -> &argus_assets::SpriteAsset {
    match &e.asset {
        Asset::Sprite(s) => s,
        other => panic!("not a sprite: {other:?}"),
    }
}

#[test]
fn a_placeholder_is_replaced_by_the_art_under_the_same_name() {
    let dir = project("replace");
    write(
        &dir,
        "sprites/hero.toml",
        "kind = \"rect\"\nsize = [8, 8]\ncolor = \"#E8703F\"\npivot = [0.5, 0.0]\n\n[clips.idle]\nframes = [0]\n",
    );
    let mut imp = Importer::new(&dir);
    let r = imp.scan();
    assert_eq!(r.imported, vec!["sprites/hero"], "{:?}", r.failed);
    let e = imp.get("sprites/hero").unwrap();
    assert!(e.warnings.is_empty(), "{:?}", messages(e));
    let s = sprite(e);
    assert_eq!(s.source, None);
    assert_eq!(s.shape.as_ref().unwrap().color, "#E8703F");
    assert_eq!(s.frames[0].pivot, [0.5, 0.0]);
    // The importer drew the shape into the cache: a real texture.
    let (w, h, px) = decode_png(&s.texture).unwrap();
    assert_eq!((w, h), (8, 8));
    assert_eq!(&px[..4], &[0xE8, 0x70, 0x3F, 255]);
    assert!(s.texture.starts_with(dir.join(".argus").join("assets")));

    // The human drops in the art: 30x20, a 3x2 grid, frame 5 left blank.
    sheet(&dir, "sprites/hero.png", 30, 20, 3, 2, Some(5));
    write(
        &dir,
        "sprites/hero.toml",
        "kind = \"rect\"\nsize = [8, 8]\ncolor = \"#E8703F\"\ngrid = [3, 2]\npivot = [0.5, 0.0]\n\n[clips.idle]\nframes = [0]\n\n[clips.run]\nfrom = 1\nto = 4\nfps = 12\n",
    );
    let r = imp.poll().expect("the tree changed");
    assert_eq!(r.imported, vec!["sprites/hero"]);
    let e = imp.get("sprites/hero").unwrap();
    let s = sprite(e);
    assert_eq!(s.source.as_deref(), Some("sprites/hero.png"));
    assert_eq!(s.frames.len(), 6);
    assert_eq!(s.frames[4].rect, [10, 10, 10, 10]);
    assert_eq!(s.clips["run"].frames, vec![1, 2, 3, 4]);
    assert_eq!(s.clips["run"].fps, Some(12.0));
    let m = messages(e);
    assert!(
        m.contains(
            &"kind, size, and color are ignored: assets/sprites/hero.png is the art now"
                .to_string()
        ),
        "{m:?}"
    );
    assert!(
        m.contains(&"texture 30x20 is not power-of-two, padding to 32x32".to_string()),
        "{m:?}"
    );
    assert!(m.contains(&"frame 5 is empty".to_string()), "{m:?}");
    assert_eq!(s.texture_size, [32, 32]);
    let (w, h, _) = decode_png(&s.texture).unwrap();
    assert_eq!((w, h), (32, 32), "the cache holds the padded texture");

    let tex = s.texture.clone();
    // Nothing changed: the poll is free and says so.
    assert!(imp.poll().is_none());
    // The manifest is in the cache for anyone to read.
    let manifest = std::fs::read_to_string(dir.join(".argus/assets/manifest.json")).unwrap();
    assert!(manifest.contains("\"sprites/hero\""));

    // Removing the files removes the asset and its cached texture.
    std::fs::remove_file(dir.join("assets/sprites/hero.png")).unwrap();
    std::fs::remove_file(dir.join("assets/sprites/hero.toml")).unwrap();
    let r = imp.poll().unwrap();
    assert_eq!(r.removed, vec!["sprites/hero"]);
    assert!(!tex.exists());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_sidecar_carries_what_aseprite_and_tiled_carry() {
    let dir = project("aseprite");
    sheet(&dir, "fx/slash.png", 64, 16, 4, 1, None);
    write(
        &dir,
        "fx/slash.toml",
        r#"grid = [4, 1]
duration_ms = 80

[[frames]]
index = 2
duration_ms = 200
pivot = [0.25, 0.5]
properties = { hitbox = "on", damage = 3 }

[clips.swing]
frames = [0, 1, 2, 3]
direction = "pingpong"
loop = false

[clips.broken]
frames = [2, 9]
"#,
    );
    let mut imp = Importer::new(&dir);
    imp.scan();
    let e = imp.get("fx/slash").unwrap();
    let s = sprite(e);
    assert_eq!(s.frames[0].duration_ms, 80);
    assert_eq!(
        s.frames[2].duration_ms, 200,
        "per-frame durations, the Aseprite shape"
    );
    assert_eq!(s.frames[2].pivot, [0.25, 0.5]);
    assert_eq!(
        s.frames[2].properties["hitbox"], "on",
        "per-frame properties, the Tiled shape"
    );
    assert_eq!(s.frames[2].properties["damage"], "3");
    assert_eq!(s.clips["swing"].direction.name(), "pingpong");
    assert!(!s.clips["swing"].looping);
    assert_eq!(s.clips["broken"].frames, vec![2]);
    let m = messages(e);
    assert_eq!(
        m,
        vec!["clip broken names frame 9; the sheet has 4 frames"],
        "a 64x16 sheet needs no padding"
    );
    assert_eq!(
        e.warnings[0].at, "assets/fx/slash.toml:15",
        "the clip's header"
    );
    let j = e.to_json();
    assert_eq!(j["clips"]["swing"]["direction"], "pingpong");
    assert_eq!(j["frames"][2]["properties"]["hitbox"], "on");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn bad_sidecars_fail_with_the_line_and_the_rest_still_import() {
    let dir = project("errors");
    sheet(&dir, "a.png", 16, 16, 1, 1, None);
    write(&dir, "a.toml", "\n\n[[frames]]\nrect = [8, 8, 16, 16]\n");
    write(&dir, "b.toml", "\nkind = \"hexagon\"\n");
    write(&dir, "c.toml", "gain_db = 3\n");
    sheet(&dir, "d.png", 16, 16, 1, 1, None);
    write(&dir, "d.wav", "not really");
    sheet(&dir, "fine.png", 16, 16, 1, 1, None);
    write(&dir, "legacy.atlas.toml", "grid = [2, 1]\n");
    sheet(&dir, "legacy.png", 32, 16, 2, 1, None);
    let mut imp = Importer::new(&dir);
    let r = imp.scan();
    let why: std::collections::BTreeMap<String, (String, Option<String>)> = r
        .failed
        .iter()
        .map(|f| {
            (
                f.name.clone(),
                (f.error.message.clone(), f.error.r#ref.clone()),
            )
        })
        .collect();
    assert!(why["a"].0.contains("outside the 16x16 image"), "{why:?}");
    assert_eq!(why["a"].1.as_deref(), Some("assets/a.toml:3"));
    assert!(why["b"].0.contains("shapes are rect and circle"));
    assert_eq!(why["b"].1.as_deref(), Some("assets/b.toml:2"));
    assert!(
        why["c"]
            .0
            .contains("no sprite, sound, animator, emitter, or material"),
        "{why:?}"
    );
    assert!(why["d"].0.contains("both a .png and a .wav"));
    assert!(
        imp.get("fine").is_ok(),
        "one bad asset never stops the rest"
    );
    assert_eq!(
        sprite(imp.get("legacy").unwrap()).frames.len(),
        2,
        "name.atlas.toml still works"
    );
    let e = imp.get("a").unwrap_err();
    assert!(
        e.hint.contains("outside"),
        "asking for a failed asset says why: {}",
        e.hint
    );
    // Fixing the file fixes the import on the next poll.
    write(&dir, "a.toml", "[[frames]]\nrect = [0, 0, 8, 8]\n");
    let r = imp.poll().unwrap();
    assert_eq!(r.imported, vec!["a"]);
    assert!(imp.failures().get("a").is_none());
    std::fs::remove_dir_all(&dir).ok();
}

fn wav(root: &std::path::Path, rel: &str, rate: u32, channels: u16, amp: f32) {
    let p = root.join("assets").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    let spec = hound::WavSpec {
        channels,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(p, spec).unwrap();
    for i in 0..(rate / 10) {
        let s = ((i as f32 * 0.2).sin() * amp * i16::MAX as f32) as i16;
        for _ in 0..channels {
            w.write_sample(s).unwrap();
        }
    }
    w.finalize().unwrap();
}

#[test]
fn a_sound_sidecar_names_its_variations_and_the_files_are_checked() {
    let dir = project("sound");
    wav(&dir, "sfx/step1.wav", 48_000, 1, 0.5);
    wav(&dir, "sfx/step2.wav", 22_050, 2, 1.0);
    write(
        &dir,
        "sfx/step.toml",
        "type = \"sound\"\nbus = \"sfx\"\ngain = 0.8\npitch_jitter = 0.05\nvariations = [\"step1.wav\", \"step2.wav\", { wave = \"noise\", ms = 40 }]\n",
    );
    let mut imp = Importer::new(&dir);
    let r = imp.scan();
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    let e = imp.get("sfx/step").unwrap();
    let Asset::Sound(s) = &e.asset else { panic!() };
    assert!(s.from_sidecar);
    assert_eq!(s.def.variations.len(), 3);
    assert_eq!(s.files.len(), 2);
    assert_eq!(
        s.files[1].path, "sfx/step2.wav",
        "relative to the sidecar's folder, stored relative to assets/"
    );
    let m = messages(e);
    assert!(
        m.contains(&"2 channels, mixed to mono for spatial playback".to_string()),
        "{m:?}"
    );
    assert!(
        m.contains(&"22050 Hz, resampled to 48000 Hz at load".to_string()),
        "{m:?}"
    );
    assert!(m.iter().any(|x| x.contains("it clips")), "{m:?}");
    assert_eq!(
        imp.sidecar_sounds().len(),
        1,
        "bare WAVs are the mixer's own business"
    );
    // Editing a variation re-imports the sound that plays it.
    wav(&dir, "sfx/step1.wav", 48_000, 1, 0.0);
    let r = imp.poll().unwrap();
    assert!(
        r.imported.contains(&"sfx/step".to_string()),
        "{:?}",
        r.imported
    );
    assert!(messages(imp.get("sfx/step1").unwrap()).contains(&"the file is silent".to_string()));
    // A missing variation fails with the sidecar named.
    write(&dir, "sfx/step.toml", "variations = [\"step9.wav\"]\n");
    let r = imp.poll().unwrap();
    assert_eq!(r.failed[0].name, "sfx/step");
    assert!(r.failed[0].error.message.contains("step9.wav"));
    std::fs::remove_dir_all(&dir).ok();
}

/// Item 8: an animator is an asset (a sidecar with type = "animator" or
/// [states]): parsed at import, its conditions checked for syntax, its
/// problems errors with the line, unknown keys warnings.
#[test]
fn an_animator_imports_with_its_states_and_its_mistakes_have_lines() {
    use argus_assets::{Num, Op, Operand};
    let dir = project("animator");
    write(
        &dir,
        "anim/hero.toml",
        "type = \"animator\"\ninitial = \"idle\"\ncolour = \"red\"\n\n[states.idle]\nclip = \"idle\"\n\n[[states.idle.transitions]]\nto = \"run\"\nwhen = \"Body.grounded and abs(Body.vel.x) > 0.5\"\nblend = 0.1\n\n[states.run]\nclip = \"run\"\nfps = \"Feel.run_fps\"\nwindup = 2\nanticipation = 3\nevents = [{ frame = 1, name = \"footstep\", sound = \"sfx/step\" }, { frame = 3, sound = \"sfx/step\" }]\n\n[[states.run.transitions]]\nto = \"idle\"\nwhen = \"not Body.grounded\"\nwindow = [2, 3]\n\n[[any]]\nto = \"idle\"\nwhen = \"end\"\n",
    );
    // Found by its keys, without a type.
    write(&dir, "anim/blink.toml", "[states.on]\nclip = \"blink\"\n");
    let mut imp = Importer::new(&dir);
    let rep = imp.scan();
    assert!(rep.failed.is_empty(), "{:?}", rep.failed);
    let e = &imp.entries()["anim/hero"];
    assert_eq!(e.kind(), "animator");
    assert_eq!(messages(e), ["unknown key \"colour\", ignored"]);
    assert_eq!(e.warnings[0].at, "assets/anim/hero.toml:3");
    let Asset::Animator(a) = &e.asset else {
        panic!()
    };
    assert_eq!(a.initial, "idle");
    let run = a.state("run").unwrap();
    assert_eq!(run.fps, Some(Num::Field("Feel.run_fps".into())));
    assert_eq!(
        (run.windup, run.anticipation.clone()),
        (2, Some(Num::Value(3.0)))
    );
    assert_eq!(run.events.len(), 2);
    assert_eq!(
        run.events[1].name, "sound",
        "an event with only a sound is named sound"
    );
    assert_eq!(run.transitions[0].window, Some([2, 3]));
    let idle = a.state("idle").unwrap();
    let w = &idle.transitions[0].when;
    assert_eq!((w.groups.len(), w.groups[0].len()), (1, 2));
    assert_eq!(
        w.groups[0][1].what,
        Operand::Field {
            path: "Body.vel.x".into(),
            abs: true
        }
    );
    assert_eq!(w.groups[0][1].cmp, Some((Op::Gt, 0.5)));
    assert!(
        idle.transitions[0].at.ends_with(":9"),
        "{}",
        idle.transitions[0].at
    );
    assert_eq!(a.any[0].when.groups[0][0].what, Operand::End);
    assert_eq!(imp.entries()["anim/blink"].kind(), "animator");
    assert_eq!(imp.animators().count(), 2);
    let j = e.to_json();
    assert_eq!(j["states"][0]["name"], "idle");

    // Mistakes are errors naming the line.
    for (text, line, words) in [
        ("[states.a]\nclip = \"a\"\n\n[[states.a.transitions]]\nto = \"b\"\nwhen = \"end\"\n", 5, "not a state; the states are a"),
        ("[states.a]\nclip = \"a\"\n\n[[states.a.transitions]]\nto = \"a\"\nwhen = \"end or\"\n", 6, "ends where a value was expected"),
        ("face = \"grounded\"\n[states.a]\nclip = \"a\"\n", 1, "face: "),
        ("face = 3\n[states.a]\nclip = \"a\"\n", 1, "face is a field path"),
        ("[states.a]\nclip = \"a\"\n\n[[states.a.transitions]]\nto = \"a\"\nwhen = \"Body = 1\"\n", 6, "use == or !="),
        ("[states.a]\nclip = \"a\"\n\n[[states.a.transitions]]\nto = \"a\"\nwhen = \"grounded\"\n", 6, "not a field path"),
        ("[states.a]\nclip = \"a\"\nfps = true\n", 3, "a number or a field path"),
        ("initial = \"b\"\n[states.a]\nclip = \"a\"\n", 1, "initial = \"b\" is not a state"),
        ("[states.a]\nloop = true\n", 2, "needs clip"),
    ] {
        write(&dir, "anim/bad.toml", &format!("type = \"animator\"\n{text}"));
        let rep = imp.scan();
        let f = rep.failed.iter().find(|f| f.name == "anim/bad").unwrap_or_else(|| panic!("{text} did not fail"));
        assert!(f.error.message.contains(words), "{text}: {}", f.error.message);
        assert_eq!(f.error.r#ref.as_deref(), Some(format!("assets/anim/bad.toml:{}", line + 1).as_str()), "{text}: {}", f.error.message);
    }
}

/// Item 10: a particle effect is an asset (type = "emitter", or found by
/// rate, burst, or lifetime): its ranges and curves checked at import, its
/// mistakes errors with the line, unknown keys warnings.
#[test]
fn an_emitter_imports_with_its_curves_and_its_mistakes_have_lines() {
    use argus_assets::{Area, Space};
    let dir = project("emitter");
    write(
        &dir,
        "fx/sparks.toml",
        "type = \"emitter\"\nsprite = \"fx/spark\"\nshape = \"circle\"\nradius = 0.25\nrate = 120\nburst = 30\nmax = 2000\nlifetime = [0.4, 0.9]\nspeed = 4\ndirection = 90\nspread = 60\ngravity = [0, -12]\ndrag = 1.5\nspace = \"local\"\nsize = [[0, 0.1], [0.5, 0.3], [1, 0]]\ncolor = [[0, \"#FFFFFFFF\"], [1, \"#FF000000\"]]\nglow = true\n",
    );
    // Found by its keys, without a type; everything else defaults.
    write(&dir, "fx/puff.toml", "burst = 8\n");
    let mut imp = Importer::new(&dir);
    let rep = imp.scan();
    assert!(rep.failed.is_empty(), "{:?}", rep.failed);
    let e = &imp.entries()["fx/sparks"];
    assert_eq!(e.kind(), "emitter");
    assert_eq!(messages(e), ["unknown key \"glow\", ignored"]);
    assert_eq!(e.warnings[0].at, "assets/fx/sparks.toml:17");
    let Asset::Emitter(f) = &e.asset else {
        panic!()
    };
    assert_eq!(f.sprite.as_deref(), Some("fx/spark"));
    assert_eq!(f.area, Area::Circle { radius: 0.25 });
    assert_eq!((f.rate, f.burst, f.max), (120.0, 30, 2000));
    assert_eq!(f.lifetime, [0.4, 0.9]);
    assert_eq!(f.speed, [4.0, 4.0], "a number is a range of one");
    assert_eq!(f.gravity, [0.0, -12.0]);
    assert_eq!(f.space, Space::Local);
    assert!(
        (f.size.at(0.25) - 0.2).abs() < 1e-6,
        "curves are linear between points"
    );
    assert_eq!(f.size.at(1.0), 0.0);
    assert_eq!(
        f.velocity.at(0.7),
        1.0,
        "a missing curve is its default constant"
    );
    assert_eq!(f.color.points[1].1, [255, 0, 0, 0]);
    let puff = &imp.entries()["fx/puff"];
    assert_eq!(puff.kind(), "emitter");
    let Asset::Emitter(p) = &puff.asset else {
        panic!()
    };
    assert_eq!(
        (p.burst, p.max, p.particle.as_str(), p.area),
        (8, 1000, "rect", Area::Point)
    );
    assert_eq!(imp.emitters().count(), 2);
    assert_eq!(e.to_json()["space"], "local");

    // Mistakes are errors naming the line.
    for (text, line, words) in [
        ("lifetime = 0\n", 1, "lifetime must be above 0"),
        ("lifetime = [2, 1]\n", 1, "min first"),
        ("rate = -3\n", 1, "rate is a number from 0"),
        ("burst = 1\nsize = [[0.5, 1], [0.2, 2]]\n", 2, "never back"),
        ("burst = 1\nsize = [[0, 1], [2, 2]]\n", 2, "never back"),
        ("burst = 1\nsize = [[0, 1, 2]]\n", 2, "is [t, value]"),
        ("burst = 1\ncolor = [[0, \"red\"]]\n", 2, "not #RRGGBB"),
        ("burst = 1\nshape = \"star\"\n", 2, "point, circle, box"),
        ("burst = 1\nspace = \"screen\"\n", 2, "world and local"),
        ("burst = 1\nparticle = \"star\"\n", 2, "rect and circle"),
        ("burst = 1\ngravity = 3\n", 2, "gravity is [x, y]"),
    ] {
        write(&dir, "fx/bad.toml", &format!("type = \"emitter\"\n{text}"));
        let rep = imp.scan();
        let f = rep
            .failed
            .iter()
            .find(|f| f.name == "fx/bad")
            .unwrap_or_else(|| panic!("{text} did not fail"));
        assert!(
            f.error.message.contains(words),
            "{text}: {}",
            f.error.message
        );
        assert_eq!(
            f.error.r#ref.as_deref(),
            Some(format!("assets/fx/bad.toml:{}", line + 1).as_str()),
            "{text}: {}",
            f.error.message
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// Item 11: a material is a .wgsl, with a sidecar for its parameters, or a
/// sidecar naming a built-in's shader. The import composes and checks the
/// shader, so a compile error fails it with the shader's own line; the
/// built-ins are there beside the project's, and a project asset of the
/// same name shadows one.
#[test]
fn a_material_imports_checked_and_its_shader_errors_have_lines() {
    let dir = project("material");
    let plain = "fn material(in: MaterialInput, p: Params) -> vec4<f32> {\n    return sample_atlas(in, in.uv) * p.tint;\n}\n";
    write(&dir, "materials/tint.wgsl", plain);
    write(
        &dir,
        "materials/tint.toml",
        "type = \"material\"\ndoc = \"Paints the sprite\"\n\n[params.tint]\ntype = \"color\"\ndefault = \"#FF0000\"\nfrom = \"Paint.color\"\n",
    );
    write(
        &dir,
        "materials/thick.toml",
        "shader = \"outline\"\n\n[params.width]\ndefault = 3\n",
    );
    write(
        &dir,
        "materials/bare.wgsl",
        "fn material(in: MaterialInput, p: Params) -> vec4<f32> {\n    return in.color;\n}\n",
    );
    write(
        &dir,
        "fx/glow.toml",
        "type = \"emitter\"\nburst = 3\nmaterial = \"materials/tint\"\n",
    );
    let mut imp = Importer::new(&dir);
    let rep = imp.scan();
    assert!(rep.failed.is_empty(), "{:?}", rep.failed);
    let e = &imp.entries()["materials/tint"];
    assert_eq!(e.kind(), "material");
    let Asset::Material(m) = &e.asset else {
        panic!()
    };
    assert_eq!(m.shader_file, "assets/materials/tint.wgsl");
    assert_eq!(
        m.param("tint").unwrap().from.as_ref().unwrap().path,
        "Paint.color"
    );
    let j = e.to_json();
    assert_eq!(j["params"][0]["from"], "Paint.color");
    assert_eq!(j["blend"], "alpha");
    assert_eq!(
        imp.entries()["materials/thick"].kind(),
        "material",
        "found by shader, without a type"
    );
    assert_eq!(
        imp.entries()["materials/bare"].kind(),
        "material",
        "a shader alone"
    );
    let Asset::Emitter(fx) = &imp.entries()["fx/glow"].asset else {
        panic!()
    };
    assert_eq!(fx.material.as_deref(), Some("materials/tint"));
    // Every material: the project's three and the library's six.
    let names: Vec<&str> = imp.materials().iter().map(|(n, _)| *n).collect();
    assert_eq!(
        names,
        [
            "materials/bare",
            "materials/thick",
            "materials/tint",
            "unlit",
            "lit",
            "palette",
            "dissolve",
            "outline",
            "sdf_text"
        ]
    );

    // A compile error fails the import with the line of the .wgsl.
    write(&dir, "materials/tint.wgsl", "fn material(in: MaterialInput, p: Params) -> vec4<f32> {\n    return sample_atlas(in, in.uv) * p.tnit;\n}\n");
    let rep = imp.poll().expect("the tree changed");
    let f = rep
        .failed
        .iter()
        .find(|f| f.name == "materials/tint")
        .expect("it fails");
    assert_eq!(f.error.code, argus_material::codes::SHADER);
    assert_eq!(
        f.error.r#ref.as_deref(),
        Some("assets/materials/tint.wgsl:2"),
        "{}",
        f.error.message
    );
    assert!(f.error.message.contains("tnit"), "{}", f.error.message);
    // Fixed, it imports again.
    write(&dir, "materials/tint.wgsl", plain);
    let rep = imp.poll().unwrap();
    assert!(
        rep.imported.contains(&"materials/tint".to_string()),
        "{rep:?}"
    );
    assert!(imp.failures().is_empty());

    // A project asset named like a built-in takes its place.
    write(
        &dir,
        "outline.wgsl",
        plain.replace("p.tint", "in.color").as_str(),
    );
    imp.poll().unwrap();
    let outline: Vec<&str> = imp
        .materials()
        .iter()
        .filter(|(n, _)| *n == "outline")
        .map(|(_, m)| m.shader_file.as_str())
        .collect();
    assert_eq!(outline, ["assets/outline.wgsl"]);
    // One name, two sources, is ambiguous.
    sheet(&dir, "materials/bare.png", 4, 4, 1, 1, None);
    let rep = imp.poll().unwrap();
    let f = rep
        .failed
        .iter()
        .find(|f| f.name == "materials/bare")
        .unwrap();
    assert!(
        f.error.message.contains("one name is one asset"),
        "{}",
        f.error.message
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ----- phase 3 item 1: fonts ----------------------------------------------------

/// The OFL font Delve ships (Liberation Sans, with its licence beside it).
fn liberation() -> Vec<u8> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/delve/assets/fonts/LiberationSans-Regular.ttf");
    std::fs::read(p).unwrap()
}

#[test]
fn a_font_imports_as_a_distance_field_atlas_of_the_glyphs_text_needs() {
    let root = project("font");
    std::fs::create_dir_all(root.join("assets/fonts")).unwrap();
    std::fs::write(root.join("assets/fonts/sans.ttf"), liberation()).unwrap();
    let mut imp = Importer::new(&root);
    let t = std::time::Instant::now();
    let r = imp.scan();
    println!("imported in {:?}", t.elapsed());
    assert_eq!(r.imported, ["fonts/sans"], "{:?}", r.failed);
    let e = imp.get("fonts/sans").unwrap();
    assert_eq!(e.kind(), "font");
    assert!(e.warnings.is_empty(), "{:?}", e.warnings);
    let Asset::Font(f) = e.asset.clone() else {
        panic!()
    };
    assert_eq!(f.family, "Liberation Sans");
    assert_eq!((f.size, f.spread), (32.0, 4.0));
    assert_eq!(f.units_per_em, 2048);
    assert!(f.metrics[0] > 0 && f.metrics[1] < 0, "{:?}", f.metrics);
    // Every printable character has a glyph with a cell (bar the space).
    let bytes = f.bytes.clone();
    let face = ttf_parser::Face::parse(&bytes, 0).unwrap();
    for c in '!'..='~' {
        let g = face.glyph_index(c).unwrap().0;
        assert!(
            f.glyphs.binary_search_by_key(&g, |b| b.id).is_ok(),
            "{c:?} baked"
        );
    }
    println!(
        "{} glyphs, atlas {:?}, {} KB",
        f.glyphs.len(),
        f.atlas,
        f.field.len() / 1024
    );
    assert!(f.glyphs.len() > 180, "{}", f.glyphs.len());
    assert!(f.atlas[0] <= 1024 && f.atlas[1] <= 1024, "{:?}", f.atlas);
    // The field: 0.5 on an edge, so an "l" (a bar) is inside at its middle
    // and outside a spread away from its sides.
    let l = face.glyph_index('l').unwrap().0;
    let b = &f.glyphs[f.glyphs.binary_search_by_key(&l, |b| b.id).unwrap()];
    let at = |x: u32, y: u32| f.field[((b.rect[1] + y) * f.atlas[0] + b.rect[0] + x) as usize];
    let mid = (b.rect[2] / 2, b.rect[3] / 2);
    assert!(at(mid.0, mid.1) > 150, "inside: {}", at(mid.0, mid.1));
    assert!(at(0, mid.1) < 20, "outside: {}", at(0, mid.1));
    assert_eq!(f.rgba().len(), f.field.len() * 4);

    // A sidecar sets the size, spread, and more characters; a bad setting
    // fails with its line; an unknown one warns.
    write(
        &root,
        "fonts/sans.toml",
        "size = 24\nspread = 3\nchars = \"\u{3b1}\u{3b2}\u{3b3}\"\nweight = 400\n",
    );
    let r = imp.scan();
    assert_eq!(r.imported, ["fonts/sans"]);
    let e = imp.get("fonts/sans").unwrap();
    let Asset::Font(g) = &e.asset else { panic!() };
    assert_eq!((g.size, g.spread), (24.0, 3.0));
    let alpha = face.glyph_index('\u{3b1}').unwrap().0;
    assert!(g.glyphs.iter().any(|b| b.id == alpha), "Greek baked");
    assert_eq!(e.warnings.len(), 1, "{:?}", e.warnings);
    assert!(
        e.warnings[0].at.ends_with("sans.toml:4"),
        "{:?}",
        e.warnings
    );
    write(&root, "fonts/sans.toml", "size = 2\n");
    let r = imp.scan();
    assert_eq!(r.failed.len(), 1);
    assert_eq!(
        r.failed[0].error.r#ref.as_deref(),
        Some("assets/fonts/sans.toml:1")
    );
    // A font's sidecar alone says what is missing.
    std::fs::remove_file(root.join("assets/fonts/sans.ttf")).unwrap();
    write(&root, "fonts/sans.toml", "type = \"font\"\n");
    let r = imp.scan();
    assert!(
        r.failed[0].error.message.contains("sans.ttf"),
        "{}",
        r.failed[0].error.message
    );
    // Not a font.
    write(&root, "fonts/bad.ttf", "not a font");
    let r = imp.scan();
    assert!(
        r.failed
            .iter()
            .any(|f| f.name == "fonts/bad" && f.error.message.contains("not a font")),
        "{:?}",
        r.failed
    );
}

/// A Spine export changed on disk is imported again (found adding art
/// scripts, 2026-10-11: a skeleton's JSON was not among the files a scan
/// compares, so a new export kept the old skeleton until the daemon
/// restarted).
#[test]
fn a_changed_skeleton_is_imported_again() {
    let dir = project("spine-change");
    let one = r#"{ "skeleton": { "spine": "4.1.24" }, "bones": [ { "name": "root" } ] }"#;
    let two = r#"{ "skeleton": { "spine": "4.1.24" }, "bones": [ { "name": "root" }, { "name": "arm", "parent": "root" } ] }"#;
    write(&dir, "puppet/puppet.json", one);
    let mut imp = Importer::new(&dir);
    let rep = imp.scan();
    assert!(rep.failed.is_empty(), "{:?}", rep.failed);
    write(&dir, "puppet/puppet.json", two);
    let rep = imp.scan();
    assert_eq!(rep.imported, vec!["puppet/puppet".to_string()], "{rep:?}");
    std::fs::remove_dir_all(&dir).ok();
}
