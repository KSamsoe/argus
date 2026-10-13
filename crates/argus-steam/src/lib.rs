//! Steam, as the engine's platform (§15, phase 5 item 9): the
//! [`argus_platform::Platform`] a game's achievements, cloud saves, rich
//! presence, button pictures, and overlay go to when it runs on Steam,
//! through steamworks-rs and Valve's Steamworks SDK.
//!
//! [`SteamPlatform::init`] starts Steam for the game's app (`platform.toml`'s
//! `[steam] app_id`; 480, Spacewar, is Valve's test app every account
//! owns). It fails when Steam is not running or the account does not own
//! the app, and the game then runs on the null platform instead: a player
//! without Steam still plays. Achievements are named by their Steamworks
//! API names; a save in the cloud is a file of Steam Cloud's (Steam
//! Remote Storage); the overlay's opening and closing come from Steam's
//! `GameOverlayActivated` callback, which the host turns into the action
//! map's `overlay` input; a button's picture is the PNG Steam Input has
//! for it.

#![warn(missing_docs)]

use argus_ecs::{ArgusError, Result};
use argus_platform::{codes, CloudFile, Family, GlyphImage, Platform};
use serde_json::{json, Value as Json};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use steamworks::{CallbackHandle, Client, GameOverlayActivated, UserStatsReceived};
use steamworks_sys as sys;

/// What the platform remembers of its calls, for `status`.
const LOG_KEEP: usize = 64;

/// Steam, started for one app.
pub struct SteamPlatform {
    client: Client,
    app: u32,
    overlay: Arc<AtomicBool>,
    _overlay_cb: CallbackHandle,
    /// The account's stats and achievements have arrived (Steam fetches
    /// them after it starts; until then an achievement does not read).
    stats: Arc<AtomicBool>,
    _stats_cb: CallbackHandle,
    input: Option<bool>,
    glyphs: HashMap<(Family, String), Option<GlyphImage>>,
    log: Vec<String>,
}

fn failed(what: String) -> ArgusError {
    ArgusError::new(codes::PLATFORM, what)
}

impl SteamPlatform {
    /// Start Steam for `app_id`. Steam must be running and logged in, and
    /// the account must own the app.
    pub fn init(app_id: u32) -> Result<SteamPlatform> {
        // With no Steam client running, Steam's library prints its own
        // failure on stderr (three `[S_API FAIL]` lines, which a player
        // running the game from a terminal sees) before it fails. Asked
        // first, the game says it once, in its own words.
        // SAFETY: callable before SteamAPI_Init; it only looks for the client.
        if !unsafe { sys::SteamAPI_IsSteamRunning() } {
            return Err(failed(format!("Steam is not running, so app {app_id} plays without it")).hint(
                "Start Steam and log in, then start the game again. The game runs on the null platform without it.",
            ));
        }
        let client = Client::init_app(app_id).map_err(|e| {
            failed(format!("Steam did not start for app {app_id}: {e}")).hint(
                "Steam must be running and logged in, and the account must own the app (480, Spacewar, is every account's). The game runs on the null platform without it.",
            )
        })?;
        let overlay = Arc::new(AtomicBool::new(false));
        let o = overlay.clone();
        let cb = client.register_callback(move |e: GameOverlayActivated| {
            o.store(e.active, Ordering::SeqCst);
        });
        let stats = Arc::new(AtomicBool::new(false));
        let me = client.user().steam_id();
        let st = stats.clone();
        let scb = client.register_callback(move |e: UserStatsReceived| {
            if e.steam_id == me && e.result.is_ok() {
                st.store(true, Ordering::SeqCst);
            }
        });
        client.user_stats().request_user_stats(me.raw());
        Ok(SteamPlatform {
            client,
            app: app_id,
            overlay,
            _overlay_cb: cb,
            stats,
            _stats_cb: scb,
            input: None,
            glyphs: HashMap::new(),
            log: Vec::new(),
        })
    }

    /// The app Steam was started for.
    pub fn app_id(&self) -> u32 {
        self.app
    }

