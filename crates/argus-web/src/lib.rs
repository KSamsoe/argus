//! The Argus web player, phase 2 item 1 (the web spike).
//!
//! A [`Player`] is a project's world assembled the way the daemon assembles
//! it, without the daemon: no socket, no index, no history, no window host.
//! The page steps it, feeds it keys, and draws it into a canvas. It builds
//! natively too, so the same code runs on both sides of the determinism
//! question (§6: a run is reproducible on the same platform and build; is it
//! across the desktop and the browser?).
//!
//! A project reaches the browser as a pack ([`pack`], [`unpack`]): every file
//! of it but `.argus/`, mounted in memory through `argus-os`, so the loaders
//! read it as they read a directory.
//!
//! The assembly here copies the daemon's `open_scene`. A release build
//! (item 7) should share one assembly between the two; this is the spike's
//! shortcut, recorded as such in `docs/web-spike.md`.

#![warn(missing_docs)]

use argus_ecs::{ArgusError, Event, Result, World};
use argus_input::{ActionMap, DeviceInput, Devices, InputState};
use argus_luau::{ScriptConfig, ScriptHost};
use argus_scene::{DefSchema, Project, Scene, SceneDoc, SeedPolicy};
use argus_sim::{Clock, ClockKind, Scheduler, SchedulerConfig};
use serde_json::{json, Value as Json};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
mod wasi;
#[cfg(target_arch = "wasm32")]
pub mod web;

// ----- packs ----------------------------------------------------------------

const MAGIC: &[u8; 8] = b"ARGUSPK1";

/// Every file under `root` but `.argus/` and `tests/`, as a pack: the magic,
/// a count, then each file's `/`-separated relative path and bytes, all
/// lengths little-endian u32. Paths are sorted.
pub fn pack(root: &Path) -> Result<Vec<u8>> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, Vec<u8>)>) -> std::io::Result<()> {
        for e in argus_os::fs::read_dir(dir)? {
            let name = e.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || (dir == root && name == "tests") {
                continue;
            }
            if e.is_dir {
                walk(&e.path, root, out)?;
            } else {
                let rel = e
                    .path
                    .strip_prefix(root)
                    .unwrap_or(&e.path)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, argus_os::fs::read(&e.path)?));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(root, root, &mut files)
        .map_err(|e| ArgusError::new(1, format!("{}: {e}", root.display())))?;
    let mut out = MAGIC.to_vec();
    out.extend((files.len() as u32).to_le_bytes());
    for (p, b) in files {
        out.extend((p.len() as u32).to_le_bytes());
        out.extend(p.as_bytes());
        out.extend((b.len() as u32).to_le_bytes());
        out.extend(b);
    }
    Ok(out)
}

/// A pack's files: relative path and bytes.
pub fn unpack(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    let bad = || {
        ArgusError::new(1, "not an Argus pack")
            .hint("Make one with `cargo run -p argus-web --example pack -- <project> <out.pack>`.")
    };
    if bytes.len() < 12 || &bytes[..8] != MAGIC {
        return Err(bad());
    }
    let u32_at = |i: usize| -> Result<usize> {
        bytes
            .get(i..i + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .ok_or_else(bad)
    };
    let n = u32_at(8)?;
    let mut i = 12;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let pl = u32_at(i)?;
        let path = std::str::from_utf8(bytes.get(i + 4..i + 4 + pl).ok_or_else(bad)?)
            .map_err(|_| bad())?
            .to_string();
        i += 4 + pl;
        let bl = u32_at(i)?;
        let data = bytes.get(i + 4..i + 4 + bl).ok_or_else(bad)?.to_vec();
        i += 4 + bl;
        out.push((path, data));
    }
    Ok(out)
}

// ----- the player -----------------------------------------------------------

