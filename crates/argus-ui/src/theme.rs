//! The UI theme (`ui.toml`, §12): the canvas the UI is laid out on, the
//! default font and text size, and colour tokens by name, with a second set
//! for high contrast. "Text scaling and high-contrast palettes are UI theme
//! tokens": a player's settings (a [`crate::UiSettings`] in the world) pick
//! the scale and the palette at run time.
//!
//! ```toml
//! width = 960            # the canvas, in UI pixels; the window scales it
//! height = 540
//! font = ""              # a font asset; empty: the built-in pixel font
//! text_size = 16
//!
//! [colors]
//! text = "#E8E4DC"
//! panel = "#15181CE6"
//!
//! [high_contrast_colors] # the same names, used when high contrast is on
//! text = "#FFFFFF"
//! panel = "#000000"
//! ```

use argus_ecs::{ArgusError, Result};
use std::collections::BTreeMap;
use std::path::Path;

/// Colours every theme has, and what they are when a project says nothing.
pub const DEFAULT_COLORS: &[(&str, &str, &str)] = &[
    // name, normal, high contrast
    ("text", "#E8E4DC", "#FFFFFF"),
    ("muted", "#9AA1A8", "#FFFFFF"),
    ("panel", "#15181CE6", "#000000"),
    ("border", "#3A4048", "#FFFFFF"),
    ("focus", "#FFD166", "#FFFF00"),
    ("accent", "#E8703F", "#FFFFFF"),
    ("disabled", "#6A7078", "#BFBFBF"),
];

/// A colour, 0 to 1 per channel, sRGB.
pub type Rgba = [f32; 4];

/// The theme.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// Canvas width in UI pixels.
    pub width: f32,
    /// Canvas height in UI pixels.
    pub height: f32,
    /// The default font asset; empty: the built-in pixel font.
    pub font: String,
    /// The default text size in UI pixels.
    pub text_size: f32,
    /// Text scale when no `UiSettings` says.
    pub text_scale: f32,
    /// High contrast when no `UiSettings` says.
    pub high_contrast: bool,
    colors: BTreeMap<String, Rgba>,
    contrast: BTreeMap<String, Rgba>,
}

impl Default for Theme {
    fn default() -> Self {
        let mut colors = BTreeMap::new();
        let mut contrast = BTreeMap::new();
        for (n, a, b) in DEFAULT_COLORS {
            colors.insert(n.to_string(), parse_color(a).expect("a default colour"));
            contrast.insert(n.to_string(), parse_color(b).expect("a default colour"));
        }
        Theme {
            width: 960.0,
            height: 540.0,
            font: String::new(),
            text_size: 16.0,
            text_scale: 1.0,
            high_contrast: false,
            colors,
            contrast,
        }
    }
}

/// `#RGB`, `#RRGGBB`, or `#RRGGBBAA`.
pub fn parse_color(s: &str) -> Option<Rgba> {
    let h = s.trim().strip_prefix('#')?;
    let digits: Vec<u8> = h
        .chars()
        .map(|c| c.to_digit(16).map(|d| d as u8))
        .collect::<Option<_>>()?;
    let pair = |i: usize| (digits[i] * 16 + digits[i + 1]) as f32 / 255.0;
    Some(match digits.len() {
        3 => [
            digits[0] as f32 / 15.0,
            digits[1] as f32 / 15.0,
            digits[2] as f32 / 15.0,
            1.0,
        ],
        6 => [pair(0), pair(2), pair(4), 1.0],
        8 => [pair(0), pair(2), pair(4), pair(6)],
        _ => return None,
    })
}

impl Theme {
    /// `ui.toml` under `root`, or the defaults when there is none.
    pub fn load(root: &Path) -> Result<Theme> {
        let path = root.join("ui.toml");
        match argus_os::fs::read_to_string(&path) {
            Ok(text) => Theme::parse(&text),
            Err(_) => Ok(Theme::default()),
        }
    }