    /// The Steamworks client, for what the platform layer does not cover.
    pub fn client(&self) -> &Client {
        &self.client
    }

    fn note(&mut self, s: String) {
        self.log.push(s);
        if self.log.len() > LOG_KEEP {
            self.log.remove(0);
        }
    }

    /// Wait for the account's stats, the first time they are needed:
    /// Steam's callbacks run until they come, up to three seconds.
    fn stats_ready(&self) -> Result<()> {
        for _ in 0..300 {
            if self.stats.load(Ordering::SeqCst) {
                return Ok(());
            }
            self.client.run_callbacks();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Err(failed(format!(
            "Steam did not send the account's achievements for app {} in three seconds",
            self.app
        ))
        .hint("Steam may be offline; achievements wait until it is back."))
    }

    fn store(&self) -> Result<()> {
        self.client
            .user_stats()
            .store_stats()
            .map_err(|_| failed("Steam did not store the achievements".to_string()))
    }

    /// A file of Steam's install drawn as RGBA.
    fn picture(path: &Path) -> Option<GlyphImage> {
        let (width, height, rgba) = argus_assets::decode_png(path).ok()?;
        Some(GlyphImage {
            width,
            height,
            rgba,
        })
    }
}

impl Drop for SteamPlatform {
    fn drop(&mut self) {
        // Friends see no "In the caves" after the game is gone.
        self.client.friends().clear_rich_presence();
    }
}

/// Steam Input's name for an input of the action map on a family of pad.
fn origin(family: Family, input: &str) -> Option<sys::EInputActionOrigin> {
    use sys::EInputActionOrigin as O;
    Some(match (family, input) {
        (Family::Xbox, "south") => O::k_EInputActionOrigin_XBoxOne_A,
        (Family::Xbox, "east") => O::k_EInputActionOrigin_XBoxOne_B,
        (Family::Xbox, "west") => O::k_EInputActionOrigin_XBoxOne_X,
        (Family::Xbox, "north") => O::k_EInputActionOrigin_XBoxOne_Y,
        (Family::Xbox, "lb") => O::k_EInputActionOrigin_XBoxOne_LeftBumper,
        (Family::Xbox, "rb") => O::k_EInputActionOrigin_XBoxOne_RightBumper,
        (Family::Xbox, "lt") => O::k_EInputActionOrigin_XBoxOne_LeftTrigger_Pull,
        (Family::Xbox, "rt") => O::k_EInputActionOrigin_XBoxOne_RightTrigger_Pull,
        (Family::Xbox, "select") => O::k_EInputActionOrigin_XBoxOne_View,
        (Family::Xbox, "start") => O::k_EInputActionOrigin_XBoxOne_Menu,
        (Family::Xbox, "lthumb") => O::k_EInputActionOrigin_XBoxOne_LeftStick_Click,
        (Family::Xbox, "rthumb") => O::k_EInputActionOrigin_XBoxOne_RightStick_Click,
        (Family::Xbox, "dpad_up") => O::k_EInputActionOrigin_XBoxOne_DPad_North,
        (Family::Xbox, "dpad_down") => O::k_EInputActionOrigin_XBoxOne_DPad_South,
        (Family::Xbox, "dpad_left") => O::k_EInputActionOrigin_XBoxOne_DPad_West,
        (Family::Xbox, "dpad_right") => O::k_EInputActionOrigin_XBoxOne_DPad_East,
        (Family::Xbox, "dpad") => O::k_EInputActionOrigin_XBoxOne_DPad_Move,
        (Family::Xbox, "left_stick" | "left_x" | "left_y") => {
            O::k_EInputActionOrigin_XBoxOne_LeftStick_Move
        }
        (Family::Xbox, "right_stick" | "right_x" | "right_y") => {
            O::k_EInputActionOrigin_XBoxOne_RightStick_Move
        }
        (Family::PlayStation, "south") => O::k_EInputActionOrigin_PS5_X,
        (Family::PlayStation, "east") => O::k_EInputActionOrigin_PS5_Circle,
        (Family::PlayStation, "west") => O::k_EInputActionOrigin_PS5_Square,
        (Family::PlayStation, "north") => O::k_EInputActionOrigin_PS5_Triangle,
        (Family::PlayStation, "lb") => O::k_EInputActionOrigin_PS5_LeftBumper,
        (Family::PlayStation, "rb") => O::k_EInputActionOrigin_PS5_RightBumper,
        (Family::PlayStation, "lt") => O::k_EInputActionOrigin_PS5_LeftTrigger_Pull,
        (Family::PlayStation, "rt") => O::k_EInputActionOrigin_PS5_RightTrigger_Pull,
        (Family::PlayStation, "select") => O::k_EInputActionOrigin_PS5_Create,
        (Family::PlayStation, "start") => O::k_EInputActionOrigin_PS5_Option,
        (Family::PlayStation, "lthumb") => O::k_EInputActionOrigin_PS5_LeftStick_Click,
        (Family::PlayStation, "rthumb") => O::k_EInputActionOrigin_PS5_RightStick_Click,
        (Family::PlayStation, "dpad_up") => O::k_EInputActionOrigin_PS5_DPad_North,
        (Family::PlayStation, "dpad_down") => O::k_EInputActionOrigin_PS5_DPad_South,
        (Family::PlayStation, "dpad_left") => O::k_EInputActionOrigin_PS5_DPad_West,
        (Family::PlayStation, "dpad_right") => O::k_EInputActionOrigin_PS5_DPad_East,
        (Family::PlayStation, "dpad") => O::k_EInputActionOrigin_PS5_DPad_Move,
        (Family::PlayStation, "left_stick" | "left_x" | "left_y") => {
            O::k_EInputActionOrigin_PS5_LeftStick_Move
        }
        (Family::PlayStation, "right_stick" | "right_x" | "right_y") => {
            O::k_EInputActionOrigin_PS5_RightStick_Move
        }
        // A Switch pad's A is east and its B south.
        (Family::Switch, "south") => O::k_EInputActionOrigin_Switch_B,
        (Family::Switch, "east") => O::k_EInputActionOrigin_Switch_A,
        (Family::Switch, "west") => O::k_EInputActionOrigin_Switch_Y,
        (Family::Switch, "north") => O::k_EInputActionOrigin_Switch_X,
        (Family::Switch, "lb") => O::k_EInputActionOrigin_Switch_LeftBumper,
        (Family::Switch, "rb") => O::k_EInputActionOrigin_Switch_RightBumper,
        (Family::Switch, "lt") => O::k_EInputActionOrigin_Switch_LeftTrigger_Pull,
        (Family::Switch, "rt") => O::k_EInputActionOrigin_Switch_RightTrigger_Pull,
        (Family::Switch, "select") => O::k_EInputActionOrigin_Switch_Minus,
        (Family::Switch, "start") => O::k_EInputActionOrigin_Switch_Plus,
        (Family::Switch, "lthumb") => O::k_EInputActionOrigin_Switch_LeftStick_Click,
        (Family::Switch, "rthumb") => O::k_EInputActionOrigin_Switch_RightStick_Click,
        (Family::Switch, "dpad_up") => O::k_EInputActionOrigin_Switch_DPad_North,
        (Family::Switch, "dpad_down") => O::k_EInputActionOrigin_Switch_DPad_South,
        (Family::Switch, "dpad_left") => O::k_EInputActionOrigin_Switch_DPad_West,
        (Family::Switch, "dpad_right") => O::k_EInputActionOrigin_Switch_DPad_East,
        (Family::Switch, "dpad") => O::k_EInputActionOrigin_Switch_DPad_Move,
        (Family::Switch, "left_stick" | "left_x" | "left_y") => {
            O::k_EInputActionOrigin_Switch_LeftStick_Move
        }
        (Family::Switch, "right_stick" | "right_x" | "right_y") => {
            O::k_EInputActionOrigin_Switch_RightStick_Move
        }
        _ => return None,
    })
}

/// The overlay's pages, as the engine names them, to Steam's dialogs.
const PAGES: &[&str] = &[
    "friends",
    "community",
    "players",
    "settings",
    "officialgamegroup",
    "stats",
    "achievements",
];

impl Platform for SteamPlatform {
    fn name(&self) -> &'static str {
        "steam"
    }

