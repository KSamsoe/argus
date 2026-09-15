#![allow(dead_code)]

use argus_ecs::{Component, DefRef, Entity, FieldType, SchemaBuilder, Value, World};
use argus_scene::{DefSchema, Project, Scene};
use argus_sim::{Scheduler, SchedulerConfig, Stage, SystemDesc, SystemInfo};
use std::path::{Path, PathBuf};

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
#[argus(doc = "A door between rooms")]
pub struct Door {
    #[argus(doc = "Where it leads")]
    pub leads_to: Entity,
    #[argus(doc = "Key def that opens it")]
    pub key: DefRef,
    #[argus(doc = "Locked")]
    pub locked: bool,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
#[argus(doc = "A light source")]
pub struct Light {
    #[argus(doc = "Brightness of the lamp", range(0, 10), tune)]
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
#[argus(doc = "Hostile to the player")]
pub struct Enemy;

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
        .join("argus-index-tests")
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

pub fn world() -> World {
    let mut w = World::new();
    argus_ecs::hierarchy::register(&mut w).unwrap();
    w.register::<Door>().unwrap();
    w.register::<Light>().unwrap();
    w.register::<Health>().unwrap();
    w.register::<Enemy>().unwrap();
    w
}

/// Two systems, so `who_writes` and the docs table have content.
pub fn systems(w: &World) -> Vec<SystemInfo> {
    let mut s = Scheduler::new(SchedulerConfig::continuous(60.0));
    let (enemy, health) = (
        w.component_id("Enemy").unwrap(),
        w.component_id("Health").unwrap(),
    );
    s.add_system(
        SystemDesc::new("sim.damage", Stage::Sim)
            .reads(enemy)
            .writes(health)
            .doc("Enemies lose hit points every tick"),
        Box::new(|_| Ok(())),
    )
    .unwrap();
    s.add_system(
        SystemDesc::new("present.hud", Stage::Present)
            .reads(health)
            .doc("Draws the hit point bar"),
        Box::new(|_| Ok(())),
    )
    .unwrap();
    s.systems(w.registry()).unwrap()
}

pub fn def_schemas() -> Vec<DefSchema> {
    let items = SchemaBuilder::new("items")
        .doc("Item definitions")
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
    vec![DefSchema::new(items).prefix("item")]
}

pub fn project(root: &Path) {
    write(
        root,
        "argus.toml",
        "name = \"fixture\"\nclock = \"continuous@60\"\ndefault_scene = \"arena\"\n",
    );
    write(root, "prefabs/door/base.toml", "tags = [\"door\"]\n\n[Transform]\n\n[Door]\nkey = \"\"\n\n[children.light.Light]\nintensity = 1.0\n");
    write(
        root,
        "prefabs/door/locked.toml",
        "prefab = \"door/base\"\n\n[Door]\nlocked = true\nkey = \"item:key.brass\"\n",
    );
    write(
        root,
        "prefabs/enemy/bat.toml",
        "tags = [\"Enemy\"]\n\n[Health]\ncur = 3\nmax = 3\n",
    );
    write(
        root,
        "defs/items.toml",
        "[\"key.brass\"]\nname = \"Brass key\"\nweight = 0.1\n\n[\"bread\"]\nname = \"Bread\"\n",
    );
    write(
        root,
        "scenes/arena/scene.toml",
        "clock = \"continuous@60\"\nseed = 7\n",
    );
    write(
        root,
        "scenes/arena/player.toml",
        "tags = [\"player\"]\n\n[Transform]\npos = [0.0, 0.0]\n\n[Health]\ncur = 10\nmax = 10\n",
    );
    write(
        root,
        "scenes/arena/bat.toml",
        "prefab = \"enemy/bat\"\n\n[Transform]\npos = [3.0, 0.0]\n",
    );
    write(root, "scenes/arena/gate.toml", "prefab = \"door/locked\"\n\n[Transform]\npos = [10.0, 0.0]\n\n[Door]\nleads_to = \"@player\"\n");
    write(
        root,
        "scenes/hollow/scene.toml",
        "seed = 3\n\n[rooms]\nstart = \"a\"\nradius = 0\n\n[rooms.graph]\n\"a\" = [\"b\"]\n",
    );
    write(
        root,
        "scenes/hollow/rooms/a/room.toml",
        "bounds = [0.0, 0.0, 10.0, 10.0]\n",
    );
    write(
        root,
        "scenes/hollow/rooms/a/left.toml",
        "prefab = \"door/base\"\n\n[Door]\nleads_to = \"@b/right\"\n",
    );
    write(root, "scenes/hollow/rooms/b/room.toml", "");
    write(
        root,
        "scenes/hollow/rooms/b/right.toml",
        "prefab = \"door/base\"\n\n[Door]\nleads_to = \"@a/left\"\n",
    );
}

pub fn open(root: &Path) -> Project {
    let p = Project::open(root, def_schemas()).unwrap();
    assert!(p.issues().is_empty(), "{:?}", p.issues());
    p
}

pub fn scene(p: &Project, w: &World, name: &str) -> Scene {
    let s = Scene::open(p, name, w.registry()).unwrap();
    assert!(s.issues().is_empty(), "{:?}", s.issues());
    s
}
