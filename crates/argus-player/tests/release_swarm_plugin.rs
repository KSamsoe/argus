//! Phase 5 item 6: Swarm's chase is Rust now (`plugins/swarm-hot`), moved
//! out of `systems/chase.luau` with its declaration unchanged (§19: "The
//! scheduler's write-set declarations make it possible to promote a Luau
//! system to Rust without changing anything around it"). The game with the
//! Rust chase runs its systems in the same order and reaches the same
//! world, tick for tick, as the game with the Luau chase it replaced.

use argus_player::Player;
use std::path::{Path, PathBuf};

/// `systems/chase.luau` as it was, word for word, before it moved.
const CHASE_LUAU: &str = r#"return system "sim.chase" {
  stage = "sim",
  after = { "sim.waves" },
  reads = { "Enemy", "Transform", "Player" },
  writes = { "Transform" },
  doc = "Every enemy walks straight at the player. Positions are native vectors: no allocation per entity.",
  run = function(q)
    local pe = q:entity("@player")
    if pe:get("Player").dead then return end
    local p = pe:get("Transform").pos
    local dt = q.dt
    for e, en, t in q:each("Enemy", "Transform") do
      local d = p - t.pos
      local len = vector.magnitude(d)
      if len > 0.05 then
        t.pos = t.pos + d * (en.speed * dt / len)
      end
    end
  end,
}
"#;

/// A temp directory removed on drop.
struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn swarm() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/swarm")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let name = e.file_name();
        if name == ".argus" || name == "dist" || name == "target" {
            continue;
        }
        let (a, b) = (e.path(), to.join(&name));
        if a.is_dir() {
            copy_dir(&a, &b);
        } else {
            std::fs::copy(&a, &b).unwrap();
        }
    }
}

/// Swarm as it was: the Luau chase, no plugins.
fn luau_swarm(tag: &str) -> Temp {
    let root = std::env::temp_dir().join(format!("argus-swarm-luau-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    copy_dir(&swarm(), &root);
    std::fs::remove_dir_all(root.join("plugins")).unwrap();
    std::fs::write(root.join("systems/chase.luau"), CHASE_LUAU).unwrap();
    let toml = std::fs::read_to_string(root.join("argus.toml")).unwrap();
    let toml = toml.replace("plugins = [\"swarm-hot\"]", "plugins = []");
    std::fs::write(root.join("argus.toml"), toml).unwrap();
    Temp(root)
}

fn order(p: &mut Player) -> Vec<String> {
    let reg = p.world.registry().clone();
    p.sched
        .systems(&reg)
        .unwrap()
        .into_iter()
        .map(|s| s.name)
        .collect()
}

fn same_run(scene: &str, ticks: u64) {
    let old = luau_swarm(scene);
    let mut luau = Player::open(&old.0, Some(scene), Some(7)).unwrap();
    let mut rust = Player::open(&swarm(), Some(scene), Some(7)).unwrap();
    assert!(rust.plugins.errors.is_empty(), "{:?}", rust.plugins.errors);
    assert_eq!(rust.plugins.of_system("sim.chase"), Some("swarm-hot"));
    assert_eq!(luau.plugins.of_system("sim.chase"), None);
    assert_eq!(
        order(&mut luau),
        order(&mut rust),
        "the same systems in the same order"
    );
    for t in 0..ticks {
        luau.step(1).unwrap();
        rust.step(1).unwrap();
        assert_eq!(
            luau.digest(),
            rust.digest(),
            "{scene}: the worlds part at tick {t}"
        );
    }
}

#[test]
fn the_rust_chase_is_the_luau_chase_to_the_bit() {
    same_run("arena", 600);
}

#[test]
fn the_stress_scenes_fifty_thousand_chase_alike() {
    same_run("stress", 40);
}
