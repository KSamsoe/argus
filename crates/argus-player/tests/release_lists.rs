//! Issue 22: list fields in a release build. A release pack carries def
//! rows and entity files in binary; their lists come through as the folder
//! has them, and the game plays the same from its packs.

use argus_ecs::Value;
use argus_player::pack::{self, BuildOptions, Pack};
use argus_player::Player;
use std::path::{Path, PathBuf};

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for Dir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn temp(tag: &str) -> Dir {
    let dir = std::env::temp_dir().join(format!("argus-lists-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Dir(dir)
}

fn put(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn mount(packs: &[Pack]) -> (Dir, PathBuf) {
    let base = temp("data");
    let data = base.join("data");
    std::fs::create_dir_all(&data).unwrap();
    for p in packs {
        std::fs::write(data.join(p.file_name()), p.encode()).unwrap();
    }
    let root = base.join("game");
    pack::mount_dir(&data, &root, None).unwrap();
    (base, root)
}

#[test]
fn lists_in_def_rows_and_entity_files_come_through_a_release_pack() {
    let dir = temp("game");
    put(
        &dir,
        "argus.toml",
        "name = \"lists\"\nclock = \"continuous@60\"\ndefault_scene = \"main\"\nplugins = []\n",
    );
    put(
        &dir,
        "defs/monsters.luau",
        "def \"monsters\" { key = \"id\", prefix = \"monster\", id = field.str {} }\n",
    );
    put(&dir, "defs/monsters.toml", "[rat]\n\n[bat]\n");
    put(
        &dir,
        "defs/rooms.luau",
        "def \"rooms\" { key = \"id\", prefix = \"room\", id = field.str {}, pool = field.list { of = \"ref\" }, heat = field.list { of = \"f32\" } }\n",
    );
    put(
        &dir,
        "defs/rooms.toml",
        "[cellar]\npool = [\"monster:rat\", \"monster:bat\"]\nheat = [0.1, 2.5]\n",
    );
    put(
        &dir,
        "components/tile.luau",
        "component \"Tile\" { on = field.list { of = \"str\" }, pool = field.list { of = \"ref\" }, path = field.list { of = \"vec2\" } }\n",
    );
    put(
        &dir,
        "systems/fill.luau",
        r#"system "sim.fill" {
  stage = "sim", writes = { "Tile" },
  run = function(q)
    for _, t in q:each("Tile") do
      local room = q:def("room:cellar")
      t.pool = room.pool
      local on = table.clone(t.on)
      table.insert(on, tostring(room.heat[2]))
      t.on = on
    end
  end,
}
"#,
    );
    put(&dir, "scenes/main/scene.toml", "seed = 1\n");
    put(
        &dir,
        "scenes/main/tile.toml",
        "[Tile]\non = [\"oiled\"]\npath = [[1.5, 2.0], [3.0, -1.0]]\n",
    );
    let built = pack::build(
        &dir,
        &BuildOptions {
            code: "test".into(),
            since: vec![],
            ..Default::default()
        },
    )
    .unwrap();
    let (_data, root) = mount(&built.packs);
    let mut a = Player::open(&dir, None, None).unwrap();
    let mut b = Player::open(&root, None, None).unwrap();
    assert_eq!(b.digest(), a.digest(), "the same world loaded");
    a.step(3).unwrap();
    b.step(3).unwrap();
    assert_eq!(b.digest(), a.digest(), "and played");
    let e = b.world.entity("@tile").unwrap();
    assert_eq!(
        b.world.get_field(e, "Tile.pool").unwrap(),
        Value::List(vec![
            Value::Ref("monster:rat".into()),
            Value::Ref("monster:bat".into())
        ]),
        "the def row's list from the pack"
    );
    let Value::List(on) = b.world.get_field(e, "Tile.on").unwrap() else {
        panic!()
    };
    assert_eq!(
        on[..2],
        [Value::Str("oiled".into()), Value::Str("2.5".into())]
    );
    assert_eq!(
        b.world.get_field(e, "Tile.path").unwrap(),
        Value::List(vec![
            Value::Vec2(argus_ecs::Vec2::new(1.5, 2.0)),
            Value::Vec2(argus_ecs::Vec2::new(3.0, -1.0)),
        ]),
        "the entity file's list from the pack"
    );
}
