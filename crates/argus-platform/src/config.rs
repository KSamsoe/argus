//! `platform.toml`: the game's achievements, and Steam's app id.
//!
//! ```toml
//! [steam]
//! app_id = 480                  # Spacewar, Valve's test app, until the game has its own
//!
//! [achievements.out_of_the_dark]
//! name = "Out of the dark"
//! doc = "Reach the cave mouth"
//! steam = "ACH_WIN_ONE_GAME"    # its API name in Steamworks; the id when absent
//! hidden = false
//!
//! [cloud]
//! saves = true                  # saves go to the platform's cloud too (the default)
//! ```

use crate::codes;
use argus_ecs::{ArgusError, Result};
use argus_scene::tomlv;
use serde_json::{json, Value as Json};
use std::collections::BTreeMap;
use std::path::Path;

/// One achievement.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AchievementDef {
    /// What a player reads.
    pub name: String,
    /// How it is earned.
    pub doc: String,
    /// Its API name on Steam, when not the id.
    pub steam: Option<String>,
    /// Hidden until earned (the platform's to honour).
    pub hidden: bool,
    /// `platform.toml:line`.
    pub at: String,
}

impl AchievementDef {
    /// The name the platform knows it by.
    pub fn api<'a>(&'a self, id: &'a str) -> &'a str {
        self.steam.as_deref().unwrap_or(id)
    }
}

/// A game's platform setup.
#[derive(Clone, Debug, PartialEq)]
pub struct PlatformConfig {
    /// Steam's app id, when the game is on Steam.
    pub steam_app_id: Option<u32>,
    /// Achievements by the game's id.
    pub achievements: BTreeMap<String, AchievementDef>,
    /// Saves go to the cloud too.
    pub cloud_saves: bool,
    /// The file, when there is one.
    pub file: Option<String>,
}

impl Default for PlatformConfig {
    fn default() -> Self {
        PlatformConfig {
            steam_app_id: None,
            achievements: BTreeMap::new(),
            cloud_saves: true,
            file: None,
        }
    }
}

impl PlatformConfig {
    /// Read `platform.toml` under `root`; the defaults when there is none.
    pub fn load(root: &Path) -> Result<PlatformConfig> {
        let p = root.join("platform.toml");
        if !argus_os::fs::is_file(&p) {
            return Ok(PlatformConfig::default());
        }
        let text = tomlv::read(&p)?;
        Self::parse("platform.toml", &text)
    }

    /// Parse a setup from text.
    pub fn parse(file: &str, text: &str) -> Result<PlatformConfig> {
        let doc = tomlv::parse(file, text)?;
        let t = doc.as_table();
        let bad = |msg: String, at: String| {
            ArgusError::new(codes::BAD_CONFIG, msg)
                .hint("platform.toml has [steam] app_id, [achievements.<id>] tables with name, doc, steam (its Steamworks API name), and hidden, and [cloud] saves.")
                .at(Some(at))
        };
        let mut cfg = PlatformConfig {
            file: Some(file.to_string()),
            ..Default::default()
        };
        for (k, item) in t.iter() {
            let at = tomlv::item_ref(file, text, item);
            match k {
                "steam" => {
                    let st = tomlv::table_like(item, "steam", &at)?;
                    for (sk, v) in st.iter() {
                        match sk {
                            "app_id" => {
                                let id = v
                                    .as_integer()
                                    .and_then(|n| u32::try_from(n).ok())
                                    .filter(|n| *n > 0)
                                    .ok_or_else(|| {
                                        bad("steam.app_id must be a whole number above 0 (480 is Spacewar, Valve's test app)".into(), at.clone())
                                    })?;
                                cfg.steam_app_id = Some(id);
                            }
                            other => {
                                return Err(bad(format!("steam.{other} is not a setting; steam has app_id"), at))
                            }
                        }
                    }
                }
                "cloud" => {
                    let ct = tomlv::table_like(item, "cloud", &at)?;
                    for (ck, v) in ct.iter() {
                        match (ck, v.as_bool()) {
                            ("saves", Some(b)) => cfg.cloud_saves = b,
                            _ => {
                                return Err(bad(format!("cloud.{ck} is not a setting; cloud has saves = true or false"), at))
                            }
                        }
                    }
                }
                "achievements" => {
                    let at_ = tomlv::table_like(item, "achievements", &at)?;
                    for (id, a) in at_.iter() {
                        let aat = tomlv::item_ref(file, text, a);
                        if id.is_empty()
                            || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                        {
                            return Err(bad(
                                format!("achievement {id:?} must be letters, digits, and underscores"),
                                aat,
                            ));
                        }
                        let it = tomlv::table_like(a, &format!("achievements.{id}"), &aat)?;
                        let mut def = AchievementDef {
                            at: aat.clone(),
                            ..Default::default()
                        };
                        for (f, v) in it.iter() {
                            match (f, v.as_str(), v.as_bool()) {
                                ("name", Some(s), _) => def.name = s.to_string(),
                                ("doc", Some(s), _) => def.doc = s.to_string(),
                                ("steam", Some(s), _) if !s.is_empty() => {
                                    def.steam = Some(s.to_string())
                                }
                                ("hidden", _, Some(b)) => def.hidden = b,
                                _ => {
                                    return Err(bad(
                                        format!("achievements.{id}.{f}: an achievement has name, doc, steam (strings), and hidden (true or false)"),
                                        aat,
                                    ))
                                }
                            }
                        }
                        if def.name.is_empty() {
                            def.name = id.to_string();
                        }
                        cfg.achievements.insert(id.to_string(), def);
                    }
                }
                other => {
                    return Err(bad(
                        format!("[{other}] is not a section; platform.toml has [steam], [achievements], and [cloud]"),
                        at,
                    ))
                }
            }
        }
        Ok(cfg)
    }

    /// An achievement by the game's id.
    pub fn achievement(&self, id: &str) -> Result<&AchievementDef> {
        self.achievements.get(id).ok_or_else(|| {
            let known: Vec<&str> = self.achievements.keys().map(String::as_str).collect();
            ArgusError::new(
                codes::UNKNOWN_ACHIEVEMENT,
                format!("no achievement {id:?} in platform.toml"),
            )
            .hint(if known.is_empty() {
                "Declare it as [achievements.<id>] in platform.toml.".to_string()
            } else {
                format!("Its achievements are {}.", known.join(", "))
            })
        })
    }

    /// For `platform.status`.
    pub fn to_json(&self) -> Json {
        json!({
            "file": self.file,
            "steam_app_id": self.steam_app_id,
            "cloud_saves": self.cloud_saves,
            "achievements": self.achievements.iter().map(|(id, d)| json!({
                "id": id,
                "name": d.name,
                "doc": d.doc,
                "api": d.api(id),
                "hidden": d.hidden,
                "at": d.at,
            })).collect::<Vec<_>>(),
        })
    }
}
