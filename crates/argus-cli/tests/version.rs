//! Distribution item 0: one version for the engine, said the same way by
//! `argus version`, `--version`, and `-V`, and carried by what the engine
//! writes (session.status, ARGUS.md, build.json, marks, reports, saves).

use serde_json::Value as Json;
use std::process::Command;

fn argus(args: &[&str]) -> (bool, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_argus"))
        .args(args)
        .output()
        .unwrap();
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
    )
}

#[test]
fn argus_version_says_the_version_the_revision_and_the_platform() {
    let (ok, line) = argus(&["version"]);
    assert!(ok, "{line}");
    let want = format!(
        "argus {} (dev, {} {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    // A test build is from a checkout: its revision is "dev", unless the
    // release script built it.
    if option_env!("ARGUS_REVISION").is_none() {
        assert_eq!(line.trim(), want);
    }
    assert_eq!(argus(&["--version"]).1, line);
    assert_eq!(argus(&["-V"]).1, line);

    let (ok, json) = argus(&["version", "--json"]);
    assert!(ok);
    let v: Json = serde_json::from_str(&json).unwrap();
    assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(v["os"], std::env::consts::OS);
    assert!(v["revision"].is_string(), "{v}");
}

#[test]
fn the_daemon_says_its_version_in_session_status() {
    let st = argus_daemon::daemon::Daemon::new()
        .call(0, "session.status", &serde_json::json!({}))
        .unwrap();
    let engine = &st["engine"];
    assert_eq!(engine["version"], env!("CARGO_PKG_VERSION"), "{st}");
    assert_eq!(engine["revision"], argus_daemon::daemon::REVISION);
}

#[test]
fn a_checkout_does_not_self_update_and_says_how_it_does() {
    for cmd in ["self-update", "uninstall"] {
        let o = Command::new(env!("CARGO_BIN_EXE_argus"))
            .arg(cmd)
            .output()
            .unwrap();
        assert!(!o.status.success(), "{cmd}");
        let e = String::from_utf8_lossy(&o.stderr);
        assert!(
            e.contains("not an installed one") && e.contains("git pull"),
            "{cmd}: {e}"
        );
    }
}