    fn user(&self) -> Option<String> {
        Some(self.client.friends().name())
    }

    fn unlock(&mut self, api: &str) -> Result<bool> {
        self.stats_ready()?;
        let stats = self.client.user_stats();
        let a = stats.achievement(api);
        let was = a.get().map_err(|_| {
            failed(format!("Steam has no achievement {api:?} for app {}", self.app))
                .hint("Its API name is the one set in Steamworks; platform.toml maps the game's id to it (steam = \"...\").")
        })?;
        if !was {
            a.set()
                .map_err(|_| failed(format!("Steam did not unlock {api:?}")))?;
            self.store()?;
        }
        self.note(format!(
            "unlock {api}{}",
            if was { " (already)" } else { "" }
        ));
        Ok(!was)
    }

    fn achieved(&mut self, api: &str) -> Result<bool> {
        self.stats_ready()?;
        self.client
            .user_stats()
            .achievement(api)
            .get()
            .map_err(|_| {
                failed(format!(
                    "Steam has no achievement {api:?} for app {}",
                    self.app
                ))
            })
    }

    fn clear_achievement(&mut self, api: &str) -> Result<()> {
        self.stats_ready()?;
        self.client
            .user_stats()
            .achievement(api)
            .clear()
            .map_err(|_| failed(format!("Steam did not clear {api:?}")))?;
        self.store()?;
        self.note(format!("clear {api}"));
        Ok(())
    }