/// A project's world, its scheduler, and what feeds and answers it.
pub struct Player {
    /// The world.
    pub world: World,
    /// Its scheduler.
    pub sched: Scheduler,
    /// The project's root, as the loaders see it.
    pub root: PathBuf,
    /// The scene.
    pub scene: String,
    /// The run seed.
    pub seed: u64,
    input: Option<Rc<RefCell<InputState>>>,
    devices: Devices,
    tiles: Rc<RefCell<argus_tiles::MapStore>>,
    /// Particle pools, which the renderer draws.
    pub particles: Rc<RefCell<argus_particles::ParticleStore>>,
    /// The imported assets, which the renderer loads.
    pub assets: argus_assets::Importer,
    /// The mixer, which answers sound requests (and, with a device, plays them).
    pub audio: Option<argus_audio::AudioEngine>,
    _host: ScriptHost,
    /// Events seen, and sounds the mixer answered.
    pub events: u64,
    /// `sound.played` answers so far.
    pub sounds: u64,
    /// A running digest of every event's frame, name, and fields.
    events_digest: u64,
}

fn fnv(h: &mut u64, bytes: &[u8]) {
    for b in bytes {
        *h ^= *b as u64;
        *h = h.wrapping_mul(0x100000001b3);
    }
}

impl Player {
    /// Open a project at `root` on a scene (default: `argus.toml`'s), with a
    /// seed (default: the scene's; a random policy gets 1, since the browser
    /// and the desktop must agree).
    pub fn open(root: &Path, scene: Option<&str>, seed: Option<u64>) -> Result<Player> {
        let root = root.to_path_buf();
        let cfg = argus_scene::ProjectConfig::load(&root)?;
        let scene_name = match scene {
            Some(s) => s.to_string(),
            None => cfg.default_scene.clone().ok_or_else(|| {
                ArgusError::new(1, "no scene given and argus.toml has no default_scene")
            })?,
        };
        let scene_doc = SceneDoc::load(&root, &scene_name)?;
        let clock = Clock::parse(scene_doc.clock.as_deref().unwrap_or(&cfg.clock))?;
        let seed = seed.unwrap_or(match scene_doc.seed {
            SeedPolicy::Fixed(s) => s,
            SeedPolicy::Random => 1,
        });
        let sched_cfg = match clock.kind {
            ClockKind::Continuous => SchedulerConfig::continuous(1.0 / clock.timestep),
            ClockKind::Discrete => SchedulerConfig::discrete(),
        }
        .seed(seed);
        let mut world = World::new();
        argus_ecs::hierarchy::register(&mut world)?;
        let mut sched = Scheduler::new(sched_cfg);
        let input = match ActionMap::load(&root)? {
            Some(map) => {
                let st = Rc::new(RefCell::new(InputState::new(
                    map,
                    clock.kind == ClockKind::Discrete,
                )));
                argus_input::install(&mut world, &mut sched, st.clone())?;
                Some(st)
            }
            None => None,
        };
        argus_render::register(&mut world)?;
        argus_audio::register(&mut world)?;
        let tiles = Rc::new(RefCell::new(argus_tiles::MapStore::new(&root)));
        argus_physics::install(&mut world, &mut sched, tiles.clone())?;
        let zones = Rc::new(RefCell::new(argus_camera::Zones::default()));
        argus_camera::install(&mut world, &mut sched, zones.clone())?;
        argus_post::install(&mut world, &mut sched)?;
        let anims = Rc::new(RefCell::new(argus_anim::AnimStore::default()));
        argus_anim::install(&mut world, &mut sched, anims.clone())?;
        argus_tween::install(&mut world, &mut sched)?;
        let particles = Rc::new(RefCell::new(argus_particles::ParticleStore::default()));
        argus_particles::install(&mut world, &mut sched, particles.clone())?;
        let mut host = ScriptHost::new(ScriptConfig::default())?;
        host.load_project(&mut world, &mut sched, &root)?;
        let defs: Vec<DefSchema> = host
            .defs()
            .iter()
            .map(|d| {
                DefSchema::new(d.schema.clone())
                    .key(d.key.clone())
                    .prefix(d.prefix.clone())
            })
            .collect();
        let project = Project::open(&root, defs)?;
        let mut scene = Scene::open(&project, &scene_name, world.registry())?;
        zones.borrow_mut().rects = scene
            .rooms()
            .filter_map(|r| r.doc.bounds.map(|b| (r.name.clone(), b)))
            .collect();
        if let Some(e) = project.issues().iter().chain(scene.issues()).next() {
            return Err(e.clone());
        }
        if scene.has_rooms() {
            scene.spawn_around(&mut world, None)?;
        } else {
            scene.spawn_all(&mut world)?;
        }
        sched.validate()?;
        let mut assets = argus_assets::Importer::new(&root);
        assets.scan();
        tiles.borrow_mut().set_tilesets(&assets);
        anims.borrow_mut().set_assets(&assets);
        particles.borrow_mut().set_assets(&assets);
        let audio = argus_audio::AudioEngine::load(&root, seed)
            .ok()
            .map(|mut a| {
                a.set_asset_sounds(assets.sidecar_sounds());
                a
            });
        let mut p = Player {
            world,
            sched,
            root,
            scene: scene_name,
            seed,
            input,
            devices: Devices::default(),
            tiles,
            particles,
            assets,
            audio,
            _host: host,
            events: 0,
            sounds: 0,
            events_digest: 0xcbf29ce484222325,
        };
        p.sched.run_load(&mut p.world)?;
        p.pump();
        Ok(p)
    }

