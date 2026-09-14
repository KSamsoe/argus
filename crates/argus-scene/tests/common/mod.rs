#![allow(dead_code)]

use argus_ecs::{Component, DefRef, Entity, FieldType, SchemaBuilder, Value, World};
use argus_scene::{DefSchema, Project};
use std::path::{Path, PathBuf};

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
#[argus(doc = "A door")]
pub struct Door {
    #[argus(doc = "Where it leads")]
    pub leads_to: Entity,
    #[argus(doc = "Key def")]
    pub key: DefRef,
    #[argus(doc = "Locked")]
    pub locked: bool,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
#[argus(doc = "A light")]
pub struct Light {
    #[argus(doc = "Brightness", range(0, 10), tune)]
    pub intensity: f32,
    #[argus(doc = "Switched on")]
    pub on: bool,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
#[argus(doc = "Hit points")]
pub struct Health {
    #[argus(doc = "Current", range(0, 1000))]
    pub cur: i32,
    #[argus(doc = "Maximum")]
    pub max: i32,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Enemy;

/// A temp directory removed on drop.
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
        .join("argus-scene-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Temp(dir)
}

pub fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

pub fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap()
}

pub fn world() -> World {
    let mut w = World::new();
    argus_ecs::hierarchy::register(&mut w).unwrap();
    w.register::<Door>().unwrap();
    w.register::<Light>().unwrap();
    w.register::<Health>().unwrap();
    w.register::<Enemy>().unwrap();
    w
}

pub fn def_schemas() -> Vec<DefSchema> {
    let items = SchemaBuilder::new("items")
        .doc("Item definitions")
        .source("defs/items.luau:1")
        .field("id", FieldType::Str)
        .field("name", FieldType::Str)
        .field_full(
            "weight",
            FieldType::F32,
            Value::Float(1.0),
            Some((0.0, 100.0)),
            "kg",
            true,
        )
        .build()
        .unwrap();
    let recipes = SchemaBuilder::new("recipes")
        .doc("Crafting")
        .source("defs/recipes.luau:1")
        .field("id", FieldType::Str)
        .field("output", FieldType::Ref)
        .field("count", FieldType::U8)
        .build()
        .unwrap();
    vec![
        DefSchema::new(items).prefix("item"),
        DefSchema::new(recipes).prefix("recipe"),
    ]
}

/// The §4 project: prefabs, defs, a room scene (hollow) and a flat one (arena).
pub fn project(root: &Path) {
    write(
        root,
        "argus.toml",
        "name = \"fixture\"\nclock = \"continuous@60\"\ndefault_scene = \"arena\"\nplugins = []\n",
    );
    write(
        root,
        "prefabs/door/base.toml",
        r#"# the plain door
tags = ["door"]

[Transform]
pos = [0.0, 0.0]

[Door]
locked = false
key = ""

[children.light.Light]
intensity = 1.0
"#,
    );
    write(
        root,
        "prefabs/door/locked.toml",
        r#"prefab = "door/base"

[Door]
locked = true
key = "item:key.brass"
"#,
    );
    write(
        root,
        "prefabs/enemy/bat.toml",
        "tags = [\"Enemy\"]\n\n[Health]\ncur = 3\nmax = 3\n",
    );
    write(
        root,
        "defs/items.toml",
        r#"["key.brass"]
name = "Brass key"
weight = 0.1

["bread"]
name = "Bread"
"#,
    );
    write(
        root,
        "defs/recipes.toml",
        "[\"bake\"]\noutput = \"item:bread\"\ncount = 2\n",
    );

    // hollow: rooms
    write(
        root,
        "scenes/hollow/scene.toml",
        r#"clock = "continuous@60"
seed = 7

[rooms]
start = "cavern-03"
radius = 1

[rooms.graph]
"cavern-03" = ["cavern-04"]
"cavern-04" = ["cavern-05"]
"#,
    );
    write(
        root,
        "scenes/hollow/rooms/cavern-03/room.toml",
        "bounds = [0.0, 0.0, 100.0, 100.0]\nneighbors = [\"cavern-04\"]\n",
    );
    write(
        root,
        "scenes/hollow/rooms/cavern-03/door.east.toml",
        r#"prefab = "door/locked"
tags   = ["door"]

[Transform]
pos = [46.0, 8.0]

[Door]
leads_to = "@cavern-04/door.west"   # entity ref by string id, room-qualified
key      = "item:key.brass"         # def ref, not a GUID
locked   = true
"#,
    );
    write(
        root,
        "scenes/hollow/rooms/cavern-03/save-point.toml",
        "tags = [\"save\"]\n\n[Transform]\npos = [1.0, 2.0]\n",
    );
    write(
        root,
        "scenes/hollow/rooms/cavern-04/room.toml",
        "bounds = [100.0, 0.0, 200.0, 100.0]\n",
    );
    write(
        root,
        "scenes/hollow/rooms/cavern-04/door.west.toml",
        r#"prefab = "door/base"

[Door]
leads_to = "@cavern-03/door.east"

[children.light.Light]
intensity = 3.0
"#,
    );
    write(root, "scenes/hollow/rooms/cavern-05/room.toml", "");
    write(
        root,
        "scenes/hollow/rooms/cavern-05/bat.toml",
        "prefab = \"enemy/bat\"\n\n[Health]\ncur = 1\n",
    );

    // arena: flat
    write(
        root,
        "scenes/arena/scene.toml",
        "clock = \"discrete\"\nseed = \"random\"\n\n[systems]\nenable = [\"sim.*\"]\n",
    );
    write(
        root,
        "scenes/arena/player.toml",
        "# the player\ntags = [\"player\"]\n\n[Transform]\npos = [5.0, 5.0]\n\n[Health]\ncur = 10 # hp\nmax = 10\n",
    );
    write(
        root,
        "scenes/arena/spawner.north.toml",
        "[Transform]\npos = [0.0, -10.0]\n\n[Door]\nleads_to = \"@player\"\n",
    );
}

pub fn open(root: &Path) -> Project {
    let p = Project::open(root, def_schemas()).unwrap();
    assert!(p.issues().is_empty(), "project issues: {:?}", p.issues());
    p
}
