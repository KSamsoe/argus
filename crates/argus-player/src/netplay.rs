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
use argus_luau::NetView;
use argus_net::{Advance, Change, Config, Lobby, Match, Phase, RoomLink, Start, State, Wire};
use std::collections::BTreeMap;

/// What the game's menu asked of whatever hosts it.
#[derive(Clone, Debug, PartialEq)]
pub enum NetAsk {
    /// `q:net_host(players, scene)`.
    Host {
        /// Players.
        players: u8,
        /// The match's scene, when the menu named one.
        scene: Option<String>,
    },
    /// `q:net_join(code)`.
    Join {
        /// The room's code.
        code: String,
    },
    /// `q:net_leave()`.
    Leave,
}

impl NetAsk {
    /// The ask an event is, if it is one.
    pub fn from_event(name: &str, fields: &[(String, Value)]) -> Option<NetAsk> {
        let get = |k: &str| fields.iter().find(|(n, _)| n == k).map(|(_, v)| v);
        match name {
            "net.host_requested" => Some(NetAsk::Host {
                players: match get("players") {
                    Some(Value::Int(n)) => (*n).clamp(2, 4) as u8,
                    _ => 2,
                },
                scene: match get("scene") {
                    Some(Value::Str(s)) => Some(s.clone()),
                    _ => None,
                },
            }),
            "net.join_requested" => match get("code") {
                Some(Value::Str(c)) => Some(NetAsk::Join { code: c.clone() }),
                _ => None,
            },
            "net.leave_requested" => Some(NetAsk::Leave),
            _ => None,
        }
    }
}

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
    /// The local player's changes from the devices, for the match.
    local: Vec<(String, ActionValue)>,
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
    /// What the game's menu asked (host, join, leave), since the last call.
    pub fn take_net_asks(&mut self) -> Vec<NetAsk> {
        std::mem::take(&mut self.net_asks)
    }

    /// What scripts read with `q:net()` from the next tick on.
    pub fn set_net_view(&mut self, view: NetView) {
        *self.net_view.borrow_mut() = view;
    }

    /// The game's shape, which a relay checks a joining player against: its
    /// components and their fields, its actions, and the engine's version.
    /// The same game built for the web or the desktop has the same one.
    pub fn fingerprint(&self) -> u64 {
        argus_net::fingerprint(&self.world, &argus_net::Peer::actions(self))
    }

    /// Out of the match: what waited for its tick to be final is done now,
    /// and the world is this machine's own again.
    pub fn leave_match(&mut self) {
        let Some(net) = self.net.take() else {
            return;
        };
        for rec in &net.waiting {
            self.external(rec);
        }
    }

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

    /// Keep the local player's changes for the match ([`Netplay::frame`]
    /// takes them).
    pub(crate) fn net_local(&mut self, changes: Vec<(String, ActionValue)>) {
        if let Some(n) = self.net.as_mut() {
            n.local.extend(changes);
        }
    }

    /// The local player's changes since the last call.
    pub fn take_net_local(&mut self) -> Vec<(String, ActionValue)> {
        self.net
            .as_mut()
            .map(|n| std::mem::take(&mut n.local))
            .unwrap_or_default()
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

/// A host's side of multiplayer: the room on the relay, then the match on
/// the player. Each frame the host hands over the game's asks
/// ([`Player::take_net_asks`]) and the local player's input, and runs
/// [`Netplay::frame`]; when [`Netplay::poll`] gives a start, the host opens
/// the player afresh on its scene and seed and calls [`Netplay::begin`].
pub struct Netplay<W: Wire> {
    lobby: Lobby<W>,
    game: Option<Match<RoomLink<W>, Player>>,
    /// Players, while the match runs.
    players: u8,
}

impl<W: Wire> Default for Netplay<W> {
    fn default() -> Netplay<W> {
        Netplay {
            lobby: Lobby::new(),
            game: None,
            players: 0,
        }
    }
}

impl<W: Wire> Netplay<W> {
    /// Nothing yet.
    pub fn new() -> Netplay<W> {
        Netplay::default()
    }

    /// Host a room through `wire` for the player's game; the match starts
    /// on `scene` (the player's when none) from `seed`.
    pub fn host(
        &mut self,
        wire: W,
        player: &Player,
        players: u8,
        scene: Option<&str>,
        seed: u64,
        delay: u8,
    ) {
        let scene = scene.unwrap_or(&player.scene).to_string();
        self.lobby
            .host(wire, players, player.fingerprint(), &scene, seed, delay);
    }

    /// Join the room with `code` through `wire`.
    pub fn join(&mut self, wire: W, player: &Player, code: &str) {
        self.lobby.join(wire, code, player.fingerprint());
    }

    /// Leave the room or the match.
    pub fn leave(&mut self, player: &mut Player) {
        if let Some(m) = self.game.as_mut() {
            m.leave();
        }
        self.game = None;
        self.lobby.leave();
        player.leave_match();
    }

    /// The relay's news: a start once every slot is taken, for the host to
    /// open the player afresh on its scene and seed.
    pub fn poll(&mut self) -> Option<Start> {
        self.lobby.poll()
    }

    /// The match begins on `player`, opened afresh for `start`. Its first
    /// world is the match's fingerprint: a player whose world began
    /// otherwise is refused.
    pub fn begin(&mut self, player: &mut Player, start: &Start) -> Result<()> {
        player.join_match();
        let mut cfg = Config::new(start.players, start.slot, player.world.column_digest());
        cfg.delay = start.delay;
        cfg.lockstep = player.sched.driving().kind == argus_sim::ClockKind::Discrete;
        let link = self.lobby.link().expect("a started lobby has its link");
        self.game = Some(Match::new(cfg, link, player)?);
        self.players = start.players;
        Ok(())
    }

    /// The local player's input this frame, from [`Player::net_devices`].
    pub fn local(&mut self, changes: &[(String, ActionValue)]) {
        if let Some(m) = self.game.as_mut() {
            m.local(changes);
        }
    }

    /// One frame: in a match, its ticks (up to `ticks`); otherwise the
    /// player runs alone. The view scripts read is brought up to date.
    pub fn frame(&mut self, player: &mut Player, ticks: u32) -> Result<Advance> {
        let mut out = Advance::default();
        match self.game.as_mut() {
            Some(m) => {
                // What the local player's devices gave since the last frame.
                m.local(&player.take_net_local());
                out = m.advance(player, ticks)?;
                let why = match m.state() {
                    State::OutOfStep(o) => Some(o.describe()),
                    State::Ended(why) => Some(why.clone()),
                    _ => None,
                };
                if let Some(why) = why {
                    self.lobby.end(&why);
                    self.game = None;
                    player.leave_match();
                }
            }
            None => {
                player.step(ticks as u64)?;
                out.ran = ticks;
            }
        }
        // A relay that closed ends the match too.
        if let (Some(m), Some(why)) = (self.game.as_mut(), self.lobby.closed()) {
            m.leave();
            self.game = None;
            player.leave_match();
            self.lobby.end(&why);
        }
        player.set_net_view(self.view());
        Ok(out)
    }

    /// Between a lobby's frames, the player running alone as its host runs
    /// it: the relay's news, and the view scripts read.
    pub fn tick_view(&mut self, player: &mut Player) {
        if self.game.is_none() {
            if let (Phase::Room | Phase::Asking, Some(why)) =
                (self.lobby.phase().clone(), self.lobby.closed())
            {
                self.lobby.end(&why);
            }
        }
        player.set_net_view(self.view());
    }

    /// Whether a match is running.
    pub fn playing(&self) -> bool {
        self.game.is_some()
    }

    /// The match, as scripts see it.
    pub fn view(&self) -> NetView {
        match self.lobby.phase() {
            Phase::Alone => NetView::alone(),
            Phase::Asking => NetView {
                state: "asking".into(),
                ..NetView::default()
            },
            Phase::Room => {
                let r = self.lobby.room().unwrap_or_default();
                NetView {
                    state: "room".into(),
                    code: r.code,
                    slot: r.slot,
                    players: r.players,
                    present: r.present,
                    why: String::new(),
                }
            }
            // The world is every player's: nothing this machine's alone.
            Phase::Playing => NetView {
                state: "playing".into(),
                players: self.players,
                ..NetView::default()
            },
            Phase::Ended(why) => NetView {
                state: "ended".into(),
                why: why.clone(),
                ..NetView::default()
            },
        }
    }

    /// The match's counts, while it runs.
    pub fn stats(&self) -> Option<argus_net::Stats> {
        self.game.as_ref().map(|m| m.stats())
    }

    /// The match's state, while it runs.
    pub fn state(&self) -> Option<&State> {
        self.game.as_ref().map(|m| m.state())
    }

    /// The room, once there is one.
    pub fn room(&self) -> Option<argus_net::Room> {
        self.lobby.room()
    }
}
