//! Reaching players item 2: a game's own icon and name where each platform
//! looks. Every template's and example game's icon is a sprite's frame; the
//! icon files read back; a Windows executable gets the icon and version in
//! its resources; a Mac build is an `.app`; a web build has a manifest.

use argus_cli::identity::{self, Icon};
use std::path::{Path, PathBuf};

/// A temp directory removed on drop.
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp(name: &str) -> Temp {
    let d = std::env::temp_dir().join(format!("argus-identity-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    Temp(d)
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let name = e.file_name();
        // Not the import cache, builds, or anything heavy beside the game.
        if [".argus", "dist", "target"].iter().any(|n| name == *n) {
            continue;
        }
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &to.join(&name));
        } else {
            std::fs::copy(&p, to.join(&name)).unwrap();
        }
    }
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A project's icon, from a copy of it (an icon is imported, which writes
/// the import cache).
fn icon_of(dir: &Path, t: &Temp) -> Icon {
    let name = dir.file_name().unwrap().to_string_lossy().into_owned();
    let copy = t.0.join(&name);
    copy_dir(dir, &copy);
    let cfg = argus_scene::ProjectConfig::load(&copy).unwrap();
    Icon::load(&copy, &cfg.game)
        .unwrap_or_else(|e| panic!("{name}: {e}"))
        .unwrap_or_else(|| panic!("{name} names no icon"))
}

#[test]
fn every_template_and_example_has_an_icon_from_its_sprites() {
    let t = temp("all");
    let shown = std::env::var("ARGUS_ICONS_TO").ok().map(PathBuf::from);
    let mut dirs: Vec<PathBuf> = ["blank", "platformer", "arena", "roguelike", "colony"]
        .iter()
        .map(|n| repo().join("templates").join(n))
        .collect();
    dirs.extend(
        ["hollow", "delve", "swarm", "outpost", "toybox", "puppet"]
            .iter()
            .map(|n| repo().join("examples").join(n)),
    );
    for d in dirs {
        let name = d.file_name().unwrap().to_string_lossy().into_owned();
        let icon = icon_of(&d, &t);
        let rgba = icon.scaled(256);
        let seen = rgba.chunks(4).filter(|p| p[3] > 0).count();
        assert!(
            seen > 256 * 256 / 10,
            "{name}: the icon is mostly clear ({seen} pixels)"
        );
        // The drawn part fills the square on its longer side (whole-pixel
        // scaling leaves at most a thin clear border): a frame's clear
        // margins are left off (Outpost's colonist was a small figure).
        let (mut lo, mut hi) = ([256u32; 2], [0u32; 2]);
        for (i, p) in rgba.chunks(4).enumerate() {
            if p[3] > 0 {
                let (x, y) = (i as u32 % 256, i as u32 / 256);
                lo = [lo[0].min(x), lo[1].min(y)];
                hi = [hi[0].max(x + 1), hi[1].max(y + 1)];
            }
        }
        let longer = (hi[0] - lo[0]).max(hi[1] - lo[1]);
        assert!(
            longer >= 256 * 85 / 100,
            "{name}: the drawn part spans {longer} of 256"
        );
        if let Some(to) = &shown {
            std::fs::create_dir_all(to).unwrap();
            std::fs::write(to.join(format!("{name}.png")), icon.png(256).unwrap()).unwrap();
        }
    }
}

#[test]
fn the_icon_files_read_back() {
    let t = temp("files");
    let icon = icon_of(&repo().join("templates/platformer"), &t);
    let ico = identity::ico(&icon).unwrap();
    assert_eq!(&ico[..4], &[0, 0, 1, 0], "an icon file");
    let count = u16::from_le_bytes([ico[4], ico[5]]);
    assert_eq!(count, 7);
    // The last entry is 256 across, written as 0, and a PNG.
    let e = 6 + 16 * 6;
    assert_eq!(ico[e], 0);
    let off = u32::from_le_bytes(ico[e + 12..e + 16].try_into().unwrap()) as usize;
    assert_eq!(&ico[off..off + 4], b"\x89PNG");
    let icns = identity::icns(&icon).unwrap();
    assert_eq!(&icns[..4], b"icns");
    assert_eq!(
        u32::from_be_bytes(icns[4..8].try_into().unwrap()) as usize,
        icns.len()
    );
    // Scaled up by whole pixels: an even grid, with a clear border.
    let rgba = icon.scaled(256);
    assert_eq!(rgba.len(), 256 * 256 * 4);
}

