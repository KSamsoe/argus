//! A game's own icon and name where each platform looks for them
//! (reaching players item 2), with no compiler: `argus.toml`'s `[game]`
//! names the title, the version, and a sprite whose frame is the icon.
//!
//! - Windows: the icon and the version in the executable's resources
//!   (rewritten in place, `editpe`), so Explorer and the taskbar show them.
//! - The web: the page's title and icon, and a manifest, so a phone's "Add
//!   to Home Screen" gives the game's name and icon.
//! - The Mac: an `.app` bundle with the icon (`.icns`) and an `Info.plist`;
//!   the runtime finds its data in the bundle's resources.
//! - Every desktop build: `data/icon.png`, which the runtime sets as the
//!   window's icon, and on Linux the icon beside the executable.
//!
//! The icon is scaled to each size it is needed at: up by whole pixels, so
//! pixel art stays sharp, and down by averaging.

use argus_scene::GameInfo;
use std::path::{Path, PathBuf};

/// A game's icon: a square picture, RGBA.
pub struct Icon {
    size: u32,
    rgba: Vec<u8>,
}

impl Icon {
    /// The project's icon, from `[game] icon` (a sprite asset) and
    /// `icon_frame`; none when the project names none. The frame's drawn
    /// part, its clear margins left off, sits in the middle of a clear
    /// square.
    pub fn load(root: &Path, game: &GameInfo) -> Result<Option<Icon>, String> {
        let Some(name) = &game.icon else {
            return Ok(None);
        };
        let mut imp = argus_assets::Importer::new(root);
        imp.scan();
        let e = imp
            .get(name)
            .map_err(|e| format!("argus.toml [game] icon {name:?}: {}", e.message))?;
        let argus_assets::Asset::Sprite(s) = &e.asset else {
            return Err(format!(
                "argus.toml [game] icon {name:?} is a {}, not a sprite",
                e.kind()
            ));
        };
        let f = s.frames.get(game.icon_frame as usize).ok_or_else(|| {
            format!(
                "argus.toml [game] icon {name:?} has frames 0 to {}, and icon_frame is {}",
                s.frames.len().saturating_sub(1),
                game.icon_frame
            )
        })?;
        let (tw, th, sheet) = argus_assets::decode_png(&s.texture).map_err(|e| e.message)?;
        // The frame's drawn part: a sprite's frame often has clear margins
        // (Outpost's colonist stands in the middle of its frame), and an
        // icon of the whole frame drew the figure small. A frame all clear
        // is kept as it is.
        let [fx, fy, fw, fh] = f.rect;
        let drawn = |i: u32, j: u32| {
            let (sx, sy) = (fx + i, fy + j);
            sx < tw && sy < th && sheet[((sy * tw + sx) * 4 + 3) as usize] > 0
        };
        let (mut x0, mut y0, mut x1, mut y1) = (fw, fh, 0, 0);
        for j in 0..fh {
            for i in 0..fw {
                if drawn(i, j) {
                    (x0, y0) = (x0.min(i), y0.min(j));
                    (x1, y1) = (x1.max(i + 1), y1.max(j + 1));
                }
            }
        }
        let [x, y, w, h] = if x1 > x0 {
            [fx + x0, fy + y0, x1 - x0, y1 - y0]
        } else {
            f.rect
        };
        let size = w.max(h).max(1);
        let (ox, oy) = ((size - w) / 2, (size - h) / 2);
        let mut rgba = vec![0u8; (size * size * 4) as usize];
        for j in 0..h {
            for i in 0..w {
                let (sx, sy) = (x + i, y + j);
                if sx >= tw || sy >= th {
                    continue;
                }
                let from = ((sy * tw + sx) * 4) as usize;
                let to = (((oy + j) * size + ox + i) * 4) as usize;
                rgba[to..to + 4].copy_from_slice(&sheet[from..from + 4]);
            }
        }
        Ok(Some(Icon { size, rgba }))
    }

