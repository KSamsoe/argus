//! Art scripts (asset generation, item 2): `assets/<name>.art.luau` draws
//! a sprite with the pixel-art kit, and the importer packs and caches it as
//! it does a PNG; the same script is the same pixels; its mistakes are
//! errors with the line; it cannot reach a clock, a file, or chance.

use argus_assets::{decode_png, Asset, Importer, SpriteAsset};

fn project(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("argus-art-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    dir
}

fn write(root: &std::path::Path, rel: &str, text: &str) {
    let p = root.join("assets").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn sprite(imp: &Importer, name: &str) -> SpriteAsset {
    match &imp.entries()[name].asset {
        Asset::Sprite(s) => s.clone(),
        other => panic!("not a sprite: {other:?}"),
    }
}

/// The colour at (x, y) of frame `f`, from the cached texture.
fn px(s: &SpriteAsset, f: usize, x: u32, y: u32) -> [u8; 4] {
    let (w, _, rgba) = decode_png(&s.texture).unwrap();
    let r = s.frames[f].rect;
    let i = (((r[1] + y) * w + r[0] + x) * 4) as usize;
    [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
}

const HERO: &str = r##"
-- A 12 x 12 slime, two frames of a bounce.
local pal = { body = "#4CAF50", shine = "#A5D6A7", eye = "#1B1B1B", line = "#1E3A20" }
local function slime(squash)
  local c = art.canvas(12, 12)
  c:ellipse(5.5, 7 + squash, 5 - squash, 4 - squash, pal.body)
  c:pixel(4, 6, pal.eye)
  c:pixel(7, 6, pal.eye)
  c:shade{ light = pal.shine, from = {-1, -1} }
  c:outline(pal.line)
  return c
end
local up, down = slime(0), slime(1)
return art.sprite{
  pixels_per_unit = 12,
  pivot = {0.5, 0},
  clips = { bounce = { up, down, fps = 4 } },
}
"##;

#[test]
fn a_script_draws_a_sprite_the_importer_packs_and_caches() {
    let dir = project("hero");
    write(&dir, "sprites/slime.art.luau", HERO);
    let mut imp = Importer::new(&dir);
    let rep = imp.scan();
    assert!(rep.failed.is_empty(), "{:?}", rep.failed);
    let s = sprite(&imp, "sprites/slime");
    assert_eq!(s.frames.len(), 2);
    assert_eq!(s.pixels_per_unit, 12.0);
    assert_eq!(s.frames[0].pivot, [0.5, 0.0]);
    assert_eq!(s.frames[0].rect[2..], [12, 12]);
    let clip = &s.clips["bounce"];
    assert_eq!(
        (clip.frames.clone(), clip.fps, clip.looping),
        (vec![0, 1], Some(4.0), true)
    );
    // The eyes where they were drawn, the outline round the body, clear
    // outside it.
    assert_eq!(px(&s, 0, 4, 6), [0x1B, 0x1B, 0x1B, 255]);
    assert_eq!(px(&s, 0, 0, 0)[3], 0, "the corner is clear");
    assert_eq!(
        px(&s, 0, 5, 2),
        [0x1E, 0x3A, 0x20, 255],
        "the outline above the body"
    );
    assert_eq!(
        px(&s, 0, 5, 3),
        [0xA5, 0xD6, 0xA7, 255],
        "the shine on its top"
    );
    assert_eq!(s.source.as_deref(), Some("sprites/slime.art.luau"));

    // The same script, the same pixels; a changed one is imported again.
    let first = std::fs::read(&s.texture).unwrap();
    let mut again = Importer::new(&dir);
    again.scan();
    assert_eq!(
        std::fs::read(&sprite(&again, "sprites/slime").texture).unwrap(),
        first
    );
    write(
        &dir,
        "sprites/slime.art.luau",
        &HERO.replace("#4CAF50", "#E53935"),
    );
    let rep = imp.scan();
    assert_eq!(rep.imported, vec!["sprites/slime".to_string()]);
    assert_ne!(
        std::fs::read(&sprite(&imp, "sprites/slime").texture).unwrap(),
        first
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Every drawing call in the kit, pinned: a change to any of them shows
/// here, on any machine (the gate runs it on Linux and in a web build's
/// import too).
#[test]
fn the_kit_draws_the_same_pixels_every_time() {
    let dir = project("kit");
    write(
        &dir,
        "kit.art.luau",
        r##"
local r = art.rng(7)
local c = art.canvas(32, 32, "#10141C")
c:rect(1, 1, 6, 4, "#FF0000")
c:box(8, 1, 6, 4, "#00FF00")
c:line(0, 31, 31, 6, "#FFFFFF")
c:circle(6, 14, 4, "#3366FF")
c:ring(18, 14, 4, "#FFCC00")
c:ellipse(26, 14, 4, 2, "#FF66CC")
c:poly({ 2, 20, 10, 20, 6, 28 }, "#88FF88")
c:dither(12, 20, 8, 8, "#FFFFFF", 0.5)
c:noise(22, 20, 8, 8, { "#202020", "#606060", "#A0A0A0" }, 3, 4)
c:speckle({ "#FF00FF" }, 0.05, 9)
local s = art.canvas(8, 8)
s:circle(3.5, 3.5, 3, art.hsv(r:int(0, 359), 0.6, 0.9))
s:outline("#000000", true)
s:mirror("x")
c:draw(s:flip("x"):rotate(1), 23, 0)
c:draw(s:scale(2):crop(0, 0, 8, 8), 15, 24)
c:recolor({ ["#FF0000"] = art.mix("#FF0000", "#0000FF", 0.5) })
c:fill(0, 0, art.darken("#10141C", 0.5))
local frames = art.grid(c, 16, 16)
assert(#frames == 4 and r:chance(1) and math.floor(math.sin(1) * 1000) == 841)
return art.sprite{ frames = frames }
"##,
    );
    let mut imp = Importer::new(&dir);
    let rep = imp.scan();
    assert!(rep.failed.is_empty(), "{:?}", rep.failed);
    let s = sprite(&imp, "kit");
    assert_eq!(s.frames.len(), 4);
    let (_, _, rgba) = decode_png(&s.texture).unwrap();
    let h = rgba.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3)
    });
    assert_eq!(h, PINNED_KIT, "the kit's pixels changed");
    std::fs::remove_dir_all(&dir).ok();
}

const PINNED_KIT: u64 = 4820063342416535357;

#[test]
fn a_scripts_mistakes_are_errors_with_the_line_and_chance_and_clocks_are_not_there() {
    let dir = project("bad");
    let mut imp = Importer::new(&dir);
    for (text, words, line) in [
        ("local c = art.canvas(4, 4)\nc:pixel(0, 0, \"red\")\nreturn art.sprite{ frames = { c } }\n", "\"red\" is not a colour", Some(2)),
        ("local c = art.canvas(4, 4\nreturn c\n", "", Some(2)),
        ("local x = math.random(1, 4)\n", "art.rng(seed)", Some(1)),
        ("local t = os.clock()\n", "os", Some(1)),
        ("return 7\n", "returned a number", None),
        ("local c = art.canvas(4, 4)\nreturn art.sprite{ frames = { c }, clips = { walk = { 0, 3 } } }\n", "clip walk names frame 3", None),
        ("return art.sprite{ frames = { art.canvas(4, 4) }, sheet = 1 }\n", "a key \"sheet\"", None),
        ("return art.sprite{ frames = { art.canvas(5000, 4) } }\n", "1 to 2048 pixels", Some(1)),
    ] {
        write(&dir, "bad.art.luau", text);
        let rep = imp.scan();
        let f = rep
            .failed
            .iter()
            .find(|f| f.name == "bad")
            .unwrap_or_else(|| panic!("{text} did not fail"));
        assert!(f.error.message.contains(words), "{text}: {}", f.error.message);
        let want = match line {
            Some(l) => format!("assets/bad.art.luau:{l}"),
            None => "assets/bad.art.luau".to_string(),
        };
        assert_eq!(f.error.r#ref.as_deref(), Some(want.as_str()), "{text}: {}", f.error.message);
    }
    // A sidecar beside it is not read, and says so; what it prints is a
    // warning.
    write(
        &dir,
        "bad.art.luau",
        "print(\"hello\", 3)\nreturn art.sprite{ frames = { art.canvas(2, 2, \"#FFFFFF\") } }\n",
    );
    write(&dir, "bad.toml", "pixels_per_unit = 8\n");
    let rep = imp.scan();
    assert!(rep.failed.is_empty(), "{:?}", rep.failed);
    let msgs: Vec<String> = imp.entries()["bad"]
        .warnings
        .iter()
        .map(|w| w.message.clone())
        .collect();
    assert!(msgs.iter().any(|m| m.contains("is not read")), "{msgs:?}");
    assert!(
        msgs.iter().any(|m| m == "the script printed: hello 3"),
        "{msgs:?}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Pixel art as text: each character a colour from the key; one the key
/// does not have leaves its pixel alone.
#[test]
fn rows_draw_pixel_art_written_as_text() {
    let dir = project("rows");
    write(
        &dir,
        "key.art.luau",
        r##"
local c = art.canvas(5, 3, "#000000")
c:rows(0, 0, {
  ".#o#.",
  "#ooo#",
  ".#.#.",
}, { ["#"] = "#FFFFFF", o = "#FF0000" })
return art.sprite{ frames = { c } }
"##,
    );
    let mut imp = Importer::new(&dir);
    let rep = imp.scan();
    assert!(rep.failed.is_empty(), "{:?}", rep.failed);
    let s = sprite(&imp, "key");
    assert_eq!(px(&s, 0, 0, 0), [0, 0, 0, 255], "a dot leaves the fill");
    assert_eq!(px(&s, 0, 1, 0), [255, 255, 255, 255]);
    assert_eq!(px(&s, 0, 2, 1), [255, 0, 0, 255]);
    assert_eq!(px(&s, 0, 2, 2), [0, 0, 0, 255]);
    std::fs::remove_dir_all(&dir).ok();
}

/// get answers #RRGGBB for an opaque pixel and #RRGGBBAA otherwise, and
/// alpha how drawn a pixel is.
#[test]
fn get_and_alpha_read_a_pixel() {
    let s = argus_assets::art::run(
        "x.art.luau",
        "local c = art.canvas(3, 1)\nc:pixel(0, 0, \"#123456\")\nc:pixel(1, 0, \"#12345680\")\nassert(c:get(0, 0) == \"#123456\" and c:get(1, 0) == \"#12345680\" and c:get(2, 0) == \"#00000000\" and c:get(5, 0) == nil)\nassert(c:alpha(0, 0) == 255 and c:alpha(1, 0) == 128 and c:alpha(2, 0) == 0 and c:alpha(-1, 0) == 0)\nreturn art.sprite{ frames = { c } }\n",
    );
    assert!(s.is_ok(), "{:?}", s.err().map(|e| e.message));
}

/// A clip can say its frames as a range, as a sidecar's does.
#[test]
fn a_clip_takes_a_range_of_frames() {
    let dir = project("range");
    write(
        &dir,
        "walker.art.luau",
        "local f = {}\nfor i = 1, 5 do f[i] = art.canvas(2, 2, art.hsv(i * 60, 1, 1)) end\nreturn art.sprite{ frames = f, clips = { walk = { from = 1, to = 4, fps = 10 } } }\n",
    );
    let mut imp = Importer::new(&dir);
    let rep = imp.scan();
    assert!(rep.failed.is_empty(), "{:?}", rep.failed);
    assert_eq!(
        sprite(&imp, "walker").clips["walk"].frames,
        vec![1, 2, 3, 4]
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// The sprite generators write art scripts that draw: every kind at
/// several seeds, the same script for the same seed.
#[test]
fn every_sprite_kind_writes_a_script_that_draws() {
    use argus_assets::artgen;
    let out = std::env::var("ARGUS_GEN_OUT").ok();
    for (kind, _) in artgen::KINDS {
        for seed in 1..=4 {
            let text = artgen::generate(kind, seed, "sprites/x").unwrap();
            assert_eq!(text, artgen::generate(kind, seed, "sprites/x").unwrap());
            let s = argus_assets::art::run("sprites/x.art.luau", &text)
                .unwrap_or_else(|e| panic!("{kind} {seed}: {}\n{text}", e.message));
            assert!(!s.frames.is_empty(), "{kind} {seed}");
            for f in &s.frames {
                assert!(
                    f.canvas.rgba.chunks(4).any(|p| p[3] > 0),
                    "{kind} {seed}: an empty frame"
                );
            }
            if let Some(dir) = &out {
                std::fs::create_dir_all(dir).unwrap();
                std::fs::write(format!("{dir}/{kind}-{seed}.art.luau"), &text).unwrap();
                // Its frames in a row, for a person to look at.
                let (w, h) = (
                    s.frames.iter().map(|f| f.canvas.w).sum::<u32>() + s.frames.len() as u32,
                    s.frames.iter().map(|f| f.canvas.h).max().unwrap(),
                );
                let mut px = vec![0u8; (w * h * 4) as usize];
                let mut x0 = 0;
                for f in &s.frames {
                    for y in 0..f.canvas.h {
                        for x in 0..f.canvas.w {
                            let i = ((y * f.canvas.w + x) * 4) as usize;
                            let j = ((y * w + x0 + x) * 4) as usize;
                            px[j..j + 4].copy_from_slice(&f.canvas.rgba[i..i + 4]);
                        }
                    }
                    x0 += f.canvas.w + 1;
                }
                argus_assets::encode_png(
                    std::path::Path::new(&format!("{dir}/{kind}-{seed}.png")),
                    w,
                    h,
                    &px,
                )
                .unwrap();
            }
        }
    }
    assert!(artgen::generate("dragon", 1, "x")
        .unwrap_err()
        .contains("creature, character"));
}

/// What each sprite kind draws at seed 1, pinned: the same seed is the
/// same sprite, version to version and machine to machine.
#[test]
fn generated_sprites_are_pinned_pixels() {
    use argus_assets::artgen;
    let got: Vec<(String, u64)> = artgen::KINDS
        .iter()
        .map(|(kind, _)| {
            let text = artgen::generate(kind, 1, "sprites/x").unwrap();
            let s = argus_assets::art::run("sprites/x.art.luau", &text).unwrap();
            let h = s
                .frames
                .iter()
                .flat_map(|f| f.canvas.rgba.iter())
                .fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
                    (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3)
                });
            (kind.to_string(), h)
        })
        .collect();
    let want: Vec<(String, u64)> = PINNED_KINDS
        .iter()
        .map(|(k, h)| (k.to_string(), *h))
        .collect();
    assert_eq!(got, want);
}

const PINNED_KINDS: [(&str, u64); 6] = [
    ("creature", 17321467844721375506),
    ("character", 7657863114222739389),
    ("item", 8866461461364842317),
    ("tile", 11962154586346593457),
    ("plant", 17621447597582241628),
    ("rock", 4439055114142076959),
];

/// Scripts share a module: `require("colours")` loads
/// assets/colours.lib.luau, and a change to it reimports every script
/// that uses it. A mistake in a module names its own line; a module that
/// is not there says so, and the script imports once it is.
#[test]
fn scripts_share_a_module_and_a_changed_module_reimports_them() {
    let dir = project("modules");
    write(&dir, "colours.lib.luau", "return { grass = \"#00FF00\" }\n");
    let script = "local pal = require(\"colours\")\nlocal c = art.canvas(2, 2, pal.grass)\nreturn art.sprite{ frames = { c } }\n";
    write(&dir, "a.art.luau", script);
    write(&dir, "b.art.luau", script);
    let mut imp = Importer::new(&dir);
    let rep = imp.scan();
    assert!(rep.failed.is_empty(), "{:?}", rep.failed);
    assert_eq!(px(&sprite(&imp, "a"), 0, 0, 0), [0, 255, 0, 255]);
    write(&dir, "colours.lib.luau", "return { grass = \"#0000FF\" }\n");
    let mut rep = imp.scan();
    rep.imported.sort();
    assert_eq!(
        rep.imported,
        vec!["a".to_string(), "b".to_string()],
        "{rep:?}"
    );
    assert_eq!(px(&sprite(&imp, "b"), 0, 0, 0), [0, 0, 255, 255]);
    // A mistake in the module, at its line.
    write(
        &dir,
        "colours.lib.luau",
        "local x = 1\nreturn { grass = nil .. \"x\" }\n",
    );
    let rep = imp.scan();
    let f = rep.failed.iter().find(|f| f.name == "a").expect("a fails");
    assert_eq!(
        f.error.r#ref.as_deref(),
        Some("assets/colours.lib.luau:2"),
        "{}",
        f.error.message
    );
    // No module: said, and imported when it comes back.
    std::fs::remove_file(dir.join("assets/colours.lib.luau")).unwrap();
    let rep = imp.scan();
    let f = rep.failed.iter().find(|f| f.name == "a").expect("a fails");
    assert!(
        f.error
            .message
            .contains("there is no assets/colours.lib.luau"),
        "{}",
        f.error.message
    );
    write(&dir, "colours.lib.luau", "return { grass = \"#FF0000\" }\n");
    let rep = imp.scan();
    assert!(rep.failed.is_empty(), "{:?}", rep.failed);
    assert_eq!(px(&sprite(&imp, "a"), 0, 0, 0), [255, 0, 0, 255]);
    // A module is not an asset.
    assert!(!imp.entries().contains_key("colours"));
    std::fs::remove_dir_all(&dir).ok();
}

/// A script's maths is libm's, the same on every machine: not the
/// built-ins Luau would call for math.sin and the rest (the arena's port
/// measured thousands of differences between the two).
#[test]
fn a_scripts_math_is_libms() {
    let script = "local s = { sin = 0, cos = 0, exp = 0, pow = 0, atan2 = 0, log = 0 }\nfor i = 1, 2000 do\n  local x = i * 0.0137 - 9\n  s.sin += math.sin(x)\n  s.cos += math.cos(x)\n  s.exp += math.exp(x / 4)\n  s.pow += math.pow(1 + i / 1000, 1.37)\n  s.atan2 += math.atan2(x, 1.5)\n  s.log += math.log(i)\nend\nfor _, k in { \"sin\", \"cos\", \"exp\", \"pow\", \"atan2\", \"log\" } do\n  print(k, string.format(\"%.17g\", s[k]))\nend\nreturn art.sprite{ frames = { art.canvas(1, 1) } }\n";
    let got = argus_assets::art::run("m.art.luau", script)
        .unwrap()
        .printed;
    let (mut a, mut b, mut c, mut d, mut e, mut f) = (0f64, 0f64, 0f64, 0f64, 0f64, 0f64);
    for i in 1..=2000 {
        let x = i as f64 * 0.0137 - 9.0;
        a += libm::sin(x);
        b += libm::cos(x);
        c += libm::exp(x / 4.0);
        d += libm::pow(1.0 + i as f64 / 1000.0, 1.37);
        e += libm::atan2(x, 1.5);
        f += libm::log(i as f64);
    }
    let want = [
        ("sin", a),
        ("cos", b),
        ("exp", c),
        ("pow", d),
        ("atan2", e),
        ("log", f),
    ];
    assert_eq!(got.len(), want.len(), "{got:?}");
    // Seventeen digits read back as the very double.
    for (line, (k, v)) in got.iter().zip(want) {
        let (name, num) = line.split_once(' ').unwrap();
        assert_eq!(name, k);
        assert_eq!(
            num.parse::<f64>().unwrap().to_bits(),
            v.to_bits(),
            "{k}: {num} against {v:e}"
        );
    }
}

/// A colour's numbers, for a script's own arithmetic.
#[test]
fn rgba_answers_a_colours_numbers() {
    let s = argus_assets::art::run(
        "x.art.luau",
        "local r, g, b, a = art.rgba(\"#10203040\")\nassert(r == 16 and g == 32 and b == 48 and a == 64)\nlocal _, _, _, o = art.rgba(\"#ABCDEF\")\nassert(o == 255)\nreturn art.sprite{ frames = { art.canvas(1, 1) } }\n",
    );
    assert!(s.is_ok(), "{:?}", s.err().map(|e| e.message));
}

/// A fresh importer (a scene load, a scenario run, a build) takes what an
/// unchanged script drew last time instead of running it again; a change
/// to the script or a module runs it again.
#[test]
fn an_unchanged_script_is_not_run_again() {
    let dir = project("memo");
    write(&dir, "pal.lib.luau", "return { c = \"#00FF00\" }\n");
    write(
        &dir,
        "s.art.luau",
        "local pal = require(\"pal\")\nreturn art.sprite{ frames = { art.canvas(2, 2, pal.c) } }\n",
    );
    let mut imp = Importer::new(&dir);
    assert!(imp.scan().failed.is_empty());
    let tex = sprite(&imp, "s").texture.clone();
    // Mark the cached sheet: if the script ran again, it would draw over it.
    argus_assets::encode_png(&tex, 2, 2, &[255, 0, 0, 255].repeat(4)).unwrap();
    let mut fresh = Importer::new(&dir);
    assert!(fresh.scan().failed.is_empty());
    assert_eq!(
        px(&sprite(&fresh, "s"), 0, 0, 0),
        [255, 0, 0, 255],
        "taken, not drawn again"
    );
    assert_eq!(sprite(&fresh, "s").frames.len(), 1);
    // A module changed: drawn again.
    write(&dir, "pal.lib.luau", "return { c = \"#0000FF\" }\n");
    let mut fresh = Importer::new(&dir);
    assert!(fresh.scan().failed.is_empty());
    assert_eq!(
        px(&sprite(&fresh, "s"), 0, 0, 0),
        [0, 0, 255, 255],
        "the module changed"
    );
    // The script changed: drawn again.
    write(
        &dir,
        "s.art.luau",
        "return art.sprite{ frames = { art.canvas(2, 2, \"#FFFFFF\") } }\n",
    );
    let mut fresh = Importer::new(&dir);
    assert!(fresh.scan().failed.is_empty());
    assert_eq!(
        px(&sprite(&fresh, "s"), 0, 0, 0),
        [255, 255, 255, 255],
        "the script changed"
    );
    std::fs::remove_dir_all(&dir).ok();
}
