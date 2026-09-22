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
    assert!(why["c"].0.contains("neither a sprite nor a sound"));
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
