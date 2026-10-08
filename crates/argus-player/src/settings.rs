//! A player's settings, kept between launches (phase 5 item 2): the text
//! size, high contrast, fullscreen, and language a game's settings page
//! sets (`UiSettings`), and the keys and hold-or-toggle choices its
//! controls page sets. They live outside the world, in the player's save
//! location (`settings.toml`; the web page keeps them in the browser), and
//! go into the world before the first tick, so the recorder's first
//! keyframe already holds them and a report replays with the player's own
//! settings, not the viewer's. The daemon does not read them: a project's
//! tests run as the project is.
//!
//! Only what differs from the game is kept: a binding or a toggle the
//! player did not change follows the game when an update changes it.

use crate::Player;
use argus_input::ActionMap;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The file in the save location.
pub const FILE: &str = "settings.toml";

/// What a player chose.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    /// The UI's: text scale, high contrast, fullscreen, language (empty:
    /// the game's base). `None` for a game without a settings page.
    pub ui: Option<(f32, bool, bool, String)>,
    /// Bindings that differ from the game's, by action and device.
    pub bindings: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    /// Actions whose hold-or-toggle differs from the game's.
    pub toggles: BTreeMap<String, bool>,
}

impl Settings {
    /// What the player's game has now, against the game's own map.
    pub fn of(p: &Player, game: Option<&ActionMap>) -> Settings {
        let mut s = Settings {
            ui: argus_ui::settings(&p.world).map(|u| {
                (
                    u.text_scale,
                    u.high_contrast,
                    u.fullscreen,
                    p.world.strings().resolve(u.language).to_string(),
                )
            }),
            ..Settings::default()
        };
        let (Some(input), Some(game)) = (&p.sources.input, game) else {
            return s;
        };
        let st = input.borrow();
        for a in &st.map().actions {
            let Ok(was) = game.action(&a.name) else {
                continue;
            };
            for device in ["keyboard", "mouse", "gamepad"] {
                let (now, then) = (a.bindings.get(device), was.bindings.get(device));
                if now != then {
                    s.bindings
                        .entry(a.name.clone())
                        .or_default()
                        .insert(device.to_string(), now.cloned().unwrap_or_default());
                }
            }
            if a.toggle != was.toggle {
                s.toggles.insert(a.name.clone(), a.toggle);
            }
        }
        s
    }

    /// Into the player's world and map. Call it before the first tick and
    /// before [`Player::record`], so the run starts with them. What no
    /// longer fits (an action the game dropped) is left out.
    pub fn apply(&self, p: &mut Player) {
        if let Some((scale, contrast, fullscreen, language)) = &self.ui {
            let lang = p.world.strings().intern(language);
            if let Some(e) = argus_ui::settings_entity(&p.world) {
                let _ = p.world.update::<argus_ui::UiSettings, _>(e, |u| {
                    u.text_scale = scale.clamp(0.5, 4.0);
                    u.high_contrast = *contrast;
                    u.fullscreen = *fullscreen;
                    u.language = lang;
                });
            }
        }
        if let Some(input) = &p.sources.input {
            let mut st = input.borrow_mut();
            let map = st.map_mut();
            for (action, devices) in &self.bindings {
                for (device, names) in devices {
                    let _ = map.set_bindings(action, device, names.clone());
                }
            }
            for (action, on) in &self.toggles {
                let _ = map.set_toggle(action, *on);
            }
        }
    }

