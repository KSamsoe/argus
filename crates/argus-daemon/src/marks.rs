//! Marks (§10, phase 2 item 2). While playing, the human presses F8, or
//! anything calls `mark.drop`, and the moment becomes a bug report the
//! agent can reproduce without asking what they were doing.
//!
//! A mark is a directory under `.argus/marks/`, numbered in the order they
//! were dropped:
//!
//! - `mark.json`: the frame, the note, the run's identity (scene, seed,
//!   code, clock), the world's digest, the last five seconds of input, the
//!   events of those seconds, and whether it is still open (the queue).
//! - `replay.bin`: a replay file (`crate::replay`): the keyframe the five
//!   seconds start from, the input commands since, and the world at the
//!   mark. It reproduces the mark after the daemon that took it is gone,
//!   which is the usual case: closing the game's window ends the daemon.
//! - `frame.png`: the frame, drawn at the window's size.
//! - `clip.png`: the two seconds before it, an animated PNG replayed from
//!   the ring and drawn offscreen, so a mark needs no GPU frame history
//!   and works headless.
//! - `clip.wav`: the last two seconds of what the mixer played.
//!
//! On a discrete clock "seconds" are turns: the lead-in is ten turns and
//! the clip the last eight, one picture a turn.
//!
//! `mark.open` reproduces one: it loads the mark's scene with its seed,
//! reads the keyframe back, and plays the lead-in forward so the event log
//! and `obs.trace` hold the seconds before the mark, then compares the
//! world with the one the mark saved, field by field. After a fix the
//! difference is the fix's effect at that frame.
//!
//! A mark from the web build (`mark.import`) has no keyframe: it is the
//! run's identity and its input log, and opens by replaying from the load.

use super::*;
use crate::replay;
use argus_input::{ActionValue, DeviceInput, JournalEntry, LogEntry};
use std::time::{Duration, Instant};

const LEAD_SECONDS: f64 = 5.0;
const CLIP_SECONDS: f64 = 2.0;
const CLIP_FPS: f64 = 30.0;
const DISCRETE_LEAD: u64 = 10;
const DISCRETE_CLIP: u64 = 8;
const DISCRETE_FPS: f64 = 4.0;
/// The clip's height; its width keeps the window's shape.
const CLIP_HEIGHT: u32 = 270;
/// The frame's size when there is no window.
const FRAME_SIZE: (u32, u32) = (960, 540);
/// Events kept in `mark.json` (the latest); the counts cover them all.
const EVENTS_KEPT: usize = 40;

/// The longest note, in characters: one line.
const NOTE_MAX: usize = 120;
/// The bar's colours: a prompt, and a message.
const PROMPT: [u8; 4] = [255, 214, 90, 255];
const TOAST: [u8; 4] = [150, 220, 255, 255];

/// A note being typed in the window after F8.
#[cfg_attr(not(feature = "render"), allow(dead_code))]
pub(super) struct Prompt {
    /// The mark it is for.
    pub id: u64,
    /// What has been typed.
    pub text: String,
    /// The sim was playing when F8 went down, and plays on after.
    pub resume: bool,
}

/// Ticks of lead-in, ticks of clip, and ticks between clip pictures.
fn spans(kind: ClockKind, hz: f64) -> (u64, u64, u64, f64) {
    match kind {
        ClockKind::Continuous => {
            let hz = hz.max(1.0);
            let every = (hz / CLIP_FPS).round().max(1.0) as u64;
            (
                (LEAD_SECONDS * hz).round() as u64,
                (CLIP_SECONDS * hz).round() as u64,
                every,
                hz / every as f64,
            )
        }
        ClockKind::Discrete => (DISCRETE_LEAD, DISCRETE_CLIP, 1, DISCRETE_FPS),
    }
}

fn marks_dir(root: &Path) -> PathBuf {
    root.join(".argus").join("marks")
}

fn mark_dir(root: &Path, id: u64) -> PathBuf {
    marks_dir(root).join(id.to_string())
}

/// Mark ids on disk, oldest first.
fn ids(root: &Path) -> Vec<u64> {
    let mut out: Vec<u64> = std::fs::read_dir(marks_dir(root))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().join("mark.json").is_file())
                .filter_map(|e| e.file_name().to_str()?.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    out.sort_unstable();
    out
}

/// Marks still in the queue.
pub(super) fn open_count(root: &Path) -> usize {
    ids(root)
        .into_iter()
        .filter(|&id| read_mark(root, id).is_ok_and(|m| m["status"] == "open"))
        .count()
}

fn no_mark(id: u64) -> ArgusError {
    ArgusError::new(codes::NO_MARK, format!("there is no mark {id}"))
        .hint("mark.list shows the marks in .argus/marks/.")
}

fn bad_mark(id: u64, e: impl std::fmt::Display) -> ArgusError {
    ArgusError::new(codes::BAD_MARK, format!("mark {id}: {e}"))
}