    /// Run `n` ticks of the driving clock.
    pub fn step(&mut self, n: u64) -> Result<()> {
        match self.sched.driving().kind {
            ClockKind::Continuous => self.sched.step(&mut self.world, n)?,
            ClockKind::Discrete => self.sched.advance(&mut self.world, n)?,
        }
        self.pump();
        Ok(())
    }

    /// The next tick to run.
    pub fn tick(&self) -> u64 {
        self.sched.tick()
    }

    /// `input.inject`, as the daemon has it.
    pub fn inject(
        &mut self,
        action: &str,
        value: Option<&Json>,
        frames: Option<u64>,
        hold: bool,
    ) -> Result<()> {
        let st = self
            .input
            .clone()
            .ok_or_else(|| ArgusError::new(1, "this project has no input.toml"))?;
        let now = self.sched.tick();
        st.borrow_mut()
            .inject(now, 1, action, value, frames, hold)?;
        Ok(())
    }

    /// A key went down or up, by the name the browser gives it
    /// (`KeyboardEvent.code`: `KeyW`, `ArrowLeft`, `Space`), which is
    /// winit's name too. Returns the turns a discrete game took.
    pub fn key(&mut self, code: &str, pressed: bool) -> u64 {
        self.devices.apply(&DeviceInput::Key {
            name: code.to_string(),
            pressed,
        });
        let Some(st) = self.input.clone() else {
            return 0;
        };
        let now = self.sched.tick();
        let commands = st.borrow_mut().apply_devices(now, &self.devices);
        if commands > 0 && self.sched.driving().kind == ClockKind::Discrete && self.step(1).is_ok()
        {
            return 1;
        }
        0
    }

    /// Losing focus lets go of every key.
    pub fn release_all(&mut self) {
        self.devices.apply(&DeviceInput::ReleaseAll);
        if let Some(st) = self.input.clone() {
            let now = self.sched.tick();
            st.borrow_mut().apply_devices(now, &self.devices);
        }
    }

    /// The input log so far, the way `input.log.get` gives it: what a mark
    /// from the browser carries to reproduce the run on the desktop.
    pub fn input_log(&self) -> Json {
        match &self.input {
            Some(st) => Json::Array(st.borrow().log().iter().map(|e| e.to_json()).collect()),
            None => json!([]),
        }
    }

    /// Schedule a value log (`input_log`'s rows) from the start of the run:
    /// how a mark from the browser is replayed on the desktop.
    pub fn replay_log(&mut self, log: &Json) -> Result<usize> {
        let st = self
            .input
            .clone()
            .ok_or_else(|| ArgusError::new(1, "this project has no input.toml"))?;
        let rows = log.as_array().cloned().unwrap_or_default();
        let mut entries = Vec::new();
        {
            let s = st.borrow();
            for r in rows {
                let line = format!(
                    "{}\t{}\t{}\t{}",
                    r[0],
                    r[1],
                    r[2].as_str().unwrap_or(""),
                    r[3]
                );
                entries.push(argus_input::LogEntry::parse(&line, s.map())?);
            }
        }
        let now = self.sched.tick();
        let n = st.borrow_mut().replay(now, &entries)?;
        Ok(n)
    }