    fn progress(&mut self, api: &str, current: u32, max: u32) -> Result<()> {
        self.stats_ready()?;
        let name = CString::new(api).map_err(|_| failed(format!("{api:?} is not a name")))?;
        // SAFETY: Steam is started (the client lives as long as self) and
        // the name is a C string for the call's length.
        let ok = unsafe {
            sys::SteamAPI_ISteamUserStats_IndicateAchievementProgress(
                sys::SteamAPI_SteamUserStats_v013(),
                name.as_ptr(),
                current,
                max,
            )
        };
        self.note(format!("progress {api} {current}/{max}"));
        if ok {
            Ok(())
        } else {
            Err(failed(format!(
                "Steam showed no progress for {api:?} (an unlocked achievement, or one Steamworks has no progress stat for)"
            )))
        }
    }

    fn presence(&mut self, key: &str, value: Option<&str>) -> Result<()> {
        let ok = self.client.friends().set_rich_presence(key, value);
        self.note(format!("presence {key} = {value:?}"));
        if ok {
            Ok(())
        } else {
            Err(failed(format!(
                "Steam refused rich presence {key:?} (a key or value too long, or too many keys)"
            )))
        }
    }

    fn cloud_enabled(&self) -> bool {
        let rs = self.client.remote_storage();
        rs.is_cloud_enabled_for_account() && rs.is_cloud_enabled_for_app()
    }

    fn cloud_write(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        let mut w = self.client.remote_storage().file(name).write();
        w.write_all(bytes)
            .and_then(|_| w.flush())
            .map_err(|e| failed(format!("Steam Cloud did not take {name}: {e}")))?;
        drop(w);
        self.note(format!("cloud write {name} ({} bytes)", bytes.len()));
        Ok(())
    }

    fn cloud_read(&mut self, name: &str) -> Result<Option<Vec<u8>>> {
        let f = self.client.remote_storage().file(name);
        if !f.exists() {
            return Ok(None);
        }
        let mut out = Vec::new();
        f.read()
            .read_to_end(&mut out)
            .map_err(|e| failed(format!("Steam Cloud's {name}: {e}")))?;
        Ok(Some(out))
    }

    fn cloud_delete(&mut self, name: &str) -> Result<()> {
        let f = self.client.remote_storage().file(name);
        if f.exists() && !f.delete() {
            return Err(failed(format!("Steam Cloud did not delete {name}")));
        }
        self.note(format!("cloud delete {name}"));
        Ok(())
    }

