//! Steam itself (phase 5 item 9): the platform layer against Spacewar
//! (app 480, Valve's test app) on a logged-in Steam. Run by hand, with
//! the account owner's say-so, never by CI: it is ignored, and it does
//! nothing without `ARGUS_STEAM_LIVE=1` (CI's perf step runs ignored
//! tests). It leaves the account as it found it: an achievement that was
//! unlocked stays unlocked, one it unlocks it clears again, and the rich
//! presence and the cloud file it makes are gone after.
//!
//! ```text
//! ARGUS_STEAM_LIVE=1 cargo test -p argus-steam --test steam_live -- --ignored --nocapture
//! ```

use argus_platform::{Family, Platform};
use argus_steam::SteamPlatform;
use serde_json::json;

/// Spacewar's first achievement.
const ACHIEVEMENT: &str = "ACH_WIN_ONE_GAME";
const FILE: &str = "argus-live-test.txt";

#[test]
#[ignore]
fn spacewar_takes_achievements_presence_cloud_files_and_gives_glyphs() {
    if std::env::var("ARGUS_STEAM_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: set ARGUS_STEAM_LIVE=1 to run against Steam");
        return;
    }
    let mut s = SteamPlatform::init(480).expect("Steam starts for Spacewar");
    s.update();
    let user = s.user().unwrap_or_default();
    assert!(!user.is_empty(), "a logged-in user");

    // Achievements, left as found.
    let was = s.achieved(ACHIEVEMENT).expect("Spacewar has it");
    let new = s.unlock(ACHIEVEMENT).unwrap();
    assert_eq!(new, !was, "unlocking says whether it was locked");
    assert!(s.achieved(ACHIEVEMENT).unwrap());
    if !was {
        s.clear_achievement(ACHIEVEMENT).unwrap();
        assert!(!s.achieved(ACHIEVEMENT).unwrap(), "cleared again");
    }
    let unknown = s.unlock("ARGUS_NO_SUCH_ACHIEVEMENT").unwrap_err();
    assert!(
        unknown.message.contains("no achievement"),
        "{}",
        unknown.message
    );

    // Rich presence, set and read back, then cleared.
    s.presence("status", Some("Testing the Argus engine"))
        .unwrap();
    s.update();
    let me = s.client().user().steam_id();
    let back = s.client().friends().get_friend(me).rich_presence("status");
    s.presence("status", None).unwrap();
    s.update();
    let after = s.client().friends().get_friend(me).rich_presence("status");

    // The cloud: a file written, read, listed, and deleted.
    let cloud = s.cloud_enabled();
    let mut cloud_round_trip = None;
    if cloud {
        s.cloud_write(FILE, b"argus").unwrap();
        let read = s.cloud_read(FILE).unwrap();
        let listed = s.cloud_files().iter().any(|f| f.name == FILE);
        s.cloud_delete(FILE).unwrap();
        let gone = s.cloud_read(FILE).unwrap().is_none();
        assert_eq!(read.as_deref(), Some(&b"argus"[..]));
        assert!(listed && gone);
        cloud_round_trip = Some(true);
    }

    // A button's picture from Steam Input.
    let glyph = s.glyph_image(Family::Xbox, "south");
    let ps = s.glyph_image(Family::PlayStation, "south");

    let status = s.status();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "user": user,
            "achievement": { "name": ACHIEVEMENT, "was_unlocked": was, "unlock_was_new": new },
            "presence": { "read_back": back, "after_clear": after },
            "cloud": { "enabled": cloud, "round_trip": cloud_round_trip },
            "glyphs": {
                "xbox_south": glyph.as_ref().map(|g| [g.width, g.height]),
                "playstation_south": ps.as_ref().map(|g| [g.width, g.height]),
            },
            "status": status,
        }))
        .unwrap()
    );
    assert_eq!(back.as_deref(), Some("Testing the Argus engine"));
    assert_eq!(after, None, "presence cleared");
    assert!(glyph.is_some_and(|g| g.width > 0 && g.rgba.len() == (g.width * g.height * 4) as usize));
}

/// Spacewar's achievements by API name, read only: what a test game maps
/// its own to.
#[test]
#[ignore]
fn spacewar_names_its_achievements() {
    if std::env::var("ARGUS_STEAM_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: set ARGUS_STEAM_LIVE=1 to run against Steam");
        return;
    }
    let mut s = SteamPlatform::init(480).expect("Steam starts for Spacewar");
    s.update();
    // The stats come after Steam starts; asking for one waits for them.
    let _ = s.achieved(ACHIEVEMENT);
    let names = s
        .client()
        .user_stats()
        .get_achievement_names()
        .unwrap_or_default();
    println!("{}", json!({ "achievements": names }));
    assert!(names.iter().any(|n| n == ACHIEVEMENT));
}
