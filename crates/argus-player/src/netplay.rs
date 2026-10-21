//! A player in a match (docs/multiplayer-checklist.md item 1): the
//! [`argus_net::Peer`] a [`argus_net::Match`] runs.
//!
//! A tick is snapshotted, run, and maybe run again after a rollback, so
//! what it does outside the world copes with that:
//!
//! - **Sounds** start at once, since a late sound feels wrong. After a
//!   rollback, a sound the ticks run again no longer make is stopped, one
//!   they make again is not played twice, and a new one plays then.
//! - **Everything else** a tick asks of the host (a message to the page, a
//!   save to load or delete, an achievement), and the event counts and
//!   digest, waits until the tick is final: every player's input for it
//!   in, and the tick run with it.
//! - **The recorder** takes its keyframes only when nothing is predicted,
//!   so a report holds the match as it was finally played.

use crate::keyframe::Keyframe;
use crate::player::Player;
use argus_ecs::{Event, EventRecord, Result, Value};
use argus_input::{ActionValue, DeviceInput};
use argus_net::Change;
use std::collections::BTreeMap;

/// A sound a tick started: the tick, which sound from which entity, its
/// place among that tick's like sounds, and the mixer's voice.
#[derive(Clone, Debug, PartialEq)]
struct Heard {
    tick: u64,
    key: (String, u64, u32),
    voice: Option<i64>,
}

/// What a player in a match keeps besides the game.
#[derive(Default)]
pub struct NetState {
    /// The local player's actions as its devices last gave them.
    devices_last: BTreeMap<String, ActionValue>,
    /// Sounds started in ticks not yet final.
    heard: Vec<Heard>,
    /// During a run again: the sounds the first run started from its tick.
    expected: Option<Vec<Heard>>,
    /// Events of ticks not yet final, for the host when they are.
    waiting: Vec<EventRecord>,
    sounds: NetSounds,
}

/// What a match's rollbacks did to the sounds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetSounds {
    /// Sounds a run again made again, left playing.
    pub kept: u64,
    /// Sounds only a run again made, played then, late.
    pub late: u64,
    /// Sounds a run again no longer made, stopped.
    pub stopped: u64,
}

fn sound_key(fields: &[(String, Value)]) -> (String, u64) {
    let get = |k: &str| fields.iter().find(|(n, _)| n == k).map(|(_, v)| v);
    let name = match get("sound") {
        Some(Value::Str(s)) => s.clone(),
        _ => String::new(),
    };
    let entity = match get("entity") {
        Some(Value::Entity(e)) => e.to_bits(),
        Some(Value::Str(s)) => argus_ecs::intern::text_hash(s),
        _ => 0,
    };
    (name, entity)
}

impl Player {
    /// Play in a match from now on: [`Player::net_devices`] instead of
    /// [`Player::devices`], and the match steps the game.
    pub fn join_match(&mut self) {
        self.net = Some(NetState::default());
    }

    /// Whether the player is in a match.
    pub fn in_match(&self) -> bool {
        self.net.is_some()
    }

    /// What rollbacks did to the sounds so far, in a match.
    pub fn net_sounds(&self) -> Option<NetSounds> {
        self.net.as_ref().map(|n| n.sounds)
    }

    /// Device input for the local player in a match: the actions it
    /// changed, read through the bindings of slot 1 (the devices of the
    /// one playing at this machine), for [`argus_net::Match::local`]. Nothing
    /// is scheduled: the match applies them at their tick.
    pub fn net_devices(&mut self, evs: &[DeviceInput]) -> Vec<(String, ActionValue)> {
        for ev in evs {
            self.devices.apply(ev);
        }
        let Some(st) = self.input.clone() else {
            return Vec::new();
        };
        let canvas = {
            let t = &self.ui.borrow().theme;
            [t.width, t.height]
        };
        let now = {
            let mut s = st.borrow_mut();
            s.set_canvas(canvas);
            self.devices.set_touch_layout(&s.touch_layout());
            self.devices.resolve(s.map(), 1)
        };
        let Some(net) = self.net.as_mut() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (action, v) in now {
            let kind = st.borrow().map().action(&action).map(|a| a.kind).ok();
            let rest = kind.map(ActionValue::default_for);
            let before = net.devices_last.get(&action).copied().or(rest);
            if Some(v) != before {
                net.devices_last.insert(action.clone(), v);
                out.push((action, v));
            }
        }
        out
    }

    /// A match's tick: `changes` applied as it begins, then the tick.
    fn net_step(&mut self, changes: &[Change], again: bool) -> Result<()> {
        if let Some(st) = &self.input {
            st.borrow_mut().apply_changes(self.sched.tick(), changes);
        }
        self.sched.set_quiet(again);
        let r = match self.sched.driving().kind {
            argus_sim::ClockKind::Continuous => self.sched.step(&mut self.world, 1),
            argus_sim::ClockKind::Discrete => self.sched.advance(&mut self.world, 1),
        };
        self.sched.set_quiet(false);
        r?;
        self.pump();
        Ok(())
    }