#[cfg(windows)]
#[test]
fn a_windows_executable_gets_the_icon_and_the_version() {
    let t = temp("exe");
    let icon = icon_of(&repo().join("templates/platformer"), &t);
    // A real executable to rewrite: this test's own program.
    let exe = t.0.join("game.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
    let game = argus_scene::GameInfo {
        title: "Hop & Coin".to_string(),
        version: "1.2.3".to_string(),
        icon: Some("sprites".to_string()),
        icon_frame: 0,
    };
    identity::windows(&exe, &game, Some(&icon)).unwrap();
    let image = editpe::Image::parse_file(&exe).unwrap();
    let res = image.resource_directory().expect("resources");
    assert!(res.get_main_icon().unwrap().is_some(), "the icon is there");
    let v = res.get_version_info().unwrap().expect("version info");
    let s = &v.strings[0].strings;
    assert_eq!(s["ProductName"], "Hop & Coin");
    assert_eq!(s["FileVersion"], "1.2.3");
    assert_eq!(s["OriginalFilename"], "game.exe");
    let fv = v.info.file_version;
    assert_eq!((fv.major, fv.minor), ((1 << 16) | 2, 3 << 16));
}

#[test]
fn a_mac_build_is_an_app_and_a_web_build_has_a_manifest() {
    let t = temp("mac");
    let icon = icon_of(&repo().join("templates/blank"), &t);
    let out = t.0.join("out");
    std::fs::create_dir_all(out.join("data")).unwrap();
    std::fs::write(out.join("data/game.pack"), b"pack").unwrap();
    std::fs::write(out.join("blank"), b"program").unwrap();
    std::fs::write(out.join("libsteam_api.dylib"), b"library").unwrap();
    let game = argus_scene::GameInfo {
        title: "Blank <Game>".to_string(),
        version: "2.0".to_string(),
        icon: Some("player".to_string()),
        icon_frame: 0,
    };
    let exe = identity::mac_app(&out, &out.join("blank"), "blank", &game, Some(&icon)).unwrap();
    // The folder takes no < or >; the plist has the title as it is.
    let app = out.join("Blank -Game-.app/Contents");
    assert_eq!(exe, app.join("MacOS/blank"));
    assert!(app.join("MacOS/libsteam_api.dylib").is_file());
    assert!(app.join("Resources/data/game.pack").is_file());
    assert!(app.join("Resources/icon.icns").is_file());
    assert!(!out.join("data").exists() && !out.join("blank").exists());
    let plist = std::fs::read_to_string(app.join("Info.plist")).unwrap();
    for want in [
        "<key>CFBundleExecutable</key><string>blank</string>",
        "<string>Blank &lt;Game&gt;</string>",
        "<key>CFBundleShortVersionString</key><string>2.0</string>",
        "<key>CFBundleIconFile</key><string>icon</string>",
    ] {
        assert!(plist.contains(want), "{want} not in\n{plist}");
    }

    let web = t.0.join("web");
    std::fs::create_dir_all(&web).unwrap();
    std::fs::write(
        web.join("index.html"),
        "<!doctype html>\n<title>Blank</title>\n<style>\n</style>\n",
    )
    .unwrap();
    identity::web(&web, &game, Some(&icon)).unwrap();
    let m: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(web.join("manifest.webmanifest")).unwrap())
            .unwrap();
    assert_eq!(m["name"], "Blank <Game>");
    assert_eq!(m["icons"][1]["src"], "icon-512.png");
    assert!(web.join("icon-192.png").is_file() && web.join("icon-512.png").is_file());
    let page = std::fs::read_to_string(web.join("index.html")).unwrap();
    assert!(
        page.contains("<link rel=\"manifest\"")
            && page.contains("<link rel=\"icon\" href=\"icon-192.png\">")
    );
}