    /// A mark: what reproduces this moment anywhere the run is
    /// deterministic (§10): the run's identity, the frame, the input log,
    /// the digests to check against, and a note.
    pub fn mark(&self, note: &str, code: &str) -> Json {
        json!({
            "scene": self.scene,
            "seed": self.seed,
            "code": code,
            "tick": self.sched.tick(),
            "digest": format!("{:016x}", self.digest()),
            "events_digest": format!("{:016x}", self.events_digest),
            "log": self.input_log(),
            "note": note,
        })
    }

    /// Answer sound requests and count and digest what happened.
    fn pump(&mut self) {
        let drained = self.world.drain_events();
        for rec in &drained {
            self.events += 1;
            let key = format!("{}|{:?}", rec.frame, rec.event);
            fnv(&mut self.events_digest, key.as_bytes());
            if let (Event::Game { name, fields }, Some(a)) = (&rec.event, self.audio.as_mut()) {
                if name == "sound.play" || name == "sound.stop" {
                    a.at_frame(rec.frame);
                    for (n, _) in a.request(&self.world, name, fields) {
                        if n == "sound.played" {
                            self.sounds += 1;
                        }
                    }
                }
            }
        }
        if let Some(a) = self.audio.as_mut() {
            let _ = a.update(&self.world);
            a.advance_to(self.sched.driving().time());
        }
    }

    /// A digest of the whole world: every field of every live entity, in
    /// entity order, floats to the bit. Two runs with the same digest are
    /// the same world.
    pub fn digest(&self) -> u64 {
        self.world.digest()
    }

    /// The events so far, as one digest.
    pub fn events_digest(&self) -> u64 {
        self.events_digest
    }

    /// Where the named entity is, for a quick look.
    pub fn position(&self, id: &str) -> Option<[f32; 2]> {
        let e = self.world.entity(id).ok()?;
        self.world
            .get::<argus_ecs::Transform>(e)
            .ok()
            .map(|t| [t.pos.x, t.pos.y])
    }

    /// What the page shows in its status line.
    pub fn status(&self) -> Json {
        json!({
            "scene": self.scene,
            "seed": self.seed,
            "tick": self.sched.tick(),
            "entities": self.world.entity_count(),
            "events": self.events,
            "sounds": self.sounds,
            "digest": format!("{:016x}", self.digest()),
            "events_digest": format!("{:016x}", self.events_digest),
        })
    }

    /// Tiles in view, for the renderer (the daemon's `tile_extras`).
    pub fn tile_extras(&mut self, view: ([f32; 2], [f32; 2])) -> Vec<argus_render::Extra> {
        let Some(tm) = self.world.component_id_of::<argus_tiles::Tilemap>() else {
            return Vec::new();
        };
        let mut maps = Vec::new();
        for e in self.world.query().has(tm).entities() {
            let Ok(t) = self.world.get::<argus_tiles::Tilemap>(e) else {
                continue;
            };
            if !t.visible {
                continue;
            }
            let origin = self
                .world
                .get::<argus_ecs::GlobalTransform>(e)
                .map(|g| [g.pos.x, g.pos.y])
                .or_else(|_| {
                    self.world
                        .get::<argus_ecs::Transform>(e)
                        .map(|t| [t.pos.x, t.pos.y])
                })
                .unwrap_or([0.0, 0.0]);
            maps.push((self.world.strings().resolve(t.map), origin));
        }
        let mut out = Vec::new();
        let mut store = self.tiles.borrow_mut();
        for (file, origin) in maps {
            let Ok(m) = store.open(&file) else { continue };
            let Ok(tiles) = argus_tiles::visible(m, origin, view.0, view.1) else {
                continue;
            };
            out.extend(tiles.into_iter().map(|t| argus_render::Extra {
                atlas: t.tileset,
                frame: t.frame,
                pos: t.pos,
                size: [t.size, t.size],
                z: t.z,
            }));
        }
        out
    }
}
