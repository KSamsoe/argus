//! A release's platform (phase 5 item 9): the game's requests reach the
//! platform between ticks, its overlay is an input, and a save goes to the
//! cloud too, so another machine sharing the cloud loads it (§15 "cloud
//! saves"). The null platform stands for Steam, its cloud a folder.

use argus_platform::{CloudFile, NullPlatform, Platform};
use argus_player::Player;
use serde_json::Value as Json;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// A game that saves at tick 5, earns an achievement at tick 8, and pauses
/// on the overlay.
fn game(tag: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("platform-{tag}"));
    let _ = std::fs::remove_dir_all(&root);
    write(
        &root,
        "argus.toml",
        "name = \"plat\"\nclock = \"continuous@60\"\ndefault_scene = \"s\"\nplugins = []\nmodules = [\"steam\"]\n",
    );
    write(
        &root,
        "platform.toml",
        "[steam]\napp_id = 480\n\n[achievements.early]\nname = \"Early\"\nsteam = \"ACH_WIN_ONE_GAME\"\n",
    );
    write(
        &root,
        "input.toml",
        "[actions.pause]\ntype = \"button\"\n[actions.pause.bindings]\nkeyboard = [\"escape\"]\nplatform = [\"overlay\"]\n",
    );
    write(
        &root,
        "scenes/s/scene.toml",
        "clock = \"continuous@60\"\nseed = 1\n",
    );
    write(
        &root,
        "scenes/s/player.toml",
        "[PlayerSlot]\nslot = 1\n\n[Actions]\n\n[Transform]\n",
    );
    write(
        &root,
        "systems/plan.luau",
        r#"return system "sim.plan" {
  stage = "sim",
  reads = { "Actions" },
  writes = { "Transform" },
  run = function(q)
    for _, a, t in q:each("Actions", "Transform") do
      t.pos = vector.create(q.tick, if a.pause then 1 else 0, 0)
    end
    if q.tick == 5 then q:save("auto") end
    if q.tick == 8 then q:achieve("early") end
  end,
}
"#,
    );
    root
}

/// `bytes` with one run of text changed for another of its length.
fn replace(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    assert_eq!(from.len(), to.len());
    let i = bytes
        .windows(from.len())
        .position(|w| w == from.as_bytes())
        .expect("found");
    let mut out = bytes.to_vec();
    out[i..i + to.len()].copy_from_slice(to.as_bytes());
    out
}

/// A platform the test can still reach once the player owns it.
struct Shared(Rc<RefCell<NullPlatform>>);

impl Platform for Shared {
    fn name(&self) -> &'static str {
        "null"
    }
    fn unlock(&mut self, api: &str) -> argus_ecs::Result<bool> {
        self.0.borrow_mut().unlock(api)
    }
    fn achieved(&mut self, api: &str) -> argus_ecs::Result<bool> {
        self.0.borrow_mut().achieved(api)
    }
    fn clear_achievement(&mut self, api: &str) -> argus_ecs::Result<()> {
        self.0.borrow_mut().clear_achievement(api)
    }
    fn progress(&mut self, api: &str, c: u32, m: u32) -> argus_ecs::Result<()> {
        self.0.borrow_mut().progress(api, c, m)
    }
    fn presence(&mut self, k: &str, v: Option<&str>) -> argus_ecs::Result<()> {
        self.0.borrow_mut().presence(k, v)
    }
    fn cloud_enabled(&self) -> bool {
        true
    }
    fn cloud_write(&mut self, n: &str, b: &[u8]) -> argus_ecs::Result<()> {
        self.0.borrow_mut().cloud_write(n, b)
    }
    fn cloud_read(&mut self, n: &str) -> argus_ecs::Result<Option<Vec<u8>>> {
        self.0.borrow_mut().cloud_read(n)
    }
    fn cloud_delete(&mut self, n: &str) -> argus_ecs::Result<()> {
        self.0.borrow_mut().cloud_delete(n)
    }
    fn cloud_files(&mut self) -> Vec<CloudFile> {
        self.0.borrow_mut().cloud_files()
    }
    fn open_overlay(&mut self, p: &str) -> argus_ecs::Result<()> {
        self.0.borrow_mut().open_overlay(p)
    }
    fn overlay_open(&self) -> bool {
        self.0.borrow().overlay_open()
    }
    fn status(&mut self) -> Json {
        self.0.borrow_mut().status()
    }
}

