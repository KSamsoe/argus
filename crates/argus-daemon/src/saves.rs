//! Save games in the daemon (§6, phase 3 item 0): `sim.save` writes the
//! world as a save file, and `sim.load` reads one back through the
//! migrations the project's components declare, reporting each component
//! brought up as an event ("Health migrated v2→v3 for 1,204 entities").
//!
//! A game asks for its own saves: `q:save(name)` in a script emits
//! `save.requested`, and the tick-end hook keeps a keyframe of that tick,
//! written once the step is over; `q:load(name)` emits `load.requested`,
//! which is done after the step that asked. The daemon keeps saves in
//! `.argus/saves/<name>.save`, a development game's player-facing place; a
//! release build keeps them where the platform says (`argus_player::save`),
//! the web page in the browser's storage.

use super::*;
use argus_player::savegame;
use std::time::Instant;

impl Daemon {
    /// Where `name` (or an explicit `path`, absolute or project-relative)
    /// is kept.
    fn save_path(&self, params: &Json) -> Result<(String, PathBuf)> {
        let root = self.require_session()?.root.clone();
        let name = json::opt_str(params, "name")?.unwrap_or("save").to_string();
        let path = match json::opt_str(params, "path")? {
            Some(p) if Path::new(p).is_absolute() => PathBuf::from(p),
            Some(p) => root.join(p),
            None => root
                .join(".argus")
                .join("saves")
                .join(savegame::file_name(&name)),
        };
        Ok((name, path))
    }

    /// `sim.save {name?, path?}`: the world now as a save file.
    pub(super) fn sim_save(&mut self, params: &Json) -> Result<Json> {
        let (name, path) = self.save_path(params)?;
        let k = {
            let s = self.require_session()?;
            s.sources.capture(&self.world, &self.sched, true)
        };
        self.write_save(&name, &path, &k)
    }