    fn cloud_files(&mut self) -> Vec<CloudFile> {
        let mut v: Vec<CloudFile> = self
            .client
            .remote_storage()
            .files()
            .into_iter()
            .map(|f| CloudFile {
                name: f.name,
                size: f.size,
            })
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    fn open_overlay(&mut self, page: &str) -> Result<()> {
        if page.starts_with("https://") {
            self.client
                .friends()
                .activate_game_overlay_to_web_page(page);
        } else if PAGES.contains(&page) {
            self.client.friends().activate_game_overlay(page);
        } else {
            return Err(ArgusError::new(
                codes::BAD_REQUEST,
                format!("{page:?} is not an overlay page"),
            )
            .hint(format!(
                "Pages are {}, or an https:// address.",
                PAGES.join(", ")
            )));
        }
        self.note(format!("overlay {page}"));
        Ok(())
    }

    fn overlay_open(&self) -> bool {
        self.overlay.load(Ordering::SeqCst)
    }

    fn glyph_image(&mut self, family: Family, input: &str) -> Option<GlyphImage> {
        let key = (family, input.to_string());
        if let Some(g) = self.glyphs.get(&key) {
            return g.clone();
        }
        let ready = *self
            .input
            .get_or_insert_with(|| self.client.input().init(false));
        let image = if ready {
            origin(family, input).and_then(|o| {
                // SAFETY: Steam Input is started; the path Steam returns
                // is a C string it owns until the next call.
                let path = unsafe {
                    let p = sys::SteamAPI_ISteamInput_GetGlyphPNGForActionOrigin(
                        sys::SteamAPI_SteamInput_v006(),
                        o,
                        sys::ESteamInputGlyphSize::k_ESteamInputGlyphSize_Medium,
                        0,
                    );
                    if p.is_null() {
                        return None;
                    }
                    CStr::from_ptr(p).to_string_lossy().into_owned()
                };
                if path.is_empty() {
                    None
                } else {
                    SteamPlatform::picture(Path::new(&path))
                }
            })
        } else {
            None
        };
        self.glyphs.insert(key, image.clone());
        image
    }

    fn update(&mut self) {
        self.client.run_callbacks();
    }

    fn status(&mut self) -> Json {
        let rs = self.client.remote_storage();
        // SAFETY: Steam is started.
        let overlay_enabled =
            unsafe { sys::SteamAPI_ISteamUtils_IsOverlayEnabled(sys::SteamAPI_SteamUtils_v010()) };
        let files = self.cloud_files();
        json!({
            "app_id": self.app,
            "user": self.client.friends().name(),
            "overlay": { "open": self.overlay_open(), "enabled": overlay_enabled },
            "cloud": {
                "account": rs.is_cloud_enabled_for_account(),
                "app": rs.is_cloud_enabled_for_app(),
                "files": files.iter().map(|f| json!({ "name": f.name, "size": f.size })).collect::<Vec<_>>(),
            },
            "steam_input": self.input,
            "log": self.log,
        })
    }
}

/// Where Steam's library is for the target: the file a release ships
/// beside its executable (`steam_api64.dll`, `libsteam_api.so`,
/// `libsteam_api.dylib`).
pub fn redistributable() -> &'static str {
    if cfg!(windows) {
        "steam_api64.dll"
    } else if cfg!(target_os = "macos") {
        "libsteam_api.dylib"
    } else {
        "libsteam_api.so"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pad_input_has_a_steam_origin_but_a_generic_pads() {
        for f in [Family::Xbox, Family::PlayStation, Family::Switch] {
            // The guide button is Steam's own: it has no picture to show.
            for input in argus_input::devices::PAD_NAMES
                .iter()
                .filter(|i| **i != "mode")
            {
                assert!(origin(f, input).is_some(), "{f:?} {input}");
            }
        }
        assert!(origin(Family::Gamepad, "south").is_none());
        assert!(origin(Family::Keyboard, "space").is_none());
    }
}