    /// As the file holds them.
    pub fn to_toml(&self, game: &str) -> String {
        let mut doc = toml_edit::DocumentMut::new();
        doc.decor_mut().set_prefix(format!(
            "# {game}: your settings. The game writes this file when you change them in it.\n\n"
        ));
        if let Some((scale, contrast, fullscreen, language)) = &self.ui {
            let mut t = toml_edit::Table::new();
            t["text_scale"] = toml_edit::value(*scale as f64);
            t["high_contrast"] = toml_edit::value(*contrast);
            t["fullscreen"] = toml_edit::value(*fullscreen);
            t["language"] = toml_edit::value(language.as_str());
            doc["ui"] = toml_edit::Item::Table(t);
        }
        if !self.bindings.is_empty() {
            let mut t = toml_edit::Table::new();
            for (action, devices) in &self.bindings {
                let mut a = toml_edit::Table::new();
                for (device, names) in devices {
                    let arr: toml_edit::Array = names.iter().map(String::as_str).collect();
                    a[device.as_str()] = toml_edit::value(arr);
                }
                t[action.as_str()] = toml_edit::Item::Table(a);
            }
            t.set_implicit(true);
            doc["bindings"] = toml_edit::Item::Table(t);
        }
        if !self.toggles.is_empty() {
            let mut t = toml_edit::Table::new();
            for (action, on) in &self.toggles {
                t[action.as_str()] = toml_edit::value(*on);
            }
            doc["toggles"] = toml_edit::Item::Table(t);
        }
        doc.to_string()
    }

    /// What [`Settings::to_toml`] wrote; what does not read is left out
    /// (a hand-edited file never stops the game).
    pub fn from_toml(text: &str) -> Settings {
        let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
            return Settings::default();
        };
        let mut s = Settings::default();
        if let Some(t) = doc.get("ui").and_then(|i| i.as_table_like()) {
            let num = |k: &str| {
                t.get(k)
                    .and_then(|i| i.as_float().or_else(|| i.as_integer().map(|n| n as f64)))
            };
            let flag = |k: &str| t.get(k).and_then(|i| i.as_bool()).unwrap_or(false);
            s.ui = Some((
                num("text_scale").unwrap_or(1.0) as f32,
                flag("high_contrast"),
                flag("fullscreen"),
                t.get("language")
                    .and_then(|i| i.as_str())
                    .unwrap_or_default()
                    .to_string(),
            ));
        }
        if let Some(t) = doc.get("bindings").and_then(|i| i.as_table_like()) {
            for (action, devices) in t.iter() {
                let Some(devices) = devices.as_table_like() else {
                    continue;
                };
                for (device, names) in devices.iter() {
                    let names: Vec<String> = names
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect();
                    s.bindings
                        .entry(action.to_string())
                        .or_default()
                        .insert(device.to_string(), names);
                }
            }
        }
        if let Some(t) = doc.get("toggles").and_then(|i| i.as_table_like()) {
            for (action, on) in t.iter() {
                if let Some(on) = on.as_bool() {
                    s.toggles.insert(action.to_string(), on);
                }
            }
        }
        s
    }
}

/// A player's settings kept: read and applied as the game opens, and
/// written back when the player changes them. `path` is the file (`None`
/// on the web page, which keeps the text itself).
pub struct Store {
    path: Option<PathBuf>,
    game: Option<ActionMap>,
    last: Settings,
}

impl Store {
    /// Read the settings (the file in `dir`, the save location, or `text`)
    /// and apply them to a player just opened, before its first tick and
    /// before its recorder starts.
    pub fn open(p: &mut Player, dir: Option<&Path>, text: Option<&str>) -> Store {
        let path = dir.map(|d| d.join(FILE));
        let text = match (text, &path) {
            (Some(t), _) => Some(t.to_string()),
            (None, Some(f)) => std::fs::read_to_string(f).ok(),
            (None, None) => None,
        };
        let game = p.sources.input.as_ref().map(|i| i.borrow().map().clone());
        if let Some(t) = text {
            Settings::from_toml(&t).apply(p);
        }
        let last = Settings::of(p, game.as_ref());
        Store { path, game, last }
    }

    /// Whether the player changed something since the last call: the
    /// settings as the file holds them, written to the file when there is
    /// one (the web page keeps the text).
    pub fn changed(&mut self, p: &Player) -> Option<String> {
        let now = Settings::of(p, self.game.as_ref());
        if now == self.last {
            return None;
        }
        self.last = now;
        let text = self.last.to_toml(&p.name);
        if let Some(f) = &self.path {
            if let Some(d) = f.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(f, &text);
        }
        Some(text)
    }
}
