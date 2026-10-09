//! The platform layer (phase 5 item 9): §15 "a platform plugin interface
//! (achievements, cloud saves, rich presence, input glyphs, overlay) ...
//! and a null implementation for everything else, so game code calls one
//! API and tests run without Steam present."

use argus_ecs::Value;
use argus_platform::{answer, codes, NullPlatform, Platform, PlatformConfig};
use std::path::PathBuf;

const TOML: &str = r#"# A game's setup.
[steam]
app_id = 480

[achievements.out_of_the_hollow]
name = "Out of the hollow"
doc = "Reach the way out"
steam = "ACH_WIN_ONE_GAME"

[achievements.collector]
name = "Collector"

[cloud]
saves = false
"#;

fn s(v: &str) -> Value {
    Value::Str(v.to_string())
}

fn req(fields: &[(&str, Value)]) -> Vec<(String, Value)> {
    fields
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

#[test]
fn platform_toml_names_the_achievements_and_steams_app() {
    let c = PlatformConfig::parse("platform.toml", TOML).unwrap();
    assert_eq!(c.steam_app_id, Some(480));
    assert!(!c.cloud_saves);
    let a = c.achievement("out_of_the_hollow").unwrap();
    assert_eq!(a.api("out_of_the_hollow"), "ACH_WIN_ONE_GAME");
    assert!(a.at.starts_with("platform.toml:"), "{}", a.at);
    // Without a steam name, the id is the platform's name for it.
    assert_eq!(
        c.achievement("collector").unwrap().api("collector"),
        "collector"
    );
    let e = c.achievement("nope").unwrap_err();
    assert_eq!(e.code, codes::UNKNOWN_ACHIEVEMENT);
    assert!(
        e.hint.contains("collector, out_of_the_hollow"),
        "{}",
        e.hint
    );
    // No file: the defaults, cloud saves on.
    let d = PlatformConfig::default();
    assert!(d.cloud_saves && d.steam_app_id.is_none() && d.achievements.is_empty());
}

#[test]
fn a_platform_toml_that_is_not_one_is_refused_with_its_line() {
    for (text, says) in [
        ("[steam]\napp_id = 0\n", "app_id"),
        ("[steam]\nappid = 480\n", "steam.appid"),
        ("[achievements.\"no way\"]\n", "letters, digits"),
        ("[achievements.x]\nhidden = \"yes\"\n", "hidden"),
        ("[cloud]\nsaves = 1\n", "cloud.saves"),
        ("[leaderboards]\n", "[leaderboards]"),
    ] {
        let e = PlatformConfig::parse("platform.toml", text).unwrap_err();
        assert_eq!(e.code, codes::BAD_CONFIG, "{text}");
        assert!(e.message.contains(says), "{text}: {}", e.message);
        assert!(
            e.r#ref
                .as_deref()
                .is_some_and(|r| r.starts_with("platform.toml:")),
            "{text}: {:?}",
            e.r#ref
        );
    }
}

#[test]
fn requests_reach_the_platform_and_its_answers_come_back() {
    let c = PlatformConfig::parse("platform.toml", TOML).unwrap();
    let mut p = NullPlatform::new();
    let a = answer(
        &mut p,
        &c,
        "achievement.unlock",
        &req(&[("id", s("out_of_the_hollow"))]),
    )
    .unwrap();
    assert_eq!(a[0].0, "platform.achieved");
    assert!(a[0].1.contains(&("api".to_string(), s("ACH_WIN_ONE_GAME"))));
    assert!(a[0].1.contains(&("new".to_string(), Value::Bool(true))));
    assert!(p.achieved("ACH_WIN_ONE_GAME").unwrap());
    // Twice is not new.
    let a = answer(
        &mut p,
        &c,
        "achievement.unlock",
        &req(&[("id", s("out_of_the_hollow"))]),
    )
    .unwrap();
    assert!(a[0].1.contains(&("new".to_string(), Value::Bool(false))));
    // One platform.toml does not declare is refused, not passed on.
    let a = answer(
        &mut p,
        &c,
        "achievement.unlock",
        &req(&[("id", s("ghost"))]),
    )
    .unwrap();
    assert_eq!(a[0].0, "platform.error");
    assert!(a[0].1.contains(&(
        "code".to_string(),
        Value::UInt(codes::UNKNOWN_ACHIEVEMENT as u64)
    )));
    assert!(!p.achieved("ghost").unwrap());
    // Progress, presence, the overlay.
    answer(
        &mut p,
        &c,
        "achievement.progress",
        &req(&[
            ("id", s("collector")),
            ("current", Value::UInt(3)),
            ("max", Value::UInt(10)),
        ]),
    )
    .unwrap();
    let a = answer(
        &mut p,
        &c,
        "achievement.progress",
        &req(&[("id", s("collector"))]),
    )
    .unwrap();
    assert_eq!(a[0].0, "platform.error", "progress needs current and max");
    answer(
        &mut p,
        &c,
        "presence.set",
        &req(&[("key", s("status")), ("value", s("In the cave"))]),
    )
    .unwrap();
    assert_eq!(p.presence_now()["status"], "In the cave");
    answer(&mut p, &c, "presence.set", &req(&[("key", s("status"))])).unwrap();
    assert!(p.presence_now().is_empty(), "no value clears it");
    answer(
        &mut p,
        &c,
        "overlay.open",
        &req(&[("page", s("achievements"))]),
    )
    .unwrap();
    assert_eq!(p.pages(), ["achievements"]);
    // What is not a request is not the platform's.
    assert!(answer(&mut p, &c, "sound.play", &[]).is_none());
    let st = p.status();
    assert_eq!(st["progress"]["collector"], serde_json::json!([3, 10]));
}

#[test]
fn two_null_platforms_sharing_a_folder_share_a_cloud() {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("platform-cloud");
    let _ = std::fs::remove_dir_all(&d);
    let mut a = NullPlatform::with_cloud(d.clone());
    let mut b = NullPlatform::with_cloud(d.clone());
    a.cloud_write("auto.save", b"one").unwrap();
    assert_eq!(
        b.cloud_read("auto.save").unwrap().as_deref(),
        Some(&b"one"[..])
    );
    assert_eq!(b.cloud_files()[0].name, "auto.save");
    assert_eq!(b.cloud_files()[0].size, 3);
    b.cloud_delete("auto.save").unwrap();
    assert!(a.cloud_read("auto.save").unwrap().is_none());
    b.cloud_delete("auto.save").unwrap();
    // A name with a folder in it is not a cloud file's.
    assert_eq!(
        a.cloud_write("../x", b"").unwrap_err().code,
        codes::BAD_REQUEST
    );
    // In memory, it is one platform's alone.
    let mut m = NullPlatform::new();
    m.cloud_write("x", b"1").unwrap();
    assert_eq!(m.cloud_files().len(), 1);
    assert!(NullPlatform::new().cloud_read("x").unwrap().is_none());
}