#[test]
fn a_save_in_the_cloud_loads_on_another_machine() {
    let root = game("cloud");
    let cloud = root.join("cloud");
    let mut a = Player::open(&root, None, None).unwrap();
    a.set_platform(Box::new(NullPlatform::with_cloud(cloud.clone())));
    a.step(6).unwrap();
    let saves = a.take_saves("test");
    assert_eq!(saves.len(), 1);
    let disk_a = root.join("machine-a/auto.save");
    let went = a
        .store_save(Some(&disk_a), &saves[0].0, &saves[0].1)
        .unwrap();
    assert_eq!(went, ["disk", "cloud"]);
    // Another machine: nothing on its disk, the same cloud.
    let mut b = Player::open(&root, None, None).unwrap();
    b.set_platform(Box::new(NullPlatform::with_cloud(cloud.clone())));
    let disk_b = root.join("machine-b/auto.save");
    let (bytes, from) = b.fetch_save(Some(&disk_b), "auto").unwrap();
    assert_eq!(from, "cloud");
    b.load(&bytes).unwrap();
    // The world at the end of the tick that asked: tick 5 ran, 6 is next.
    assert_eq!(b.tick(), 6);
    // The same second on both: the disk's.
    assert_eq!(a.fetch_save(Some(&disk_a), "auto").unwrap().1, "disk");
    // Saved a minute later elsewhere: a takes the cloud's, the newer.
    let at = argus_player::savegame::saved_at(&bytes).unwrap();
    let later = replace(&bytes, &at.to_string(), &(at + 60).to_string());
    assert_eq!(argus_player::savegame::saved_at(&later), Some(at + 60));
    let mut np = NullPlatform::with_cloud(cloud.clone());
    np.cloud_write("auto.save", &later).unwrap();
    assert_eq!(a.fetch_save(Some(&disk_a), "auto").unwrap().1, "cloud");
    // No disk and no cloud copy: there is no save.
    let mut c = Player::open(&root, None, None).unwrap();
    c.set_platform(Box::new(NullPlatform::new()));
    assert!(c
        .fetch_save(Some(&root.join("machine-c/auto.save")), "nope")
        .is_err());
}

#[test]
fn requests_reach_the_platform_and_the_overlay_is_an_input() {
    let root = game("requests");
    let shared = Rc::new(RefCell::new(NullPlatform::new()));
    let mut p = Player::open(&root, None, None).unwrap();
    p.set_platform(Box::new(Shared(shared.clone())));
    p.step(10).unwrap();
    assert!(shared.borrow_mut().achieved("ACH_WIN_ONE_GAME").unwrap());
    let st = p.platform_status();
    assert_eq!(st["achievements"][0]["id"], "early");
    assert_eq!(st["achievements"][0]["unlocked"], true);
    assert_eq!(st["answers"][0]["event"], "platform.achieved");
    assert_eq!(p.status()["platform"], "null");
    // The overlay opens: the pause action is held, from the next tick.
    shared.borrow_mut().set_overlay(true);
    p.poll_platform();
    p.step(2).unwrap();
    let y = |p: &Player| {
        let e = p.world.entity("player").unwrap();
        p.world.get::<argus_ecs::Transform>(e).unwrap().pos.y
    };
    assert_eq!(y(&p), 1.0, "paused while the overlay is open");
    shared.borrow_mut().set_overlay(false);
    p.poll_platform();
    p.step(2).unwrap();
    assert_eq!(y(&p), 0.0);
    // And it was logged as an input, so a replay holds it too.
    let log = p.input_log().to_string();
    assert!(log.contains("pause"), "{log}");
}