fn read_mark(root: &Path, id: u64) -> Result<Json> {
    let path = mark_dir(root, id).join("mark.json");
    let text = std::fs::read_to_string(&path).map_err(|_| no_mark(id))?;
    serde_json::from_str(&text).map_err(|e| bad_mark(id, format!("mark.json: {e}")))
}

fn write_mark(root: &Path, mark: &Json) -> Result<()> {
    let id = mark["id"].as_u64().unwrap_or(0);
    let dir = mark_dir(root, id);
    std::fs::create_dir_all(&dir).map_err(|e| bad_mark(id, e))?;
    let text = serde_json::to_string_pretty(mark).expect("json");
    std::fs::write(dir.join("mark.json"), text + "\n").map_err(|e| bad_mark(id, e))
}

fn mark_id(params: &Json) -> Result<u64> {
    match params.get("id") {
        Some(Json::Number(n)) => n.as_u64(),
        Some(Json::String(s)) => s.trim_start_matches('#').parse().ok(),
        _ => None,
    }
    .ok_or_else(|| bad_params("give id: the mark's number, as mark.list shows it"))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// The input of `[from, to]` from the value log: the changes, and what was
/// already held when the window opened (a key held since before it).
fn input_window(st: &InputState, from: u64, to: u64) -> Json {
    let mut held: BTreeMap<(u8, String), &LogEntry> = BTreeMap::new();
    let mut changes = Vec::new();
    for e in st.log() {
        if e.tick < from {
            held.insert((e.slot, e.action.clone()), e);
        } else if e.tick <= to {
            changes.push(e.to_json());
        }
    }
    let held: Vec<Json> = held
        .values()
        .filter(|e| {
            st.map()
                .action(&e.action)
                .is_ok_and(|a| e.value != ActionValue::default_for(a.kind))
        })
        .map(|e| e.to_json())
        .collect();
    json!({ "from": from, "to": to, "held": held, "changes": changes })
}

/// A world diff as a mark reports it: counts, and the first few changes.
fn diff_json(then: &World, now: &World) -> Json {
    let d = crate::history::diff(then, now);
    json!({
        "same": d.appeared.is_empty() && d.vanished.is_empty() && d.changed.is_empty(),
        "appeared": d.appeared.len(),
        "vanished": d.vanished.len(),
        "changed": d.changed.len(),
        "first": d.changed.iter().take(8).map(|c| json!({
            "entity": crate::history::id_of(then, c.entity),
            "fields": c.fields.iter().take(6).map(|(p, a, b)| json!([
                p,
                argus_index::shapes::value_json(a),
                argus_index::shapes::value_json(b),
            ])).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// Every file a replay directory can hold.
const REPLAY_FILES: [&str; 6] = [
    "replay.bin",
    "replay.json",
    "frame.png",
    "clip.png",
    "clip-sheet.png",
    "clip.wav",
];

/// A replay directory as `replay.save` and `replay.open` report it.
fn replay_summary(dir: &Path, info: &Json) -> Json {
    let path = |f: &str| {
        dir.join(f)
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_string()
    };
    let file = |k: &str, f: &str| info[k][f].as_str().map(path);
    json!({
        "dir": dir.to_string_lossy().trim_start_matches(r"\\?\").to_string(),
        "frame": info["frame"],
        "scene": info["scene"],
        "note": info["note"],
        "about": info["about"],
        "picture": file("picture", "file"),
        "clip": file("clip", "file"),
        "sheet": file("clip", "sheet"),
        "sound": file("sound", "file"),
        "ms": info["ms"],
    })
}

impl Daemon {
    /// This moment, into `dir`: the replay file, the frame, the clip and
    /// its sheet, and the sound; returns what `mark.json` and
    /// `replay.json` hold about it (frame, run identity, digest, the input
    /// and events of the lead-in, the files). A mark and a replay are
    /// both this.
    fn capture(&mut self, dir: &Path, want_clip: bool, software: bool) -> Result<Json> {
        self.require_session()?;
        self.before_ticks();
        let started = std::time::Instant::now();
        let frame = self.sched.tick();
        let driving = self.sched.driving().clone();
        let (lead, clip_len, every, fps) = spans(driving.kind, driving.hz());
        let (root, scene, seed, ring, sources, input) = {
            let s = self.session.as_ref().expect("session");
            *s.sources.rooms.borrow_mut() = Some(s.scene.stream_state());
            (
                s.root.clone(),
                s.scene_name.clone(),
                s.seed,
                s.ring.clone(),
                s.sources.clone(),
                s.input.clone(),
            )
        };
        std::fs::create_dir_all(dir)
            .map_err(|e| ArgusError::new(codes::BAD_MARK, format!("{}: {e}", dir.display())))?;
        let code = code_version(&root);

        // The replay: where the lead-in starts, the commands since, the world now.
        let now = sources.capture(&self.world, &self.sched, false);
        let start = ring
            .borrow()
            .lead_in(frame, lead)
            .cloned()
            .unwrap_or_else(|| now.clone());
        let base = start.input.as_ref().map_or(0, |s| s.cursor());
        let upto = now.input.as_ref().map_or(0, |s| s.cursor());
        let journal: Vec<JournalEntry> = input
            .as_ref()
            .map(|i| i.borrow().journal()[base.min(upto)..upto].to_vec())
            .unwrap_or_default();
        let meta = json!({
            "scene": scene, "seed": seed, "code": code,
            "clock": driving.to_string(), "from": start.tick, "to": frame,
        });
        let bytes = replay::write(
            &dir.join("replay.bin"),
            &meta,
            &start,
            base,
            &journal,
            Some(&self.world),
        )?;

        let window_from = frame.saturating_sub(lead);
        let mut out = json!({
            "frame": frame,
            "time": round3(driving.time()),
            "clock": driving.to_string(),
            "scene": scene,
            "seed": seed,
            "code": code,
            "digest": format!("{:016x}", self.world.digest()),
            "input": input.as_ref().map(|i| input_window(&i.borrow(), window_from, frame)),
            "events": self.events_window(window_from, frame),
            "replay": { "file": "replay.bin", "from": start.tick, "to": frame, "bytes": bytes },
        });
        self.pictures(
            &mut out, dir, &start, frame, clip_len, every, fps, want_clip, software,
        );
        #[cfg(feature = "audio")]
        if let Some(a) = &self.audio {
            if let Ok((n, peak)) = a.clip(CLIP_SECONDS as f32, &dir.join("clip.wav")) {
                out["sound"] = json!({
                    "file": "clip.wav",
                    "seconds": CLIP_SECONDS,
                    "frames": n,
                    "peak_db": (peak as f64 * 10.0).round() / 10.0,
                });
            }
        }
        out["ms"] = json!((started.elapsed().as_secs_f64() * 1e4).round() / 10.0);
        Ok(out)
    }

    /// `mark.drop`: this moment as a mark. See the module docs.
    pub(super) fn mark_drop(&mut self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let note = json::opt_str(params, "note")?
            .unwrap_or("")
            .trim()
            .to_string();
        let author = json::opt_str(params, "author")?
            .unwrap_or("human")
            .to_string();
        let want_clip = json::bool_param(params, "clip", true)?;
        let id = ids(&root).last().map_or(1, |i| i + 1);
        let mut mark = self.capture(&mark_dir(&root, id), want_clip, false)?;
        let frame = mark["frame"].as_u64().unwrap_or(0);
        for (k, v) in [
            ("id", json!(id)),
            ("status", json!("open")),
            ("source", json!("desktop")),
            ("note", json!(note)),
            ("author", json!(author)),
            ("dropped_at", json!(unix_now())),
        ] {
            mark[k] = v;
        }
        write_mark(&root, &mark)?;
        self.emit_mark("mark.dropped", id, frame, &note);
        Ok(self.mark_summary(&root, &mark))
    }

    /// The frame and the clip, into `mark`: what they are, or why there
    /// are none. `software` draws with the software rasteriser, as CI does.
    #[allow(clippy::too_many_arguments)]
    fn pictures(
        &mut self,
        mark: &mut Json,
        dir: &Path,
        start: &crate::ring::Keyframe,
        frame: u64,
        clip_len: u64,
        every: u64,
        fps: f64,
        want_clip: bool,
        software: bool,
    ) {
        #[cfg(feature = "render")]
        {
            let (fw, fh) = self
                .render
                .window
                .as_ref()
                .map(|(h, _)| h.size())
                .filter(|(w, h)| *w > 1 && *h > 1)
                .unwrap_or(FRAME_SIZE);
            self.refresh_assets();
            match self
                .draw_offscreen((fw, fh), software)
                .and_then(|img| img.save_png(&dir.join("frame.png")))
            {
                Ok(()) => {
                    mark["picture"] = json!({ "file": "frame.png", "width": fw, "height": fh })
                }
                Err(e) => mark["picture"] = json!({ "skipped": e.message }),
            }
            if !want_clip {
                mark["clip"] = json!({ "skipped": "clip: false" });
                return;
            }
            let ch = CLIP_HEIGHT.min(fh);
            let cw = ((fw as u64 * ch as u64) / fh.max(1) as u64).max(2) as u32;
            let from = frame.saturating_sub(clip_len).max(start.tick);
            let t = std::time::Instant::now();
            mark["clip"] = match self
                .render_clip(start, from, frame, every, (cw, ch), software)
                .and_then(|frames| {
                    let bytes = argus_render::save_apng(&frames, fps, &dir.join("clip.png"))?;
                    // The same clip as one picture of eight, for a reader
                    // that sees the first picture of an animation only.
                    let labels: Vec<String> = (0..frames.len())
                        .map(|i| format!("{}", (from + i as u64 * every).min(frame)))
                        .collect();
                    if let Some(sheet) = argus_render::contact_sheet(&frames, &labels) {
                        sheet.save_png(&dir.join("clip-sheet.png"))?;
                    }
                    Ok((frames.len(), bytes))
                }) {
                Ok((n, bytes)) => json!({
                    "file": "clip.png", "sheet": "clip-sheet.png", "frames": n,
                    "fps": (fps * 10.0).round() / 10.0,
                    "width": cw, "height": ch, "from": from, "to": frame, "bytes": bytes,
                    "ms": (t.elapsed().as_secs_f64() * 1e4).round() / 10.0,
                }),
                Err(e) => json!({ "skipped": e.message }),
            };
        }
        #[cfg(not(feature = "render"))]
        {
            let _ = (dir, start, frame, clip_len, every, fps, want_clip, software);
            mark["picture"] = json!({ "skipped": "this daemon was built without the renderer" });
            mark["clip"] = json!({ "skipped": "this daemon was built without the renderer" });
        }
    }

    /// The world now, drawn offscreen at alpha 1.
    #[cfg(feature = "render")]
    fn draw_offscreen(
        &mut self,
        (w, h): (u32, u32),
        software: bool,
    ) -> Result<argus_render::Image> {
        let _ = self.sched.present(&mut self.world);
        let tick = self.sched.tick();
        let extras = self.tile_extras(argus_render::view_for(&self.world, w, h));
        let ses = self.session.as_ref().expect("session");
        let r = self.render.offscreen(w, h, software, &ses.assets)?;
        r.set_time(shader_time(&self.sched, 1.0));
        let store = ses.particles.borrow();
        r.render_frame(&self.world, tick, 1.0, &extras, Some(&store))?;
        drop(store);
        r.capture()
    }

    /// Pictures of `from..=to`, one every `every` ticks, replayed quietly
    /// from `start`; the live state is put back after, whatever happened.
    #[cfg(feature = "render")]
    fn render_clip(
        &mut self,
        start: &crate::ring::Keyframe,
        from: u64,
        to: u64,
        every: u64,
        size: (u32, u32),
        software: bool,
    ) -> Result<Vec<argus_render::Image>> {
        let sources = self.session.as_ref().expect("session").sources.clone();
        let now = sources.capture(&self.world, &self.sched, false);
        let out = self.clip_frames(&sources, start, from, to, every, size, software);
        self.put_back(&sources, &now);
        self.sched.set_quiet(false);
        out
    }

    #[cfg(feature = "render")]
    #[allow(clippy::too_many_arguments)]
    fn clip_frames(
        &mut self,
        sources: &crate::ring::Sources,
        start: &crate::ring::Keyframe,
        from: u64,
        to: u64,
        every: u64,
        size: (u32, u32),
        software: bool,
    ) -> Result<Vec<argus_render::Image>> {
        self.put_back(sources, start);
        self.sched.set_quiet(true);
        let discrete = self.sched.driving().kind == ClockKind::Discrete;
        let step = |d: &mut Daemon, n: u64| -> Result<()> {
            if discrete {
                d.sched.advance(&mut d.world, n).map(|_| ())
            } else {
                d.sched.step(&mut d.world, n).map(|_| ())
            }?;
            let _ = d.world.drain_events();
            Ok(())
        };
        if from > start.tick {
            step(self, from - start.tick)?;
        }
        let mut frames = vec![self.draw_offscreen(size, software)?];
        let mut t = from;
        while t < to {
            let n = every.min(to - t);
            step(self, n)?;
            t += n;
            frames.push(self.draw_offscreen(size, software)?);
        }
        Ok(frames)
    }

    /// Restore a keyframe as a seek does: the world, the scheduler, the
    /// input, the pools, the rooms; its queued events are the log's already.
    fn put_back(&mut self, sources: &crate::ring::Sources, k: &crate::ring::Keyframe) {
        sources.restore(k, &mut self.world, &mut self.sched);
        if let (Some(rooms), Some(s)) = (&k.rooms, self.session.as_mut()) {
            s.scene.set_stream_state(rooms);
        }
        let _ = self.world.drain_events();
    }

    /// The game events of `[from, to]`: counts by name, and the latest.
    fn events_window(&self, from: u64, to: u64) -> Json {
        let mut counts: BTreeMap<String, u64> = BTreeMap::new();
        let mut last: VecDeque<Json> = VecDeque::new();
        for (seq, r) in &self.events.records {
            if r.frame < from || r.frame > to {
                continue;
            }
            if let Event::Game { name, .. } = &r.event {
                *counts.entry(name.clone()).or_default() += 1;
                last.push_back(event_json(&self.world, *seq, r));
                if last.len() > EVENTS_KEPT {
                    last.pop_front();
                }
            }
        }
        json!({ "from": from, "to": to, "counts": counts, "last": last })
    }

    /// A mark event in the log, between ticks, for whoever is watching.
    fn emit_mark(&mut self, name: &str, id: u64, frame: u64, note: &str) {
        self.world.emit(Event::Game {
            name: name.to_string(),
            fields: vec![
                ("id".into(), Value::UInt(id)),
                ("frame".into(), Value::UInt(frame)),
                ("note".into(), Value::Str(note.to_string())),
            ],
        });
        self.pump_events();
    }

    /// What a list row or a drop reports: the mark without its bulk, with
    /// the absolute paths of its files.
    fn mark_summary(&self, root: &Path, mark: &Json) -> Json {
        let id = mark["id"].as_u64().unwrap_or(0);
        let dir = mark_dir(root, id);
        let path = |f: &str| {
            dir.join(f)
                .to_string_lossy()
                .trim_start_matches(r"\\?\")
                .to_string()
        };
        let file = |k: &str| mark[k]["file"].as_str().map(path);
        json!({
            "id": id,
            "status": mark["status"],
            "source": mark["source"],
            "frame": mark["frame"],
            "time": mark["time"],
            "scene": mark["scene"],
            "note": mark["note"],
            "author": mark["author"],
            "dropped_at": mark["dropped_at"],
            "dir": dir.to_string_lossy().trim_start_matches(r"\\?\"),
            "picture": file("picture"),
            "clip": file("clip"),
            "sheet": mark["clip"]["sheet"].as_str().map(path),
            "sound": file("sound"),
            "ms": mark["ms"],
        })
    }

    /// `mark.list`: the queue, oldest first; `all` includes closed marks.
    pub(super) fn mark_list(&self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let all = json::bool_param(params, "all", false)?;
        let mut rows = Vec::new();
        let (mut open, mut closed) = (0, 0);
        for id in ids(&root) {
            let Ok(m) = read_mark(&root, id) else {
                continue;
            };
            let is_open = m["status"] == "open";
            if is_open {
                open += 1;
            } else {
                closed += 1;
            }
            if is_open || all {
                rows.push(self.mark_summary(&root, &m));
            }
        }
        Ok(json!({ "open": open, "closed": closed, "rows": rows }))
    }

    /// `mark.get`: everything `mark.json` holds, with the files' paths.
    pub(super) fn mark_get(&self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let id = mark_id(params)?;
        let mut m = read_mark(&root, id)?;
        m["paths"] = self.mark_summary(&root, &m);
        Ok(m)
    }

    /// `mark.note`: set a mark's note (the window's prompt calls this too).
    pub(super) fn mark_note(&mut self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let id = mark_id(params)?;
        let note = json::str_param(params, "note")?.trim().to_string();
        self.set_note(&root, id, &note)
    }

    fn set_note(&mut self, root: &Path, id: u64, note: &str) -> Result<Json> {
        let mut m = read_mark(root, id)?;
        m["note"] = json!(note);
        write_mark(root, &m)?;
        let frame = m["frame"].as_u64().unwrap_or(0);
        self.emit_mark("mark.noted", id, frame, note);
        Ok(self.mark_summary(root, &m))
    }

    /// `mark.close`: done with a mark; it leaves the queue. `note` says what
    /// was done ("fixed: dash buffer 6 frames").
    pub(super) fn mark_close(&mut self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let id = mark_id(params)?;
        let mut m = read_mark(&root, id)?;
        m["status"] = json!("closed");
        m["closed"] = json!({
            "note": json::opt_str(params, "note")?.unwrap_or(""),
            "by": json::opt_str(params, "author")?.unwrap_or("agent"),
            "at": unix_now(),
        });
        write_mark(&root, &m)?;
        let frame = m["frame"].as_u64().unwrap_or(0);
        let note = m["closed"]["note"].as_str().unwrap_or("").to_string();
        self.emit_mark("mark.closed", id, frame, &note);
        Ok(self.mark_summary(&root, &m))
    }

    /// `mark.import`: a mark from the web build (its JSON), into the queue.
    pub(super) fn mark_import(&mut self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let path = PathBuf::from(json::str_param(params, "path")?);
        let path = if path.is_absolute() {
            path
        } else {
            root.join(path)
        };
        let text = std::fs::read_to_string(&path)
            .map_err(|e| ArgusError::new(codes::BAD_MARK, format!("{}: {e}", path.display())))?;
        let web: Json = serde_json::from_str(&text).map_err(|e| {
            ArgusError::new(
                codes::BAD_MARK,
                format!("{}: not a mark: {e}", path.display()),
            )
        })?;
        let need = |k: &str| {
            web.get(k).filter(|v| !v.is_null()).cloned().ok_or_else(|| {
                ArgusError::new(codes::BAD_MARK, format!("{}: a web mark needs {k}", path.display()))
                    .hint("A web mark has scene, seed, code, tick, digest, log, and note (WebPlayer.mark).")
            })
        };
        let (scene, seed, tick, log) = (need("scene")?, need("seed")?, need("tick")?, need("log")?);
        let id = ids(&root).last().map_or(1, |i| i + 1);
        let frame = tick.as_u64().unwrap_or(0);
        let note = web["note"].as_str().unwrap_or("").to_string();
        let mark = json!({
            "id": id,
            "status": "open",
            "source": "web",
            "frame": frame,
            "scene": scene,
            "seed": seed,
            "code": web["code"],
            "note": note,
            "author": params.get("author").cloned().unwrap_or(json!("web")),
            "dropped_at": unix_now(),
            "digest": web["digest"],
            "events_digest": web["events_digest"],
            "log": log,
            "imported_from": path.to_string_lossy(),
        });
        write_mark(&root, &mark)?;
        self.emit_mark("mark.dropped", id, frame, &note);
        Ok(self.mark_summary(&root, &mark))
    }

    /// `mark.open`: reproduce a mark. See the module docs.
    pub(super) fn mark_open(&mut self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let id = mark_id(params)?;
        let mut mark = read_mark(&root, id)?;
        let started = std::time::Instant::now();
        let scene = mark["scene"].as_str().unwrap_or_default().to_string();
        let seed = mark["seed"].as_u64();
        let frame = mark["frame"].as_u64().unwrap_or(0);
        let file = if mark["source"] == "web" {
            None
        } else {
            Some(
                replay::read(&mark_dir(&root, id).join("replay.bin"))
                    .map_err(|e| bad_mark(id, e.message))?,
            )
        };
        self.load_for(&root, &scene, seed)
            .map_err(|e| bad_mark(id, e.message))?;
        let mut out = match file {
            None => self.open_web(id, &mark, frame)?,
            Some(file) => self.open_file(file, frame)?,
        };
        out["id"] = json!(id);
        self.after_open();
        let digest = format!("{:016x}", self.world.digest());
        let same = mark["digest"].as_str() == Some(digest.as_str());
        out["digest"] = json!({ "mark": mark["digest"], "now": digest, "same": same });
        out["code"] = json!({ "mark": mark["code"], "now": code_version(&root) });
        // A mark without its clip (a web mark, or dropped with no GPU) gets
        // one now, from the lead-in just played.
        let dir = mark_dir(&root, id);
        if mark["clip"]["file"].is_null() {
            let driving = self.sched.driving().clone();
            let (_, clip_len, every, fps) = spans(driving.kind, driving.hz());
            let start = self
                .session
                .as_ref()
                .and_then(|s| s.ring.borrow().lead_in(frame, clip_len).cloned());
            if let Some(start) = start {
                self.pictures(
                    &mut mark, &dir, &start, frame, clip_len, every, fps, true, false,
                );
                write_mark(&root, &mark)?;
            }
        }
        out["mark"] = self.mark_summary(&root, &mark);
        out["note"] = mark["note"].clone();
        out["ms"] = json!((started.elapsed().as_secs_f64() * 1e4).round() / 10.0);
        Ok(out)
    }

    /// A fresh load of a scene with a seed, for a mark or a replay: the run
    /// before it is another run.
    fn load_for(&mut self, root: &Path, scene: &str, seed: Option<u64>) -> Result<()> {
        self.forks.clear();
        self.active = "main".to_string();
        let cfg = ProjectConfig::load(root)?;
        self.open_scene(root.to_path_buf(), &cfg, scene, seed)
            .map(|_| ())
            .map_err(|e| {
                ArgusError::new(
                    codes::BAD_MARK,
                    format!("its scene {scene:?} does not load: {}", e.message),
                )
            })
    }

    /// After a replay: no sound effects from its lead-in, and the window
    /// does not blend from the world before it.
    fn after_open(&mut self) {
        #[cfg(feature = "audio")]
        if let Some(a) = self.audio.as_mut() {
            a.silence();
        }
        #[cfg(feature = "render")]
        if let Some((_, r)) = &mut self.render.window {
            r.forget_motion();
        }
    }

    /// Restore a replay file's keyframe, play the lead-in to `frame`, and
    /// compare with the world the file saved.
    fn open_file(&mut self, file: replay::ReplayFile, frame: u64) -> Result<Json> {
        let (sources, ring, input, particles) = {
            let s = self.session.as_ref().expect("session");
            (
                s.sources.clone(),
                s.ring.clone(),
                s.input.clone(),
                s.particles.clone(),
            )
        };
        let back = file.keyframe(&self.world, &particles.borrow())?;
        let k = back.keyframe;
        if let (Some(i), Some(s)) = (&input, &k.input) {
            i.borrow_mut().load_journal(file.journal.clone(), s);
        }
        self.put_back(&sources, &k);
        self.events.through = k.tick;
        {
            let mut r = ring.borrow_mut();
            r.restart(k.clone());
            r.mutations = self.mutations;
        }
        let n = frame.saturating_sub(k.tick);
        self.play_ticks(n)?;
        let diff = match file.end_world(&self.world)? {
            Some(then) => diff_json(&then, &self.world),
            None => Json::Null,
        };
        Ok(json!({
            "frame": self.sched.tick(),
            "from": k.tick,
            "replayed": n,
            "against_mark": diff,
            "remapped": back.world.remapped,
            "dropped": back.world.dropped,
            "missing_effects": back.missing_effects,
        }))
    }

    /// A web mark: its input log from the load, played to its frame.
    fn open_web(&mut self, id: u64, mark: &Json, frame: u64) -> Result<Json> {
        let st = self.input()?;
        let map = st.borrow().map().clone();
        let mut entries = Vec::new();
        for r in mark["log"].as_array().cloned().unwrap_or_default() {
            let line = format!(
                "{}\t{}\t{}\t{}",
                r[0],
                r[1],
                r[2].as_str().unwrap_or(""),
                r[3]
            );
            entries.push(LogEntry::parse(&line, &map).map_err(|e| bad_mark(id, e.message))?);
        }
        let now = self.sched.tick();
        st.borrow_mut().replay(now, &entries)?;
        let n = frame.saturating_sub(now);
        self.play_ticks(n)?;
        Ok(
            json!({ "id": id, "frame": self.sched.tick(), "from": now, "replayed": n, "against_mark": null }),
        )
    }

    /// Keys from the window, before the game hears them. F8 (`[marks] key`
    /// in argus.toml) drops a mark,
    /// pauses, and opens the note prompt; while it is open every key and
    /// character goes to the note (Backspace, Enter saves, Escape skips)
    /// and the game hears only that keys went up.
    pub(super) fn mark_keys(
        &mut self,
        evs: Vec<DeviceInput>,
        typed: Vec<String>,
    ) -> Vec<DeviceInput> {
        let up = |e: &DeviceInput| matches!(e, DeviceInput::ReleaseAll);
        if let Some(p) = self.mark_prompt.as_mut() {
            for t in typed {
                for c in t.chars() {
                    if p.text.chars().count() < NOTE_MAX {
                        p.text.push(c);
                    }
                }
            }
            let mut done = None;
            for e in &evs {
                if let DeviceInput::Key {
                    name,
                    pressed: true,
                } = e
                {
                    match name.as_str() {
                        "Backspace" => {
                            p.text.pop();
                        }
                        "Enter" | "NumpadEnter" => done = Some(true),
                        "Escape" => done = Some(false),
                        _ => {}
                    }
                }
            }
            if let Some(save) = done {
                self.finish_prompt(save);
            }
            return evs.into_iter().filter(up).collect();
        }
        let Some(key) = self.session.as_ref().map(|s| s.mark_key.clone()) else {
            return evs;
        };
        let is_key = |e: &DeviceInput| matches!(e, DeviceInput::Key { name, .. } if *name == key);
        let f8 = evs
            .iter()
            .any(|e| matches!(e, DeviceInput::Key { name, pressed: true } if *name == key));
        let rest: Vec<DeviceInput> = evs.into_iter().filter(|e| !is_key(e)).collect();
        if !f8 {
            return rest;
        }
        // What came before F8 in this batch happened before the mark.
        if !rest.is_empty() {
            self.device_input(&rest);
        }
        self.start_prompt();
        vec![DeviceInput::ReleaseAll]
    }

    /// F8: drop a mark now, pause, and ask for its note.
    fn start_prompt(&mut self) {
        let resume = self.sched.is_playing() && !self.sched.is_paused();
        if resume {
            self.sched.pause();
        }
        match self.mark_drop(&json!({ "author": "human" })) {
            Ok(m) => {
                self.mark_prompt = Some(Prompt {
                    id: m["id"].as_u64().unwrap_or(0),
                    text: String::new(),
                    resume,
                });
            }
            Err(e) => {
                self.toast = Some((
                    format!("NO MARK: {}", e.message),
                    Instant::now() + Duration::from_secs(5),
                ));
                if resume {
                    self.sched.play(&self.world);
                }
            }
        }
    }

    /// The prompt closes: keep the note (Enter) or not (Escape), and play on.
    pub(super) fn finish_prompt(&mut self, save: bool) {
        let Some(p) = self.mark_prompt.take() else {
            return;
        };
        let note = p.text.trim().to_string();
        let kept = save && !note.is_empty();
        if kept {
            if let Some(root) = self.session.as_ref().map(|s| s.root.clone()) {
                let _ = self.set_note(&root, p.id, &note);
            }
        }
        let text = if kept {
            format!("MARK {} SAVED: {note}", p.id)
        } else {
            format!("MARK {} SAVED", p.id)
        };
        self.toast = Some((text, Instant::now() + Duration::from_millis(2500)));
        if p.resume {
            self.sched.play(&self.world);
        }
    }

    /// What the window's bottom bar says now, if anything.
    fn bar_text(&mut self) -> Option<(String, [u8; 4])> {
        if let Some(p) = &self.mark_prompt {
            let when = match self.sched.driving().kind {
                ClockKind::Discrete => format!("TURN {}", self.sched.tick()),
                ClockKind::Continuous => {
                    let t = self.sched.driving().time();
                    format!("{}:{:04.1}", (t / 60.0).floor() as u64, t % 60.0)
                }
            };
            return Some((
                format!(
                    "MARK {} AT {when}  NOTE: {}_   ENTER SAVES, ESC SKIPS",
                    p.id, p.text
                ),
                PROMPT,
            ));
        }
        match &self.toast {
            Some((text, until)) if Instant::now() < *until => Some((text.clone(), TOAST)),
            Some(_) => {
                self.toast = None;
                None
            }
            None => None,
        }
    }

    /// Put the bar on the window, when it changed.
    #[cfg(feature = "render")]
    pub(super) fn update_bar(&mut self, width: u32) {
        let want = self.bar_text();
        let key = want.as_ref().map(|(t, _)| (t.clone(), width));
        if key == self.render.bar {
            return;
        }
        if let Some((_, r)) = &mut self.render.window {
            match &want {
                Some((text, rgba)) => {
                    let img = argus_render::status_bar(width.max(64), text, *rgba);
                    r.gpu_mut().set_overlay(Some(&img));
                }
                None => r.gpu_mut().set_overlay(None),
            }
        }
        self.render.bar = key;
    }

    /// `replay.save`: this moment as a replay directory, with no queue: the
    /// replay file, the frame, the clip and its sheet, the sound, and
    /// `replay.json` (what `mark.json` holds, plus a note and whatever
    /// `about` says). The test runner saves one for each failing run
    /// (phase 2 item 3); an agent can save one of anything.
    pub(super) fn replay_save(&mut self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let dir = match json::opt_str(params, "dir")? {
            Some(d) if Path::new(d).is_absolute() => PathBuf::from(d),
            Some(d) => root.join(d),
            None => root.join(".argus").join("replays").join(format!(
                "{}-{}",
                unix_now(),
                self.sched.tick()
            )),
        };
        let clip = json::bool_param(params, "clip", true)?;
        let software = json::bool_param(params, "software", false)?;
        // What a save to this directory left before is another moment's.
        for f in REPLAY_FILES {
            let _ = std::fs::remove_file(dir.join(f));
        }
        let mut info = self.capture(&dir, clip, software)?;
        info["note"] = json!(json::opt_str(params, "note")?.unwrap_or(""));
        info["saved_at"] = json!(unix_now());
        if let Some(about) = params.get("about").filter(|v| !v.is_null()) {
            info["about"] = about.clone();
        }
        let text = serde_json::to_string_pretty(&info).expect("json") + "\n";
        std::fs::write(dir.join("replay.json"), text)
            .map_err(|e| ArgusError::new(codes::BAD_MARK, format!("{}: {e}", dir.display())))?;
        Ok(replay_summary(&dir, &info))
    }

    /// `replay.open`: reproduce a saved replay (a directory `replay.save`
    /// or a failing test wrote, or its `replay.bin`): load its scene with
    /// its seed, restore the keyframe, play the lead-in, and compare the
    /// world with the one it saved. Paused at its frame; `sim.seek` moves
    /// within the lead-in and a window shows it.
    pub(super) fn replay_open(&mut self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let path = PathBuf::from(json::str_param(params, "path")?);
        let path = if path.is_absolute() {
            path
        } else {
            root.join(path)
        };
        let dir = if path.is_file() {
            path.parent().map(Path::to_path_buf).unwrap_or_default()
        } else {
            path
        };
        let started = std::time::Instant::now();
        let file = replay::read(&dir.join("replay.bin"))?;
        let info: Json = std::fs::read_to_string(dir.join("replay.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Json::Null);
        let scene = file.meta["scene"].as_str().unwrap_or_default().to_string();
        let seed = file.meta["seed"].as_u64();
        let code = file.meta["code"].clone();
        let frame = file.to;
        self.load_for(&root, &scene, seed)?;
        let mut out = self.open_file(file, frame)?;
        self.after_open();
        let digest = format!("{:016x}", self.world.digest());
        let same = info["digest"].as_str().map(|d| d == digest);
        out["digest"] = json!({ "saved": info["digest"], "now": digest, "same": same });
        out["code"] = json!({ "saved": code, "now": code_version(&root) });
        out["replay"] = replay_summary(&dir, &info);
        out["ms"] = json!((started.elapsed().as_secs_f64() * 1e4).round() / 10.0);
        Ok(out)
    }

    /// Run `n` ticks (turns on a discrete clock) as `sim.step` does, logging
    /// their events and feeding the traces.
    fn play_ticks(&mut self, n: u64) -> Result<()> {
        if n == 0 {
            return Ok(());
        }
        match self.sched.driving().kind {
            ClockKind::Continuous => self.sched.step(&mut self.world, n)?,
            ClockKind::Discrete => self.sched.advance(&mut self.world, n)?,
        };
        self.pump_ticks();
        Ok(())
    }
}