    /// A theme from the text of a `ui.toml`.
    pub fn parse(text: &str) -> Result<Theme> {
        let doc: toml_edit::ImDocument<String> =
            text.parse().map_err(|e: toml_edit::TomlError| {
                bad(format!("ui.toml does not parse: {}", e.message()))
            })?;
        let mut t = Theme::default();
        let num = |k: &str| -> Result<Option<f32>> {
            match doc.get(k) {
                None => Ok(None),
                Some(i) => i
                    .as_float()
                    .or_else(|| i.as_integer().map(|n| n as f64))
                    .map(|f| Some(f as f32))
                    .ok_or_else(|| bad(format!("ui.toml {k} must be a number"))),
            }
        };
        if let Some(w) = num("width")? {
            t.width = w;
        }
        if let Some(h) = num("height")? {
            t.height = h;
        }
        if let Some(s) = num("text_size")? {
            t.text_size = s;
        }
        if let Some(s) = num("text_scale")? {
            t.text_scale = s;
        }
        if t.width < 1.0 || t.height < 1.0 || t.text_size <= 0.0 || t.text_scale <= 0.0 {
            return Err(bad(
                "ui.toml width, height, text_size, and text_scale must be positive",
            ));
        }
        if let Some(f) = doc.get("font") {
            t.font = f
                .as_str()
                .ok_or_else(|| bad("ui.toml font must be a font asset's name"))?
                .to_string();
        }
        if let Some(h) = doc.get("high_contrast") {
            t.high_contrast = h
                .as_bool()
                .ok_or_else(|| bad("ui.toml high_contrast must be true or false"))?;
        }
        for (table, into) in [
            ("colors", &mut t.colors),
            ("high_contrast_colors", &mut t.contrast),
        ] {
            let Some(tab) = doc.get(table).and_then(|i| i.as_table_like()) else {
                continue;
            };
            for (k, v) in tab.iter() {
                let c = v.as_str().and_then(parse_color).ok_or_else(|| {
                    bad(format!(
                        "ui.toml [{table}] {k} must be a colour like \"#E8E4DC\""
                    ))
                })?;
                into.insert(k.to_string(), c);
            }
        }
        // A name given only a normal colour is the same in high contrast.
        for (k, c) in t.colors.clone() {
            t.contrast.entry(k).or_insert(c);
        }
        Ok(t)
    }

    /// A colour by token name or `#hex`, from the high-contrast set when
    /// `contrast`; `None` for a name the theme does not have.
    pub fn color(&self, name: &str, contrast: bool) -> Option<Rgba> {
        if name.starts_with('#') {
            return parse_color(name);
        }
        let set = if contrast {
            &self.contrast
        } else {
            &self.colors
        };
        set.get(name).copied()
    }

    /// Every token name.
    pub fn color_names(&self) -> Vec<&str> {
        self.colors.keys().map(String::as_str).collect()
    }
}

fn bad(msg: impl Into<String>) -> ArgusError {
    ArgusError::new(argus_ecs::codes::BAD_SCHEMA, msg)
        .hint("ui.toml has width, height, font, text_size, text_scale, high_contrast, [colors], and [high_contrast_colors].")
        .at(Some("ui.toml:1".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_parse_and_a_theme_overrides_the_defaults() {
        assert_eq!(parse_color("#FFF"), Some([1.0, 1.0, 1.0, 1.0]));
        assert_eq!(parse_color("#FF000080").unwrap()[3], 128.0 / 255.0);
        assert_eq!(parse_color("red"), None);
        let t = Theme::parse(
            "width = 640\nheight = 360\n[colors]\ntext = \"#000000\"\nwood = \"#8B5A2B\"\n[high_contrast_colors]\ntext = \"#FFFFFF\"\n",
        )
        .unwrap();
        assert_eq!((t.width, t.height), (640.0, 360.0));
        assert_eq!(t.color("text", false), Some([0.0, 0.0, 0.0, 1.0]));
        assert_eq!(t.color("text", true), Some([1.0, 1.0, 1.0, 1.0]));
        assert_eq!(t.color("wood", true), t.color("wood", false));
        assert!(t.color("panel", false).is_some(), "the defaults stay");
        assert_eq!(t.color("#00FF00", false), Some([0.0, 1.0, 0.0, 1.0]));
        assert!(Theme::parse("width = 0").is_err());
    }
}
