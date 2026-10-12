//! A rig for physics tests: a tilemap drawn as text, a world with the
//! physics systems, and bodies driven by writing their intent.
#![allow(dead_code)]

use argus_assets::{encode_png, Importer};
use argus_ecs::{Entity, Event, Transform, Value, Vec2, World};
use argus_physics::{CharacterController, Collider, KinematicBody};
use argus_sim::{Scheduler, SchedulerConfig};
use argus_tiles::{LayerDef, MapFile, MapStore, Rect, Tilemap};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Tiles by character: `#` solid, `-` one-way, `/` and `\` 45-degree
/// slopes, `a` `b` a 22.5-degree rise (0 to 0.5, 0.5 to 1), `c` `d` the
/// matching fall.
const SIDECAR: &str = r#"grid = [4, 2]

[[frames]]
index = 0
properties = { solid = true }

[[frames]]
index = 1
properties = { one_way = true }

[[frames]]
index = 2
properties = { slope = [0, 1] }

[[frames]]
index = 3
properties = { slope = [1, 0] }

[[frames]]
index = 4
properties = { slope = [0, 0.5] }

[[frames]]
index = 5
properties = { slope = [0.5, 1] }

[[frames]]
index = 6
properties = { slope = [1, 0.5] }

[[frames]]
index = 7
properties = { slope = [0.5, 0] }
"#;

fn tile_of(ch: char) -> u32 {
    match ch {
        '#' => 1,
        '-' => 2,
        '/' => 3,
        '\\' => 4,
        'a' => 5,
        'b' => 6,
        'c' => 7,
        'd' => 8,
        _ => 0,
    }
}

/// A game event as the rig keeps it: frame, name, fields.
pub type Ev = (u64, String, Vec<(String, Value)>);

pub const FILE: &str = "scenes/t/room.tilemap.toml";

/// A temp directory removed on drop, used as its path.
pub struct Temp(pub PathBuf);

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

pub struct Rig {
    pub world: World,
    pub sched: Scheduler,
    pub store: Rc<RefCell<MapStore>>,
    pub events: Vec<Ev>,
    pub ts: f32,
    pub rows: usize,
    /// Last: fields drop in order, so whatever the rest of the rig holds
    /// open in it is closed before it is removed.
    pub dir: Temp,
}

impl Rig {
    /// A map from text rows (top row first), `ts` world units per tile,
    /// placed so the bottom row's bottom is at y = 0 and column 0 at x = 0.
    pub fn new(name: &str, ts: f32, art: &[&str]) -> Rig {
        let dir = std::env::temp_dir().join(format!("argus-physics-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let dir = Temp(dir);
        std::fs::create_dir_all(dir.join("assets/tiles")).unwrap();
        encode_png(
            &dir.join("assets/tiles/set.png"),
            16,
            8,
            &[128, 128, 128, 255].repeat(16 * 8),
        )
        .unwrap();
        std::fs::write(dir.join("assets/tiles/set.toml"), SIDECAR).unwrap();
        let mut imp = Importer::new(&dir);
        let r = imp.scan();
        assert!(r.failed.is_empty(), "{:?}", r.failed);
        let rows = art.len();
        let cols = art.iter().map(|r| r.chars().count()).max().unwrap_or(1);
        let mut map = MapFile::create(
            &dir,
            FILE,
            cols as u32,
            rows as u32,
            ts,
            16,
            1,
            vec![LayerDef {
                name: "ground".into(),
                tileset: Some("tiles/set".into()),
                z: 0.0,
                collision: true,
            }],
        )
        .unwrap();
        let cells: Vec<u32> = art
            .iter()
            .flat_map(|r| {
                let mut v: Vec<u32> = r.chars().map(tile_of).collect();
                v.resize(cols, 0);
                v
            })
            .collect();
        map.set(0, Rect::new(0, 0, cols as u32, rows as u32), &cells)
            .unwrap();
        map.save().unwrap();
        let store = Rc::new(RefCell::new(MapStore::new(&dir)));
        store.borrow_mut().insert(FILE, map);
        store.borrow_mut().set_tilesets(&imp);

        let mut world = World::new();
        argus_ecs::hierarchy::register(&mut world).unwrap();
        let mut sched = Scheduler::new(SchedulerConfig::continuous(60.0).seed(1));
        argus_physics::install(&mut world, &mut sched, store.clone()).unwrap();
        let tm = world.spawn();
        world
            .insert(tm, Transform::at(0.0, rows as f32 * ts))
            .unwrap();
        let id = world.strings().intern(FILE);
        world
            .insert(
                tm,
                Tilemap {
                    map: id,
                    visible: true,
                },
            )
            .unwrap();
        world.drain_events();
        Rig {
            dir,
            world,
            sched,
            store,
            events: Vec::new(),
            ts,
            rows,
        }
    }

    /// A player-sized actor (0.8 x 1.6) with a controller, feet at (x, y).
    pub fn player(&mut self, x: f32, y: f32) -> Entity {
        let e = self.world.spawn();
        self.world.insert(e, Transform::at(x, y + 0.8)).unwrap();
        self.world
            .insert(
                e,
                Collider {
                    size: Vec2::new(0.8, 1.6),
                    ..Default::default()
                },
            )
            .unwrap();
        self.world.insert(e, KinematicBody::default()).unwrap();
        self.world
            .insert(e, CharacterController::default())
            .unwrap();
        e
    }

    /// A solid body (a platform), centred at (x, y).
    pub fn solid(&mut self, x: f32, y: f32, w: f32, h: f32, vel: Vec2, one_way: bool) -> Entity {
        let e = self.world.spawn();
        self.world.insert(e, Transform::at(x, y)).unwrap();
        self.world
            .insert(
                e,
                Collider {
                    size: Vec2::new(w, h),
                    ..Default::default()
                },
            )
            .unwrap();
        self.world
            .insert(
                e,
                KinematicBody {
                    solid: true,
                    one_way,
                    velocity: vel,
                    ..Default::default()
                },
            )
            .unwrap();
        e
    }

    pub fn tick(&mut self, n: u64) {
        for _ in 0..n {
            let frame = self.world.frame();
            self.sched.step(&mut self.world, 1).unwrap();
            for r in self.world.drain_events() {
                if let Event::Game { name, fields } = r.event {
                    self.events.push((frame, name, fields));
                }
            }
        }
        self.healthy();
    }

    /// No physics system faulted (a fault disables the system and the
    /// world runs on, which would make a test pass for the wrong reason).
    pub fn healthy(&mut self) {
        for s in self.sched.systems(self.world.registry()).unwrap() {
            assert!(s.enabled, "{} faulted: {:?}", s.name, s.disabled_reason);
        }
    }

    pub fn input(&mut self, e: Entity, x: f32, y: f32, jump: bool) {
        self.world
            .update::<CharacterController, _>(e, |c| {
                c.move_x = x;
                c.move_y = y;
                c.jump = jump;
            })
            .unwrap();
    }

    pub fn body(&self, e: Entity) -> KinematicBody {
        self.world.get::<KinematicBody>(e).unwrap()
    }

    pub fn pos(&self, e: Entity) -> Vec2 {
        self.world.get::<Transform>(e).unwrap().pos
    }

    /// A player's feet.
    pub fn feet(&self, e: Entity) -> f32 {
        self.pos(e).y - 0.8
    }

    /// Events named `name` since the rig started (or since `clear`).
    pub fn named(&self, name: &str) -> Vec<&Ev> {
        self.events.iter().filter(|e| e.1 == name).collect()
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Tick until the body is on the ground (at most `max` ticks).
    pub fn settle(&mut self, e: Entity, max: u64) {
        for _ in 0..max {
            self.tick(1);
            if self.body(e).grounded {
                return;
            }
        }
        panic!("never landed");
    }
}

pub fn field<'a>(ev: &'a Ev, k: &str) -> Option<&'a Value> {
    ev.2.iter().find(|(n, _)| n == k).map(|(_, v)| v)
}

pub fn fnum(ev: &Ev, k: &str) -> f64 {
    match field(ev, k) {
        Some(Value::Float(f)) => *f,
        Some(Value::Int(i)) => *i as f64,
        other => panic!("{k}: {other:?}"),
    }
}

pub fn fbool(ev: &Ev, k: &str) -> bool {
    matches!(field(ev, k), Some(Value::Bool(true)))
}