    /// Write `k` as the save `name` at `path`, and say so in the log.
    fn write_save(&mut self, name: &str, path: &Path, k: &crate::ring::Keyframe) -> Result<Json> {
        let t = Instant::now();
        let (root, scene, seed) = {
            let s = self.require_session()?;
            (s.root.clone(), s.scene_name.clone(), s.seed)
        };
        let cfg = ProjectConfig::load(&root)?;
        let meta = json!({
            "game": cfg.name,
            "scene": scene,
            "seed": seed,
            "code": code_version(&root),
            "clock": self.sched.driving().to_string(),
            "tick": k.tick,
            "time": k.time,
            "name": name,
            "saved_at": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        });
        let bytes = savegame::encode(&meta, k);
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| save_io(path, e))?;
        }
        std::fs::write(path, &bytes).map_err(|e| save_io(path, e))?;
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let shown = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        self.world.emit(Event::Game {
            name: "game.saved".to_string(),
            fields: vec![
                ("name".into(), Value::Str(name.to_string())),
                ("path".into(), Value::Str(shown.clone())),
                ("tick".into(), Value::UInt(k.tick)),
                ("bytes".into(), Value::UInt(bytes.len() as u64)),
            ],
        });
        self.pump_events();
        Ok(json!({
            "name": name,
            "path": shown,
            "tick": k.tick,
            "bytes": bytes.len(),
            "ms": (ms * 100.0).round() / 100.0,
        }))
    }

    /// `sim.load {name? | path?}`: the save's scene loaded fresh with its
    /// seed, then its world, clocks, random streams, particles, and rooms
    /// put back, components of older versions migrated. The feed, undo, and
    /// locks stay: they are the session's, not the world's. A load that
    /// fails leaves the game that was running as it was.
    pub(super) fn sim_load(&mut self, params: &Json) -> Result<Json> {
        let t = Instant::now();
        let (name, path) = self.save_path(params)?;
        let bytes = std::fs::read(&path).map_err(|e| {
            ArgusError::new(
                codes::BAD_SAVE,
                format!("no save {name:?} at {}: {e}", path.display()),
            )
            .hint("sim.save {name} writes one; saves are kept in .argus/saves/.")
        })?;
        let file = savegame::decode(&bytes)?;
        let root = self.require_session()?.root.clone();
        // A game loaded while playing plays on (a fresh scene's clock starts
        // stopped: the proposal trial's frozen variant, phase 2's gate).
        let play = (
            self.sched.is_playing(),
            self.sched.is_paused(),
            self.sched.speed(),
        );
        let edits = std::mem::take(&mut self.edits);
        let mut before = Slot::empty();
        self.swap_slot(&mut before);
        // Sequence numbers keep counting, as on any load, so a client's
        // `since` cursor (a test's, a subscriber's) sees what comes after.
        self.events.next_seq = before.events.next_seq;
        let r = self.load_save(&root, &name, &file, t);
        self.edits = edits;
        match &r {
            Err(_) => self.swap_slot(&mut before),
            Ok(_) if play.0 => {
                if let Some(ses) = &mut self.session {
                    ses.play_from = Some(self.sched.tick());
                }
                self.sched.play(&self.world);
                if play.1 {
                    self.sched.pause();
                }
                if self.sched.driving().kind == ClockKind::Continuous {
                    self.sched.set_speed(play.2)?;
                }
            }
            Ok(_) => {}
        }
        r
    }

    /// The rest of `sim.load`, into a fresh session.
    fn load_save(
        &mut self,
        root: &Path,
        name: &str,
        file: &savegame::SaveFile,
        t: Instant,
    ) -> Result<Json> {
        let name = name.to_string();
        self.load_for(root, file.scene(), file.seed())
            .map_err(|e| {
                ArgusError::new(
                    codes::BAD_SAVE,
                    format!(
                        "the save's scene {:?} does not load: {}",
                        file.scene(),
                        e.message
                    ),
                )
            })?;
        let (sources, ring, particles) = {
            let s = self.session.as_ref().expect("session");
            (s.sources.clone(), s.ring.clone(), s.particles.clone())
        };
        let back = {
            let ses = self.session.as_mut().expect("session");
            file.read_into(&self.world, &particles.borrow(), Some(&mut ses.host))?
        };
        let k = back.keyframe.clone();
        self.put_back(&sources, &k);
        self.events.through = k.tick;
        {
            let mut r = ring.borrow_mut();
            r.restart(k.clone());
            r.mutations = self.mutations;
        }
        self.after_open();
        let report = savegame::report_json(&back);
        for m in &back.world.migrated {
            self.world.emit(Event::Game {
                name: "component.migrated".to_string(),
                fields: vec![
                    ("component".into(), Value::Str(m.component.clone())),
                    ("from".into(), Value::UInt(m.from as u64)),
                    ("to".into(), Value::UInt(m.to as u64)),
                    ("entities".into(), Value::UInt(m.rows as u64)),
                    ("declared".into(), Value::Bool(m.declared)),
                ],
            });
        }
        self.world.emit(Event::Game {
            name: "game.loaded".to_string(),
            fields: vec![
                ("name".into(), Value::Str(name.clone())),
                ("tick".into(), Value::UInt(k.tick)),
                (
                    "migrated".into(),
                    Value::UInt(back.world.migrated.len() as u64),
                ),
            ],
        });
        self.pump_events();
        let mut out = json!({
            "name": name,
            "scene": file.scene(),
            "tick": k.tick,
            "saved": file.meta.clone(),
            "ms": ((t.elapsed().as_secs_f64() * 1000.0) * 100.0).round() / 100.0,
        });
        if let (Json::Object(o), Json::Object(r)) = (&mut out, report) {
            o.extend(r);
        }
        Ok(out)
    }

    /// After a step: write the saves its ticks asked for (`q:save`), then
    /// do a load one asked for (`q:load`). A failure is an event, not the
    /// step's error: the game asked, and the log says what came of it.
    pub(super) fn answer_save_requests(&mut self) {
        let pending: Vec<(String, crate::ring::Keyframe)> = match &self.session {
            Some(s) => std::mem::take(&mut *s.saves.borrow_mut()),
            None => return,
        };
        let load = self.load_request.take();
        if self.backstage {
            return;
        }
        for (name, k) in pending {
            let r = self
                .save_path(&json!({ "name": name }))
                .and_then(|(name, path)| self.write_save(&name, &path, &k));
            if let Err(e) = r {
                self.request_failed("game.save_failed", &name, &e);
            }
        }
        if let Some(name) = load {
            if let Err(e) = self.sim_load(&json!({ "name": name })) {
                self.request_failed("game.load_failed", &name, &e);
            }
        }
    }

    fn request_failed(&mut self, event: &str, name: &str, e: &ArgusError) {
        self.world.emit(Event::Game {
            name: event.to_string(),
            fields: vec![
                ("name".into(), Value::Str(name.to_string())),
                ("message".into(), Value::Str(e.message.clone())),
            ],
        });
        self.pump_events();
    }
}

fn save_io(path: &Path, e: impl std::fmt::Display) -> ArgusError {
    ArgusError::new(codes::BAD_SAVE, format!("{}: {e}", path.display()))
}