    /// The icon at `size` by `size`: up by a whole number of pixels a pixel,
    /// centred, so pixel art stays sharp and even (a 13-pixel hero is 19
    /// times at 256, with a clear border); down by averaging each box,
    /// weighted by its alpha so a clear edge does not darken.
    pub fn scaled(&self, size: u32) -> Vec<u8> {
        let s = self.size;
        if size == s {
            return self.rgba.clone();
        }
        let mut out = vec![0u8; (size * size * 4) as usize];
        if size > s {
            let k = size / s;
            let pad = (size - s * k) / 2;
            for y in 0..s * k {
                for x in 0..s * k {
                    let from = (((y / k) * s + x / k) * 4) as usize;
                    let to = (((pad + y) * size + pad + x) * 4) as usize;
                    out[to..to + 4].copy_from_slice(&self.rgba[from..from + 4]);
                }
            }
            return out;
        }
        for y in 0..size {
            for x in 0..size {
                let to = ((y * size + x) * 4) as usize;
                let (x0, x1) = (x * s / size, ((x + 1) * s / size).max(x * s / size + 1));
                let (y0, y1) = (y * s / size, ((y + 1) * s / size).max(y * s / size + 1));
                let (mut r, mut g, mut b, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
                for sy in y0..y1 {
                    for sx in x0..x1 {
                        let p = &self.rgba[((sy * s + sx) * 4) as usize..][..4];
                        let pa = u64::from(p[3]);
                        r += u64::from(p[0]) * pa;
                        g += u64::from(p[1]) * pa;
                        b += u64::from(p[2]) * pa;
                        a += pa;
                        n += 1;
                    }
                }
                // A box all clear stays clear.
                if let (Some(r), Some(g), Some(b)) =
                    (r.checked_div(a), g.checked_div(a), b.checked_div(a))
                {
                    out[to] = r as u8;
                    out[to + 1] = g as u8;
                    out[to + 2] = b as u8;
                    out[to + 3] = (a / n) as u8;
                }
            }
        }
        out
    }

    /// The icon at `size` as PNG bytes.
    pub fn png(&self, size: u32) -> Result<Vec<u8>, String> {
        argus_assets::png_bytes(size, size, &self.scaled(size)).map_err(|e| e.message)
    }
}

/// A Windows icon file: PNGs at the sizes Explorer and the taskbar ask for.
pub fn ico(icon: &Icon) -> Result<Vec<u8>, String> {
    let sizes = [16u32, 24, 32, 48, 64, 128, 256];
    let pngs = sizes
        .iter()
        .map(|&s| icon.png(s))
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = Vec::new();
    out.extend(0u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend((sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (s, p) in sizes.iter().zip(&pngs) {
        // 256 is written as 0.
        let b = if *s >= 256 { 0u8 } else { *s as u8 };
        out.extend([b, b, 0, 0]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((p.len() as u32).to_le_bytes());
        out.extend(offset.to_le_bytes());
        offset += p.len() as u32;
    }
    for p in &pngs {
        out.extend(p);
    }
    Ok(out)
}

/// A Mac icon file: PNGs at its sizes, each with its type's tag.
pub fn icns(icon: &Icon) -> Result<Vec<u8>, String> {
    let entries: [(&[u8; 4], u32); 10] = [
        (b"icp4", 16),
        (b"icp5", 32),
        (b"ic11", 32),
        (b"ic12", 64),
        (b"ic07", 128),
        (b"ic13", 256),
        (b"ic08", 256),
        (b"ic14", 512),
        (b"ic09", 512),
        (b"ic10", 1024),
    ];
    let mut body = Vec::new();
    for (tag, size) in entries {
        let p = icon.png(size)?;
        body.extend(tag);
        body.extend(((p.len() + 8) as u32).to_be_bytes());
        body.extend(p);
    }
    let mut out = Vec::with_capacity(body.len() + 8);
    out.extend(b"icns");
    out.extend(((body.len() + 8) as u32).to_be_bytes());
    out.extend(body);
    Ok(out)
}

/// The version's four numbers, missing ones 0.
fn version_parts(v: &str) -> [u32; 4] {
    let mut out = [0u32; 4];
    for (i, p) in v.split('.').take(4).enumerate() {
        out[i] = p.parse().unwrap_or(0);
    }
    out
}

/// Windows: the icon and the version into the executable's resources.
pub fn windows(exe: &Path, game: &GameInfo, icon: Option<&Icon>) -> Result<(), String> {
    let at = |e: &dyn std::fmt::Display| format!("{}: {e}", exe.display());
    let mut image = editpe::Image::parse_file(exe).map_err(|e| at(&e))?;
    let mut res = image.resource_directory().cloned().unwrap_or_default();
    if let Some(icon) = icon {
        res.set_main_icon(ico(icon)?).map_err(|e| at(&e))?;
    }
    let [a, b, c, d] = version_parts(&game.version);
    let v = editpe::types::VersionU32 {
        major: (a << 16) | (b & 0xffff),
        minor: (c << 16) | (d & 0xffff),
    };
    let file = exe
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut strings = indexmap::IndexMap::with_hasher(foldhash::fast::RandomState::default());
    for (k, val) in [
        ("FileDescription", game.title.as_str()),
        ("FileVersion", game.version.as_str()),
        ("InternalName", file.trim_end_matches(".exe")),
        ("OriginalFilename", file.as_str()),
        ("ProductName", game.title.as_str()),
        ("ProductVersion", game.version.as_str()),
    ] {
        strings.insert(k.to_string(), val.to_string());
    }
    let info = editpe::VersionInfo {
        info: editpe::types::FixedFileInfo {
            file_version: v,
            product_version: v,
            ..Default::default()
        },
        strings: vec![editpe::VersionStringTable {
            key: "040904B0".to_string(),
            strings,
        }],
        // US English, Unicode: the table above.
        vars: vec![editpe::types::VersionU16 {
            major: 0x0409,
            minor: 0x04B0,
        }],
    };
    res.set_version_info(&info).map_err(|e| at(&e))?;
    image.set_resource_directory(res).map_err(|e| at(&e))?;
    image.write_file(exe).map_err(|e| at(&e))
}

/// Every desktop build: the icon in the data, which the runtime sets as
/// the window's.
pub fn data_icon(data: &Path, icon: Option<&Icon>) -> Result<(), String> {
    let Some(icon) = icon else {
        return Ok(());
    };
    let f = data.join("icon.png");
    std::fs::write(&f, icon.png(256)?).map_err(|e| format!("{}: {e}", f.display()))
}

/// Linux: the icon beside the executable, as `<game>.png`.
pub fn linux(exe: &Path, icon: Option<&Icon>) -> Result<(), String> {
    let Some(icon) = icon else {
        return Ok(());
    };
    let f = exe.with_extension("png");
    std::fs::write(&f, icon.png(256)?).map_err(|e| format!("{}: {e}", f.display()))
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The Mac: everything the build put in `out` made into `<title>.app`:
/// the executable and the runtime's files in `Contents/MacOS`, the data in
/// `Contents/Resources/data` (where the runtime looks when there is none
/// beside it), the icon, and the `Info.plist`. Returns the executable's new
/// place.
pub fn mac_app(
    out: &Path,
    exe: &Path,
    game_folder: &str,
    game: &GameInfo,
    icon: Option<&Icon>,
) -> Result<PathBuf, String> {
    // A folder's name every system takes (the plist keeps the title).
    let title: String = game
        .title
        .chars()
        .map(|c| {
            let refused = matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*');
            if c.is_control() || refused {
                '-'
            } else {
                c
            }
        })
        .collect();
    let app = out.join(format!("{title}.app"));
    let io = |e: std::io::Error, p: &Path| format!("{}: {e}", p.display());
    if app.exists() {
        std::fs::remove_dir_all(&app).map_err(|e| io(e, &app))?;
    }
    let macos = app.join("Contents").join("MacOS");
    let resources = app.join("Contents").join("Resources");
    std::fs::create_dir_all(&macos).map_err(|e| io(e, &macos))?;
    std::fs::create_dir_all(&resources).map_err(|e| io(e, &resources))?;
    // The executable and the runtime's files beside it (Steam's library);
    // a build check's pictures stay where they are.
    let mut new_exe = macos.join(exe.file_name().unwrap_or_default());
    for e in std::fs::read_dir(out).map_err(|e| io(e, out))? {
        let e = e.map_err(|e| io(e, out))?;
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p == app {
            continue;
        }
        let to = if p.is_dir() {
            match name.as_str() {
                "data" => resources.join("data"),
                // Rust plugins load from beside the executable.
                "plugins" => macos.join("plugins"),
                _ => continue,
            }
        } else if name.starts_with("look-") || name == "build.json" {
            continue;
        } else {
            macos.join(&name)
        };
        if p == exe {
            new_exe = to.clone();
        }
        std::fs::rename(&p, &to).map_err(|e| io(e, &p))?;
    }
    if let Some(icon) = icon {
        let f = resources.join("icon.icns");
        std::fs::write(&f, icns(icon)?).map_err(|e| io(e, &f))?;
    }
    let id: String = game_folder
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let exe_name = new_exe
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    let icon_key = if icon.is_some() {
        "  <key>CFBundleIconFile</key><string>icon</string>\n"
    } else {
        ""
    };
    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>CFBundleName</key><string>{t}</string>\n  <key>CFBundleDisplayName</key><string>{t}</string>\n  <key>CFBundleExecutable</key><string>{e}</string>\n  <key>CFBundleIdentifier</key><string>org.argus-games.{id}</string>\n  <key>CFBundlePackageType</key><string>APPL</string>\n  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>\n  <key>CFBundleShortVersionString</key><string>{v}</string>\n  <key>CFBundleVersion</key><string>{v}</string>\n{icon_key}  <key>LSMinimumSystemVersion</key><string>11.0</string>\n  <key>NSHighResolutionCapable</key><true/>\n</dict>\n</plist>\n",
        t = xml(&game.title),
        e = xml(&exe_name),
        v = xml(&game.version),
    );
    let f = app.join("Contents").join("Info.plist");
    std::fs::write(&f, plist).map_err(|e| io(e, &f))?;
    Ok(new_exe)
}

/// The web: the page's icon and a manifest beside it, and the page's
/// title (the caller's page has `<title>`; `game_page` fills it).
pub fn web(out: &Path, game: &GameInfo, icon: Option<&Icon>) -> Result<(), String> {
    let io = |e: std::io::Error, p: &Path| format!("{}: {e}", p.display());
    let mut icons = Vec::new();
    if let Some(icon) = icon {
        for size in [192u32, 512] {
            let name = format!("icon-{size}.png");
            let f = out.join(&name);
            std::fs::write(&f, icon.png(size)?).map_err(|e| io(e, &f))?;
            icons.push(serde_json::json!({ "src": name, "sizes": format!("{size}x{size}"), "type": "image/png" }));
        }
    }
    let manifest = serde_json::json!({
        "name": game.title,
        "short_name": game.title,
        "start_url": "./",
        "display": "fullscreen",
        "background_color": "#000000",
        "theme_color": "#000000",
        "icons": icons,
    });
    let f = out.join("manifest.webmanifest");
    std::fs::write(
        &f,
        serde_json::to_string_pretty(&manifest).unwrap_or_default(),
    )
    .map_err(|e| io(e, &f))?;
    let page = out.join("index.html");
    let text = std::fs::read_to_string(&page).map_err(|e| io(e, &page))?;
    let mut head = String::from("<link rel=\"manifest\" href=\"manifest.webmanifest\">\n<meta name=\"mobile-web-app-capable\" content=\"yes\">\n<meta name=\"apple-mobile-web-app-capable\" content=\"yes\">\n");
    if icon.is_some() {
        head.push_str("<link rel=\"icon\" href=\"icon-192.png\">\n<link rel=\"apple-touch-icon\" href=\"icon-192.png\">\n");
    }
    let text = text.replacen("<style>", &format!("{head}<style>"), 1);
    std::fs::write(&page, text).map_err(|e| io(e, &page))
}
