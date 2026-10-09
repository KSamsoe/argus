//! The platform layer (§15, phase 5 item 9): "A platform plugin interface
//! (achievements, cloud saves, rich presence, input glyphs, overlay) with
//! a Steam implementation first and a null implementation for everything
//! else, so game code calls one API and tests run without Steam present."
//!
//! Game code asks, as it asks for a sound: `q:achieve("out_of_the_dark")`,
//! `q:presence("status", "In the caves")`, `q:overlay("achievements")` are
//! events (`achievement.unlock`, `achievement.progress`, `presence.set`,
//! `overlay.open`), and whatever runs the game hands each to its
//! [`Platform`] between ticks ([`answer`]). Nothing the platform says goes
//! back into the world, so a run replays the same with Steam or without
//! it. The overlay being open is an input (the action map's `platform =
//! ["overlay"]`), logged like a key; button prompts follow the device the
//! player last used (argus-input's `InputGlyph`), and a platform with
//! pictures of its buttons supplies them ([`Platform::glyph_image`]).
//! Saves go to the platform's cloud as well as the disk, and a load takes
//! the newer (argus-player's `savegame::newer`).
//!
//! `platform.toml` ([`PlatformConfig`]) names the game's achievements and,
//! for Steam, its app id. [`NullPlatform`] is the platform of the daemon,
//! the web page, the tests, and a release without Steam: it keeps what it
//! is told, and a folder can stand for its cloud, so two copies of a game
//! share saves as two machines on Steam do.

#![warn(missing_docs)]

mod config;
mod null;

pub use argus_input::Family;
pub use config::{AchievementDef, PlatformConfig};
pub use null::NullPlatform;

use argus_ecs::{ArgusError, Result, Value};
use serde_json::{json, Value as Json};

/// Error codes this crate adds (the 63xx block).
pub mod codes {
    /// `platform.toml` is malformed.
    pub const BAD_CONFIG: u32 = 6301;
    /// An achievement `platform.toml` does not declare.
    pub const UNKNOWN_ACHIEVEMENT: u32 = 6302;
    /// A request without what it needs (an id, a key).
    pub const BAD_REQUEST: u32 = 6303;
    /// The platform refused or failed a call.
    pub const PLATFORM: u32 = 6304;
}

/// A file in the platform's cloud.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloudFile {
    /// Its name.
    pub name: String,
    /// Bytes.
    pub size: u64,
}

/// A picture of a button, as RGBA pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlyphImage {
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
    /// Pixels, row by row.
    pub rgba: Vec<u8>,
}

/// What a game asks of the platform it runs on. Achievements are named by
/// the platform's own names (`platform.toml` maps the game's ids to them).
pub trait Platform {
    /// `null`, `steam`.
    fn name(&self) -> &'static str;
    /// The player's name on the platform, if it has one.
    fn user(&self) -> Option<String> {
        None
    }
    /// Unlock an achievement; whether it was locked until now.
    fn unlock(&mut self, api: &str) -> Result<bool>;
    /// Whether an achievement is unlocked.
    fn achieved(&mut self, api: &str) -> Result<bool>;
    /// Lock an achievement again (for a game's tests and a developer, not
    /// for players).
    fn clear_achievement(&mut self, api: &str) -> Result<()>;
    /// Show progress towards an achievement (Steam shows it at its
    /// milestones); reaching `max` does not unlock it.
    fn progress(&mut self, api: &str, current: u32, max: u32) -> Result<()>;
    /// Set a rich presence key, or clear it with `None`.
    fn presence(&mut self, key: &str, value: Option<&str>) -> Result<()>;
    /// Whether it has a cloud for files.
    fn cloud_enabled(&self) -> bool;
    /// Write a file to the cloud.
    fn cloud_write(&mut self, name: &str, bytes: &[u8]) -> Result<()>;
    /// Read a file from the cloud; `None` when it is not there.
    fn cloud_read(&mut self, name: &str) -> Result<Option<Vec<u8>>>;
    /// Delete a file from the cloud (not there is not an error).
    fn cloud_delete(&mut self, name: &str) -> Result<()>;
    /// The cloud's files.
    fn cloud_files(&mut self) -> Vec<CloudFile>;
    /// Open the overlay at a page: `friends`, `achievements`, `community`,
    /// `players`, `settings`, `stats`, or a web page's `https://` address.
    fn open_overlay(&mut self, page: &str) -> Result<()>;
    /// Whether the overlay is open now.
    fn overlay_open(&self) -> bool;
    /// A picture of a button on a family of pad (`south` on `xbox`), when
    /// the platform has one.
    fn glyph_image(&mut self, _family: Family, _input: &str) -> Option<GlyphImage> {
        None
    }
    /// Let the platform do its work (Steam's callbacks); once a frame.
    fn update(&mut self) {}
    /// What it holds, for `platform.status`.
    fn status(&mut self) -> Json;
}

