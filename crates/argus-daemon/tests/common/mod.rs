#![allow(dead_code)]

use argus_daemon::Daemon;
use serde_json::{json, Value as Json};
use std::path::{Path, PathBuf};

pub struct Temp(pub PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Temp {
    pub fn path(&self) -> &Path {
        &self.0
    }
}

pub fn temp(name: &str) -> Temp {
    let dir = std::env::temp_dir()
        .join("argus-daemon-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Temp(dir)
}

/// A daemon serving from a thread of its own on a local socket. Dropped,
/// it is told to shut down and waited for (ten seconds at most), so what
/// it holds open in its project, the index above all, is closed before
/// the project's `Temp` goes: Windows does not remove an open file.
pub struct Served {
    socket: String,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Served {
    pub fn new(socket: &str, thread: std::thread::JoinHandle<()>) -> Served {
        Served {
            socket: socket.to_string(),
            thread: Some(thread),
        }
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        use std::io::Write;
        use std::time::{Duration, Instant};
        // No reply is read: a daemon already shut down leaves its listener
        // taking connections that nothing answers.
        if let Ok(mut s) = argus_daemon::server::connect(&self.socket) {
            let call = json!({ "jsonrpc": "2.0", "id": 1, "method": "session.shutdown" });
            let _ = writeln!(s, "{call}");
            let _ = s.flush();
        }
        let Some(thread) = self.thread.take() else {
            return;
        };
        let until = Instant::now() + Duration::from_secs(10);
        while !thread.is_finished() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        if thread.is_finished() {
            let _ = thread.join();
        }
    }
}

pub fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

pub fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap()
}

/// A small game: script components, one script system that hurts enemies
/// every tick and emits `death`, a def table, a prefab, a flat continuous
/// scene, a discrete scene, and a two-room scene.
pub fn project(root: &Path) {
    write(
        root,
        "argus.toml",
        "name = \"fixture\"\nclock = \"continuous@60\"\ndefault_scene = \"arena\"\nplugins = []\n",
    );
    write(
        root,
        "components/health.luau",
        r#"component "Health" {
  doc = "Hit points",
  cur = field.i32 { default = 10, range = {0, 1000}, doc = "Current" },
  max = field.i32 { default = 10, doc = "Maximum" },
}
component "Enemy" { doc = "Hostile" }
component "Door" {
  doc = "A door",
  leads_to = field.entity { doc = "Where it leads" },
  key = field.ref { doc = "Key def" },
  locked = field.bool { default = false },
}
-- Light is the engine's (argus-light, phase 5 item 3): its intensity is
-- the tune field the tests set.
"#,
    );
    write(
        root,
        "defs/items.luau",
        "def \"items\" { key = \"id\", prefix = \"item\", id = field.str {}, name = field.str {}, weight = field.f32 { default = 1.0 } }\n",
    );
    write(
        root,
        "systems/damage.luau",
        r#"system "sim.damage" {
  stage = "sim",
  reads = { "Enemy", "Health" },
  writes = { "Health" },
  doc = "Every enemy loses one hit point per tick; at zero it dies.",
  run = function(q)
    for e, hp in q:each("Health") do
      if e:has("Enemy") and hp.cur > 0 then
        hp.cur -= 1
        if hp.cur == 0 then
          q:emit("death", { entity = e, cause = "attrition" })
        end
      end
    end
  end,
}
"#,
    );
    write(
        root,
        "input.toml",
        r#"[actions.move]
type = "axis2"
doc = "Walk direction"
[actions.move.bindings]
keyboard = ["wasd"]
gamepad = ["left_stick"]

[actions.fire]
type = "button"
doc = "Shoot"
[actions.fire.bindings]
keyboard = ["space"]
gamepad = ["south"]

[slots.1]
devices = ["keyboard", "gamepad:0"]
[slots.2]
devices = ["gamepad:1"]
"#,
    );
    write(
        root,
        "systems/move.luau",
        r#"system "sim.move" {
  stage = "sim",
  reads = { "PlayerSlot", "Actions" },
  writes = { "Transform" },
  doc = "Walks the player by its move action; fire emits a shot.",
  run = function(q)
    for e, t, a in q:each("Transform", "Actions") do
      local m = a.move
      local p = t.pos
      t.pos = { x = p.x + m.x * 2, y = p.y + m.y * 2 }
      if a.fire_pressed then
        q:emit("shot", { by = e })
      end
    end
  end,
}
"#,
    );
    write(
        root,
        "systems/present.luau",
        "system \"present.hud\" { stage = \"present\", reads = { \"Health\" }, run = function(q) end }\n",
    );
    write(
        root,
        "defs/items.toml",
        "[\"key.brass\"]\nname = \"Brass key\"\nweight = 0.1\n\n[\"bread\"]\nname = \"Bread\"\n",
    );
    write(
        root,
        "prefabs/enemy/bat.toml",
        "tags = [\"Enemy\"]\n\n[Health]\ncur = 2\nmax = 2\n",
    );
    write(
        root,
        "scenes/arena/scene.toml",
        "clock = \"continuous@60\"\nseed = 7\n",
    );
    write(
        root,
        "scenes/arena/player.toml",
        "# the player\ntags = [\"player\"]\n\n[PlayerSlot]\nslot = 1\n\n[Actions]\n\n[Transform]\npos = [0.0, 0.0]\n\n[Health]\ncur = 3 # hp\nmax = 3\n",
    );
    write(
        root,
        "scenes/arena/bat.toml",
        "prefab = \"enemy/bat\"\n\n[Transform]\npos = [3.0, 0.0]\n",
    );
    write(
        root,
        "scenes/arena/door.toml",
        "[Transform]\npos = [10.0, 0.0]\n\n[Door]\nleads_to = \"@player\"\nkey = \"item:key.brass\"\n",
    );
    write(
        root,
        "scenes/delve/scene.toml",
        "clock = \"discrete\"\nseed = 1\n",
    );
    write(root, "scenes/delve/hero.toml", "[Health]\ncur = 5\n");
    write(
        root,
        "scenes/hollow/scene.toml",
        "seed = 3\n\n[rooms]\nstart = \"a\"\nradius = 0\n\n[rooms.graph]\n\"a\" = [\"b\"]\n",
    );
    write(root, "scenes/hollow/rooms/a/room.toml", "");
    write(
        root,
        "scenes/hollow/rooms/a/left.toml",
        "[Door]\nleads_to = \"@b/right\"\n",
    );
    write(root, "scenes/hollow/rooms/b/room.toml", "");
    write(
        root,
        "scenes/hollow/rooms/b/right.toml",
        "[Door]\nleads_to = \"@a/left\"\n",
    );
}