    /// The match's half of [`Player::pump`]: sounds now, the rest when final.
    pub(crate) fn pump_net(&mut self, drained: Vec<EventRecord>) {
        let mut counts: BTreeMap<(String, u64, u64), u32> = BTreeMap::new();
        for rec in drained {
            let Event::Game { name, fields } = &rec.event else {
                self.net.as_mut().unwrap().waiting.push(rec);
                continue;
            };
            if name == "sound.play" {
                let (sound, entity) = sound_key(fields);
                let n = counts
                    .entry((sound.clone(), entity, rec.frame))
                    .or_default();
                *n += 1;
                let key = (sound, entity, *n);
                let net = self.net.as_mut().unwrap();
                if let Some(exp) = net.expected.as_mut() {
                    if let Some(i) = exp.iter().position(|h| h.tick == rec.frame && h.key == key) {
                        // The first run started it: it plays on.
                        let h = exp.remove(i);
                        net.heard.push(h);
                        net.sounds.kept += 1;
                        net.waiting.push(rec);
                        continue;
                    }
                }
                let voice = self.play_now(&rec);
                let net = self.net.as_mut().unwrap();
                if net.expected.is_some() {
                    net.sounds.late += 1;
                }
                net.heard.push(Heard {
                    tick: rec.frame,
                    key,
                    voice,
                });
                net.waiting.push(rec);
                continue;
            }
            if name == "sound.stop" {
                self.play_now(&rec);
            }
            self.net.as_mut().unwrap().waiting.push(rec);
        }
        self.audio_follow();
    }

    /// Answer a sound request now; the voice it started.
    fn play_now(&mut self, rec: &EventRecord) -> Option<i64> {
        let (Event::Game { name, fields }, Some(a)) = (&rec.event, self.audio.as_mut()) else {
            return None;
        };
        a.at_frame(rec.frame);
        let mut voice = None;
        for (n, f) in a.request(&self.world, name, fields) {
            if n == "sound.played" {
                self.sounds += 1;
                voice = f.iter().find_map(|(k, v)| match (k.as_str(), v) {
                    ("voice", Value::Int(i)) => Some(*i),
                    _ => None,
                });
            }
        }
        voice
    }

    fn stop_voice(&mut self, voice: i64) {
        if let Some(a) = self.audio.as_mut() {
            a.request(
                &self.world,
                "sound.stop",
                &[
                    ("voice".to_string(), Value::Int(voice)),
                    ("fade_ms".to_string(), Value::Float(30.0)),
                ],
            );
        }
    }
}

impl argus_net::Peer for Player {
    type Saved = Keyframe;

    fn tick(&self) -> u64 {
        self.sched.tick()
    }

    fn actions(&self) -> Vec<String> {
        self.input
            .as_ref()
            .map(|i| {
                i.borrow()
                    .map()
                    .actions
                    .iter()
                    .map(|a| a.name.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn save(&self) -> Keyframe {
        self.sources.capture(&self.world, &self.sched, false)
    }

    fn load(&mut self, k: &Keyframe) {
        self.sources.restore(k, &mut self.world, &mut self.sched);
        self.world.drain_events();
        if let Some(st) = &self.input {
            st.borrow_mut().rewind_to(k.tick);
        }
    }

    fn takes_turn(&self, changes: &[(String, ActionValue)]) -> bool {
        let Some(st) = &self.input else {
            return false;
        };
        let st = st.borrow();
        changes.iter().any(|(action, v)| {
            // A place, not a press, as a turn-based game alone reads it.
            st.map().action(action).is_ok_and(|a| {
                a.kind != argus_input::ActionKind::Pointer
                    && action != argus_input::map::DEVICE_ACTION
                    && v.is_active()
            })
        })
    }

    fn step(&mut self, changes: &[Change], again: bool) -> Result<()> {
        self.net_step(changes, again)
    }

    fn rolled_back(&mut self, tick: u64) {
        let Some(net) = self.net.as_mut() else {
            return;
        };
        // What those ticks did comes again from the run again.
        net.waiting.retain(|r| r.frame < tick);
        let (keep, redo): (Vec<Heard>, Vec<Heard>) =
            net.heard.drain(..).partition(|h| h.tick < tick);
        net.heard = keep;
        net.expected = Some(redo);
    }

    fn caught_up(&mut self) {
        let gone = self
            .net
            .as_mut()
            .and_then(|n| n.expected.take())
            .unwrap_or_default();
        if let Some(n) = self.net.as_mut() {
            n.sounds.stopped += gone.len() as u64;
        }
        for h in gone {
            if let Some(v) = h.voice {
                self.stop_voice(v);
            }
        }
    }

    fn confirmed(&mut self, tick: u64) {
        let Some(net) = self.net.as_mut() else {
            return;
        };
        net.heard.retain(|h| h.tick >= tick);
        let (done, later): (Vec<EventRecord>, Vec<EventRecord>) =
            net.waiting.drain(..).partition(|r| r.frame < tick);
        net.waiting = later;
        for rec in &done {
            self.external(rec);
        }
        // A keyframe only of a world nothing in is a guess.
        if tick == self.sched.tick() {
            if let Some(r) = self.recorder.as_mut() {
                r.after_ticks(&self.world, &self.sched, &self.sources);
            }
        }
    }

    fn digest(&self) -> u64 {
        let cells = self.tiles.borrow().runtime_cells();
        match argus_tiles::cells_digest(&cells) {
            None => self.world.column_digest(),
            Some(c) => self.world.column_digest() ^ c.rotate_left(1),
        }
    }

    fn rows_of(&self, k: &Keyframe) -> Vec<String> {
        let mut rows = k.sim.world.world().digest_rows();
        for (file, chunks) in &k.tiles {
            for (layer, cx, cy, d) in chunks {
                rows.push(format!(
                    "map {file} {layer} {cx},{cy} = {:016x}",
                    argus_tiles::fnv1a64(d)
                ));
            }
        }
        rows
    }
}