fn field<'a>(fields: &'a [(String, Value)], k: &str) -> Option<&'a Value> {
    fields.iter().find(|(n, _)| n == k).map(|(_, v)| v)
}

fn text(fields: &[(String, Value)], k: &str) -> Option<String> {
    match field(fields, k)? {
        Value::Str(s) | Value::Ref(s) => Some(s.clone()),
        _ => None,
    }
}

fn count(fields: &[(String, Value)], k: &str) -> Option<u32> {
    match field(fields, k)? {
        Value::Int(n) => u32::try_from(*n).ok(),
        Value::UInt(n) => u32::try_from(*n).ok(),
        Value::Float(f) if *f >= 0.0 => Some(f.round() as u32),
        _ => None,
    }
}

/// The events game code asks the platform with.
pub const REQUESTS: &[&str] = &[
    "achievement.unlock",
    "achievement.progress",
    "presence.set",
    "overlay.open",
];

/// What a host reports after handing a request on: an event name and its
/// fields (`platform.achieved {id, api, new}`, `platform.error {request,
/// code, message}`), for its log, never the world's.
pub type Answer = (String, Vec<(String, Value)>);

/// Hand one of the game's events to the platform, if it is a request;
/// what to report back. An achievement `platform.toml` does not declare
/// is refused before the platform hears of it.
pub fn answer(
    p: &mut dyn Platform,
    cfg: &PlatformConfig,
    name: &str,
    fields: &[(String, Value)],
) -> Option<Vec<Answer>> {
    if !REQUESTS.contains(&name) {
        return None;
    }
    let refused = |e: ArgusError| {
        vec![(
            "platform.error".to_string(),
            vec![
                ("request".to_string(), Value::Str(name.to_string())),
                ("code".to_string(), Value::UInt(e.code as u64)),
                ("message".to_string(), Value::Str(e.message)),
            ],
        )]
    };
    let result = (|| -> Result<Vec<Answer>> {
        match name {
            "achievement.unlock" | "achievement.progress" => {
                let id = text(fields, "id").ok_or_else(|| {
                    ArgusError::new(codes::BAD_REQUEST, format!("{name} without an id"))
                })?;
                let def = cfg.achievement(&id)?;
                let api = def.api(&id).to_string();
                if name == "achievement.unlock" {
                    let new = p.unlock(&api)?;
                    Ok(vec![(
                        "platform.achieved".to_string(),
                        vec![
                            ("id".to_string(), Value::Str(id)),
                            ("api".to_string(), Value::Str(api)),
                            ("new".to_string(), Value::Bool(new)),
                        ],
                    )])
                } else {
                    let (current, max) = (count(fields, "current"), count(fields, "max"));
                    let (Some(current), Some(max)) = (current, max) else {
                        return Err(ArgusError::new(
                            codes::BAD_REQUEST,
                            format!("achievement.progress {id} needs a current and a max, whole numbers"),
                        ));
                    };
                    p.progress(&api, current.min(max), max)?;
                    Ok(Vec::new())
                }
            }
            "presence.set" => {
                let key = text(fields, "key")
                    .filter(|k| !k.is_empty())
                    .ok_or_else(|| {
                        ArgusError::new(codes::BAD_REQUEST, "presence.set without a key")
                    })?;
                p.presence(&key, text(fields, "value").as_deref())?;
                Ok(Vec::new())
            }
            _ => {
                let page = text(fields, "page").unwrap_or_else(|| "friends".to_string());
                p.open_overlay(&page)?;
                Ok(Vec::new())
            }
        }
    })();
    Some(result.unwrap_or_else(refused))
}

/// An answer as JSON: `{event, fields}`.
pub fn answer_json((name, fields): &Answer) -> Json {
    let f: serde_json::Map<String, Json> = fields
        .iter()
        .map(|(k, v)| {
            let j = match v {
                Value::Bool(b) => json!(b),
                Value::Int(i) => json!(i),
                Value::UInt(u) => json!(u),
                Value::Float(x) => json!(x),
                Value::Str(s) | Value::Ref(s) => json!(s),
                Value::Vec2(p) => json!([p.x, p.y]),
                Value::Entity(_) => Json::Null,
            };
            (k.clone(), j)
        })
        .collect();
    json!({ "event": name, "fields": f })
}

/// The platform part of a status answer: its name, its user, and what the
/// game's declared achievements are on it.
pub fn status(p: &mut dyn Platform, cfg: &PlatformConfig) -> Json {
    let mut achievements = Vec::new();
    for (id, def) in &cfg.achievements {
        let api = def.api(id).to_string();
        let unlocked = p.achieved(&api).ok();
        achievements.push(json!({
            "id": id,
            "api": api,
            "name": def.name,
            "unlocked": unlocked,
        }));
    }
    json!({
        "platform": p.name(),
        "user": p.user(),
        "steam_app_id": cfg.steam_app_id,
        "achievements": achievements,
        "state": p.status(),
    })
}