/// A daemon with the fixture project open on its default scene.
pub fn open(root: &Path) -> Daemon {
    let mut d = Daemon::new();
    let r = d
        .call(
            0,
            "session.open",
            &json!({ "project": root.to_string_lossy() }),
        )
        .unwrap();
    assert!(r.get("issues").is_none(), "scene issues: {r}");
    assert_eq!(r["script_errors"], 0, "{r}");
    d
}

/// Call and unwrap.
pub fn ok(d: &mut Daemon, method: &str, params: Json) -> Json {
    match d.call(0, method, &params) {
        Ok(v) => v,
        Err(e) => panic!("{method} failed: {e}"),
    }
}

/// `obs.frame`, or `None` on a machine with no graphics adapter at all.
/// Any other error fails the test: a renderer that does not start is a bug,
/// not a missing GPU.
pub fn frame(d: &mut Daemon, params: Json) -> Option<Json> {
    match d.call(0, "obs.frame", &params) {
        Ok(f) => Some(f),
        Err(e) if e.code == argus_daemon::codes::NO_ADAPTER => {
            eprintln!("skipping the frame: {e}");
            None
        }
        Err(e) => panic!("obs.frame: {e}"),
    }
}

/// Call and expect an error.
pub fn err(d: &mut Daemon, method: &str, params: Json) -> argus_ecs::ArgusError {
    match d.call(0, method, &params) {
        Ok(v) => panic!("{method} succeeded: {v}"),
        Err(e) => e,
    }
}

/// Open a transaction, queue ops, commit. Returns the commit row.
pub fn commit(d: &mut Daemon, author: &str, note: &str, ops: &[(&str, Json)]) -> Json {
    let tx = ok(d, "edit.begin", json!({ "author": author, "note": note }))["tx"].clone();
    for (method, mut p) in ops.iter().cloned() {
        p["tx"] = tx.clone();
        ok(d, method, p);
    }
    ok(d, "edit.commit", json!({ "tx": tx }))
}
