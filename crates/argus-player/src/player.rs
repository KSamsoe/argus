//! The player: a project's world without the daemon (phase 2 items 1 and
//! 7). The web page and a release build step it, feed it keys, and draw it;
//! it builds natively too, so the same code runs on both sides of the
//! determinism question (§6: a run is reproducible on the same platform and
//! build; is it across the desktop and the browser?). Its world is
//! [`assemble`](crate::assemble::assemble)d exactly as the daemon's is.

use crate::keyframe::Sources;
use argus_ecs::{ArgusError, Event, Result, World};
use argus_input::{DeviceInput, Devices, InputState};
use argus_luau::ScriptHost;
use argus_sim::{ClockKind, Scheduler};
use serde_json::{json, Value as Json};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

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
    /// The project's name (`argus.toml`), for a window's title.
    pub name: String,
    /// The run seed.
    pub seed: u64,
    input: Option<Rc<RefCell<InputState>>>,
    devices: Devices,
    tiles: Rc<RefCell<argus_tiles::MapStore>>,
    /// Navigation grids and searches (phase 3 item 2).
    pub nav: Rc<RefCell<argus_nav::NavStore>>,
    /// Particle pools, which the renderer draws.
    pub particles: Rc<RefCell<argus_particles::ParticleStore>>,
    /// The imported assets, which the renderer loads.
    pub assets: argus_assets::Importer,
    /// The mixer, which answers sound requests (and, with a device, plays them).
    pub audio: Option<argus_audio::AudioEngine>,
    /// What a keyframe is captured from (the recorder's).
    pub sources: Sources,
    /// The UI's theme and fonts, which the renderer draws with.
    pub ui: Rc<RefCell<argus_ui::Shared>>,
    /// The bounded recorder, once [`Player::record`] started it.
    pub recorder: Option<crate::recorder::Recorder>,
    host: ScriptHost,
    /// Keyframes of the ticks that asked for a save (`q:save`), by name,
    /// until [`Player::take_saves`] (phase 3 item 0).
    saves: Rc<RefCell<Vec<(String, crate::keyframe::Keyframe)>>>,
    /// Saves the game asked to load (`q:load`), until
    /// [`Player::take_loads`].
    loads: Vec<String>,
    /// Events seen, and sounds the mixer answered.
    pub events: u64,
    /// `sound.played` answers so far.
    pub sounds: u64,
    /// A running digest of every event's frame, name, and fields.
    events_digest: u64,
    /// The mods laid over the game, those left out, and what they changed
    /// (phase 3 item 8).
    pub mods: crate::mods::Report,
    /// The mods asked for, which a load opens again.
    mods_asked: crate::mods::Report,
    /// The project's Rust plugins, and what went wrong with any (phase 5
    /// item 6).
    pub plugins: argus_plugins::Plugins,
    /// Where they come from, which a load uses again.
    plugin_source: argus_plugins::Source,
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
        Player::open_with(root, scene, seed, crate::mods::Report::default())
    }

    /// [`open`](Player::open) with mods over the game (phase 3 item 8):
    /// `mods` is what [`crate::mods::choose`] planned, their files already
    /// at `mods/<id>/` under `root`. A mod with a problem is left out, and
    /// [`Player::mods`] says why.
    pub fn open_with(
        root: &Path,
        scene: Option<&str>,
        seed: Option<u64>,
        mods: crate::mods::Report,
    ) -> Result<Player> {
        Player::open_from(root, scene, seed, mods, argus_plugins::Source::default())
    }

    /// [`open_with`](Player::open_with), the Rust plugins from `plugins`: a
    /// release build's own libraries (phase 5 item 6).
    pub fn open_from(
        root: &Path,
        scene: Option<&str>,
        seed: Option<u64>,
        mods: crate::mods::Report,
        plugins: argus_plugins::Source,
    ) -> Result<Player> {
        let root = root.to_path_buf();
        let cfg = argus_scene::ProjectConfig::load(&root)?;
        let scene_name = match scene {
            Some(s) => s.to_string(),
            None => cfg.default_scene.clone().ok_or_else(|| {
                ArgusError::new(1, "no scene given and argus.toml has no default_scene")
            })?,
        };
        let a = crate::assemble::assemble(
            &root,
            &cfg,
            &scene_name,
            seed,
            &crate::assemble::Options {
                mods: mods.clone(),
                plugins: plugins.clone(),
                ..crate::assemble::Options::default()
            },
        )?;
        if let Some(e) = a.issues.first() {
            return Err(e.clone());
        }
        let sources = Sources {
            input: a.input.clone(),
            particles: a.particles.clone(),
            scene: Some(a.scene.clone()),
            contacts: a.contacts.clone(),
            tiles: Some(a.tiles.clone()),
            rigid: a.rigid.clone(),
        };
        let mut p = Player {
            world: a.world,
            sched: a.sched,
            root,
            name: cfg.name.clone(),
            scene: scene_name,
            seed: a.seed,
            input: a.input,
            devices: Devices::default(),
            tiles: a.tiles,
            nav: a.nav,
            particles: a.particles,
            assets: a.assets,
            audio: a.audio.and_then(|r| r.ok()),
            sources,
            ui: a.ui,
            recorder: None,
            host: a.host,
            saves: Rc::new(RefCell::new(Vec::new())),
            loads: Vec::new(),
            events: 0,
            sounds: 0,
            events_digest: 0xcbf29ce484222325,
            mods: a.mods,
            mods_asked: mods,
            plugins: a.plugins,
            plugin_source: plugins,
        };
        let (saves, sources) = (p.saves.clone(), p.sources.clone());
        p.sched.on_tick_end(Box::new(move |world, sched| {
            crate::savegame::requested(world, sched, &sources, &mut saves.borrow_mut());
        }));
        p.sched.run_load(&mut p.world)?;
        p.pump();
        Ok(p)
    }

    /// The saves the game asked for since the last call (`q:save(name)`),
    /// as save files by name, each the world of the tick that asked. The
    /// caller keeps them: a release build where the platform says, the web
    /// page in the browser's storage. `code` is the build's.
    pub fn take_saves(&mut self, code: &str) -> Vec<(String, Vec<u8>)> {
        let pending = std::mem::take(&mut *self.saves.borrow_mut());
        pending
            .into_iter()
            .map(|(name, k)| {
                let mut meta = json!({
                    "game": self.name,
                    "scene": self.scene,
                    "seed": self.seed,
                    "code": code,
                    "clock": self.sched.driving().to_string(),
                    "tick": k.tick,
                    "time": k.time,
                    "name": name,
                    "saved_at": argus_os::time::unix_secs(),
                });
                self.with_mods(&mut meta);
                let bytes = crate::savegame::encode(&meta, &k);
                (name, bytes)
            })
            .collect()
    }

    /// The saves the game asked to load since the last call
    /// (`q:load(name)`); the caller finds each and hands it to
    /// [`Player::load`].
    pub fn take_loads(&mut self) -> Vec<String> {
        std::mem::take(&mut self.loads)
    }

    /// Load a save (§6): the save's scene opened fresh with its seed, then
    /// its world, clocks, random streams, particles, and rooms put back,
    /// components of older versions through their migrations. The mixer
    /// and the recorder carry over. On an error nothing changed. Returns
    /// what was migrated.
    pub fn load(&mut self, bytes: &[u8]) -> Result<Json> {
        let file = crate::savegame::decode(bytes)?;
        let mut next = Player::open_from(
            &self.root,
            Some(file.scene()),
            file.seed(),
            self.mods_asked.clone(),
            self.plugin_source.clone(),
        )?;
        let back = {
            let store = next.particles.borrow();
            file.read_into(&next.world, &store, Some(&mut next.host))?
        };
        next.sources
            .restore(&back.keyframe, &mut next.world, &mut next.sched);
        let _ = next.world.drain_events();
        next.audio = self.audio.take().or(next.audio);
        if self.recorder.is_some() {
            next.record();
        }
        next.events = self.events;
        next.sounds = self.sounds;
        *self = next;
        Ok(crate::savegame::report_json(&back))
    }

    /// Run `n` ticks of the driving clock.
    pub fn step(&mut self, n: u64) -> Result<()> {
        match self.sched.driving().kind {
            ClockKind::Continuous => self.sched.step(&mut self.world, n)?,
            ClockKind::Discrete => self.sched.advance(&mut self.world, n)?,
        }
        self.pump();
        if let Some(r) = self.recorder.as_mut() {
            r.after_ticks(&self.world, &self.sched, &self.sources);
        }
        Ok(())
    }

    /// Keep the bounded recorder from now on (§15): what a report key or a
    /// crash saves. A release build and the web page start it at open.
    pub fn record(&mut self) {
        self.recorder = Some(crate::recorder::Recorder::new(
            &self.world,
            &self.sched,
            &self.sources,
        ));
    }

    /// A report of the last thirty to sixty seconds as a replay file
    /// (`kind` "report" for the report key, "crash" for a crash, which
    /// leaves the world at the end out). None without the recorder.
    pub fn report(&self, note: &str, kind: &str, code: &str) -> Option<Vec<u8>> {
        let r = self.recorder.as_ref()?;
        let mut meta = json!({
            "game": self.name,
            "scene": self.scene,
            "seed": self.seed,
            "code": code,
            "note": note,
            "kind": kind,
            "digest": format!("{:016x}", self.digest()),
        });
        self.with_mods(&mut meta);
        let end = (kind != "crash").then_some(&self.world);
        Some(r.report(meta, &self.sched, &self.sources, end))
    }

    /// Play a report in this player (a fresh one of the same game and
    /// scene): back to its keyframe, its commands replayed to its last
    /// tick, and the world compared with the one it saved. How a check
    /// shows a browser's report reproduces on the desktop.
    pub fn play_report(&mut self, bytes: &[u8]) -> Result<Json> {
        let file = crate::replay::decode(bytes)?;
        let back = file.keyframe(&self.world, &self.particles.borrow())?;
        let k = back.keyframe;
        self.sources.restore(&k, &mut self.world, &mut self.sched);
        if let (Some(i), Some(s)) = (&self.input, &k.input) {
            i.borrow_mut().load_journal(file.journal.clone(), s);
        }
        let n = file.to.saturating_sub(self.sched.tick());
        self.step(n)?;
        let end = file
            .end_world(&self.world)?
            .map(|w| format!("{:016x}", crate::keyframe::run_digest(&w, &file.end_cells)));
        let now = format!("{:016x}", self.digest());
        Ok(json!({
            "from": file.from,
            "to": file.to,
            "tick": self.sched.tick(),
            "digest": now,
            "saved": end,
            "same": end.as_deref() == Some(now.as_str()),
            "meta": file.meta,
        }))
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
        self.devices(&[DeviceInput::Key {
            name: code.to_string(),
            pressed,
        }])
    }

    /// Device input (keys, mouse, pads) through the action map, as the
    /// daemon takes it. Returns the turns a discrete game took: one command,
    /// one turn.
    pub fn devices(&mut self, evs: &[DeviceInput]) -> u64 {
        let st = self.input.clone();
        let next = self.sched.tick();
        let mut caught = false;
        for ev in evs {
            // A controls menu waiting for a key takes the press.
            if st
                .as_ref()
                .is_some_and(|s| s.borrow_mut().capture(next, ev))
            {
                caught = true;
                continue;
            }
            self.devices.apply(ev);
        }
        let Some(st) = st else {
            return 0;
        };
        let now = self.sched.tick();
        let commands = st.borrow_mut().apply_devices(now, &self.devices);
        // A caught key is a command too: the controls page shows it on the
        // next tick, and a discrete game ticks only on a command.
        if (commands > 0 || caught)
            && self.sched.driving().kind == ClockKind::Discrete
            && self.step(1).is_ok()
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

    /// The mods a replay, a save, or a mark needs to reproduce this run
    /// (phase 3 item 8): id, version, and content hash of each, in load
    /// order, when there are any.
    fn with_mods(&self, meta: &mut Json) {
        if !self.mods.loaded.is_empty() {
            meta["mods"] = self.mods.identity();
        }
    }

    /// A mark: what reproduces this moment anywhere the run is
    /// deterministic (§10): the run's identity, the frame, the input log,
    /// the digests to check against, and a note.
    pub fn mark(&self, note: &str, code: &str) -> Json {
        let mut m = json!({
            "scene": self.scene,
            "seed": self.seed,
            "code": code,
            "tick": self.sched.tick(),
            "digest": format!("{:016x}", self.digest()),
            "events_digest": format!("{:016x}", self.events_digest),
            "log": self.input_log(),
            "note": note,
        });
        self.with_mods(&mut m);
        m
    }

    /// Answer sound requests and count and digest what happened.
    fn pump(&mut self) {
        let drained = self.world.drain_events();
        for rec in &drained {
            if let Event::Game { name, fields } = &rec.event {
                if name == "load.requested" {
                    self.loads.push(crate::savegame::request_name(fields));
                }
            }
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
            // The mixer reaches the sim's time first, so a music change is
            // measured from where the music is now (phase 3 item 7).
            a.advance_to(self.sched.driving().time());
            let _ = a.update(&self.world);
        }
    }

    /// A digest of the whole world: every field of every live entity, in
    /// entity order, floats to the bit, and the runtime maps' cells when
    /// any holds a tile (phase 5 item 4). Two runs with the same digest are
    /// the same world.
    pub fn digest(&self) -> u64 {
        crate::keyframe::run_digest(&self.world, &self.tiles.borrow().runtime_cells())
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
            "game": self.name,
            "scene": self.scene,
            "seed": self.seed,
            "tick": self.sched.tick(),
            "entities": self.world.entity_count(),
            "events": self.events,
            "sounds": self.sounds,
            "digest": format!("{:016x}", self.digest()),
            "events_digest": format!("{:016x}", self.events_digest),
            "script_bytes": self.host.memory_used(),
            "strings": self.world.strings().len(),
        })
    }

    /// The solid tiles' edges where they can shadow a view, for the
    /// renderer's 2D lighting (none with lighting off; the daemon's
    /// `tile_occluders`).
    pub fn tile_occluders(&mut self, view: ([f32; 2], [f32; 2])) -> Vec<argus_light::Edge> {
        match argus_light::reach(&self.world, view.0, view.1) {
            Some(rect) => argus_light::tile_edges(&self.world, &mut self.tiles.borrow_mut(), rect),
            None => Vec::new(),
        }
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
