//! The host channel (items 0 and 1): text from the host reaches a script as
//! a `host.message` event, the script's `q:send` reaches the host, and a
//! report with messages in it replays to the same world in a fresh player,
//! with no host to answer it. The game that asked for this (Bring Your Own
//! Game Master) sends a model's ruling in and the scene summary out.

use argus_player::Player;
use std::path::{Path, PathBuf};

/// A temp directory removed on drop, used as its path.
struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for Temp {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

const ECHO: &str = r#"
-- Answers the host: each message goes on the message line, and back out.
return system "sim.echo" {
  stage = "sim",
  unfrozen = true,
  writes = { "UiText" },
  doc = "Writes each host message on the message line and sends it back on the echo channel.",
  run = function(q)
    for _, ev in ipairs(q:events("host.message")) do
      q:entity("@messages"):get("UiText").text = ev.text
      q:send("echo", ev.channel .. ":" .. ev.text)
    end
  end,
}
"#;

/// The roguelike template (a discrete game) with the echo system added.
fn project(name: &str) -> Temp {
    let dir = std::env::temp_dir().join(format!("argus-host-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../templates/roguelike"),
        &dir,
    );
    let _ = std::fs::remove_dir_all(dir.join(".argus"));
    std::fs::write(dir.join("systems/echo.luau"), ECHO).unwrap();
    Temp(dir)
}

fn line(p: &Player) -> String {
    let e = p.world.entity("@messages").unwrap();
    match p.world.get_field(e, "UiText.text").unwrap() {
        argus_ecs::Value::Str(s) => s,
        v => panic!("{v:?}"),
    }
}

#[test]
fn a_message_reaches_a_script_and_its_answer_reaches_the_host() {
    let dir = project("echo");
    let mut p = Player::open(&dir, None, None).unwrap();
    let ruling = "{\"intent\": \"tie the door shut\", \"check\": {\"difficulty\": \"trivial\"}}";
    let turns = p.message("gm.ruling", ruling).unwrap();
    assert_eq!(turns, 1, "on a discrete clock a message is a turn");
    assert_eq!(line(&p), ruling);
    let sent = p.take_sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1, "echo");
    assert_eq!(sent[0].2, format!("gm.ruling:{ruling}"));
    assert!(p.take_sent().is_empty(), "taken once");
    assert!(p.message("bad channel", "x").is_err());
}

/// Hears the host on frozen ticks' terms: not `unfrozen`, so the template's
/// pause keeps it from running. What it heard goes on the message line
/// (after the template's own writer) and back to the host.
const HEARD: &str = r#"
return system "sim.heard" {
  stage = "sim",
  after = { "sim.messages" },
  writes = { "UiText" },
  doc = "Puts each host message on the message line and sends it back on the heard channel.",
  run = function(q)
    for _, ev in ipairs(q:events("host.message")) do
      q:entity("@messages"):get("UiText").text = "heard " .. ev.text
      q:send("heard", ev.text)
    end
  end,
}
"#;

/// Runs through the pause, and says what it saw.
const WATCH: &str = r#"
return system "sim.watch" {
  stage = "sim",
  unfrozen = true,
  doc = "Sends each host message back on the watched channel, paused or not.",
  run = function(q)
    for _, ev in ipairs(q:events("host.message")) do
      q:send("watched", ev.text)
    end
  end,
}
"#;

fn sent(p: &mut Player) -> Vec<(String, String)> {
    p.take_sent().into_iter().map(|s| (s.1, s.2)).collect()
}

fn pair(channel: &str, text: &str) -> (String, String) {
    (channel.to_string(), text.to_string())
}

/// Issue 5: a model's answer arrived while the pause froze the tick, the
/// turn system never saw it, and the game waited for it for ever. A
/// message is held over for the systems a freeze kept from running, until
/// each next runs, and a system that ran sees it once.
#[test]
fn a_message_on_a_frozen_tick_waits_for_the_systems_the_freeze_stopped() {
    let dir = project("held");
    std::fs::remove_file(dir.join("systems/echo.luau")).unwrap();
    std::fs::write(dir.join("systems/heard.luau"), HEARD).unwrap();
    std::fs::write(dir.join("systems/watch.luau"), WATCH).unwrap();
    let mut p = Player::open(&dir, None, None).unwrap();
    let press = |p: &mut Player, key: &str| {
        p.key(key, true);
        p.key(key, false);
    };

    p.message("gm.ruling", "unpaused").unwrap();
    let mut both = sent(&mut p);
    both.sort();
    assert_eq!(
        both,
        [pair("heard", "unpaused"), pair("watched", "unpaused")],
        "no freeze, no holding"
    );
    press(&mut p, "Escape");
    assert!(sent(&mut p).is_empty());
    p.message("gm.ruling", "the door holds").unwrap();
    p.message("gm.ruling", "the goblin flees").unwrap();
    assert_eq!(
        sent(&mut p),
        [
            pair("watched", "the door holds"),
            pair("watched", "the goblin flees")
        ],
        "only the unfrozen system runs while paused"
    );

    // A report from here holds the two in its keyframe.
    p.record();
    press(&mut p, "Escape");
    assert!(
        sent(&mut p).is_empty(),
        "the press that goes on is frozen too, so they still wait"
    );
    press(&mut p, "ArrowRight");
    assert_eq!(
        sent(&mut p),
        [
            pair("heard", "the door holds"),
            pair("heard", "the goblin flees")
        ],
        "the first turn that runs it hears both, oldest first; the watcher saw them once"
    );
    assert_eq!(line(&p), "heard the goblin flees");
    press(&mut p, "ArrowRight");
    assert!(sent(&mut p).is_empty(), "heard once");

    let before = p.digest();
    let bytes = p.report("held over", "report", "test").unwrap();
    let mut fresh = Player::open(&dir, None, None).unwrap();
    let out = fresh.play_report(&bytes).unwrap();
    assert_eq!(out["same"], true, "{out}");
    assert_eq!(fresh.digest(), before);
    assert_eq!(
        line(&fresh),
        "heard the goblin flees",
        "held in the keyframe"
    );
}

#[test]
fn a_report_with_messages_in_it_replays_to_the_same_world_without_the_host() {
    let dir = project("report");
    let mut p = Player::open(&dir, None, None).unwrap();
    p.record();
    for i in 0..12 {
        p.key("ArrowRight", true);
        p.key("ArrowRight", false);
        p.message(
            "gm.ruling",
            &format!(
                "ruling {i}: the goblin is {}",
                ["prone", "burning", "frightened"][i % 3]
            ),
        )
        .unwrap();
    }
    let before = p.digest();
    let said = line(&p);
    assert_eq!(said, "ruling 11: the goblin is frightened");
    let bytes = p
        .report("a ruling I want to see again", "report", "test")
        .unwrap();
    let mut fresh = Player::open(&dir, None, None).unwrap();
    let out = fresh.play_report(&bytes).unwrap();
    assert_eq!(out["same"], true, "{out}");
    assert_eq!(fresh.digest(), before);
    assert_eq!(
        line(&fresh),
        said,
        "the last ruling, replayed from the report, not sent again"
    );
}
