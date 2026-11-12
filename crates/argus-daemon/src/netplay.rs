//! Multiplayer in the daemon (docs/multiplayer-checklist.md item 3).
//!
//! The daemon's world is an [`argus_net::Peer`]: a match runs it a tick at a
//! time, snapshots it, and puts it back to run ticks again quietly, with the
//! event log, history, and keyframe ring cut back to the tick.
//!
//! `net.sim` plays a whole match in one daemon, as a scenario's `t:net`
//! does: the other players are forks of the world (`net.2`, ...), and every
//! player's messages go through a simulated network, as bad as asked. While
//! it lasts, stepping runs every player's world; an inject goes to its
//! slot's player, through the match; and a step ends with every player at
//! the same tick, every tick final, and every world the same, or an error
//! that names the fields that differ.
//!
//! `net.host` and `net.join` play one side of a real match through a relay,
//! so an agent can play against a person: the daemon's frames run the match
//! while it lasts, its injects and its window's devices are its side's
//! input, and the world is both players' to look at.

use super::*;
use argus_input::ActionValue;
use argus_net::{Change, Conditions, Config, Match, SimLink, SimNet, State};
use argus_player::keyframe::Keyframe;

/// A whole match in one daemon (`net.sim`).
pub(super) struct NetSim {
    net: SimNet,
    /// Milliseconds of network time a frame (one tick).
    frame_ms: f64,
    /// Each player's world (`main` for slot 1) and its match.
    players: Vec<(String, Match<SimLink, Daemon>)>,
    /// What each slot's injects asked since the last frame.
    pending: Vec<Vec<(String, ActionValue)>>,
    /// Holds that end: slot, the main world's tick, action, its rest value.
    releases: Vec<(u8, u64, String, ActionValue)>,
}

impl argus_net::Peer for Daemon {
    type Saved = Keyframe;

    fn tick(&self) -> u64 {
        self.sched.tick()
    }

    fn actions(&self) -> Vec<String> {
        self.session
            .as_ref()
            .and_then(|s| s.input.as_ref())
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

    fn takes_turn(&self, changes: &[(String, ActionValue)]) -> bool {
        let Some(st) = self.session.as_ref().and_then(|s| s.input.as_ref()) else {
            return false;
        };
        let st = st.borrow();
        changes.iter().any(|(action, v)| {
            st.map().action(action).is_ok_and(|a| {
                a.kind != argus_input::ActionKind::Pointer
                    && action != argus_input::map::DEVICE_ACTION
                    && v.is_active()
            })
        })
    }

    fn save(&self) -> Keyframe {
        let ses = self.session.as_ref().expect("a match has a session");
        ses.sources.capture(&self.world, &self.sched, false)
    }

    fn load(&mut self, k: &Keyframe) {
        let Some(ses) = &self.session else {
            return;
        };
        ses.sources.restore(k, &mut self.world, &mut self.sched);
        let _ = self.world.drain_events();
        if let Some(i) = &ses.input {
            i.borrow_mut().rewind_to(k.tick);
        }
        // What the ticks from here did the first time did not happen.
        ses.ring.borrow_mut().prune_after(k.tick);
        ses.history.borrow_mut().rewind(k.tick);
        self.cut_log(k.tick);
    }

    fn step(&mut self, changes: &[Change], again: bool) -> Result<()> {
        if let Some(i) = self.session.as_ref().and_then(|s| s.input.clone()) {
            i.borrow_mut().apply_changes(self.sched.tick(), changes);
        }
        // A tick run again is quiet: no history, keyframes, sound, or
        // saves of its own; its events replace the first run's in the log.
        self.sched.set_quiet(again);
        let was = self.backstage;
        self.backstage = was || again;
        let r = match self.sched.driving().kind {
            ClockKind::Continuous => self.sched.step(&mut self.world, 1),
            ClockKind::Discrete => self.sched.advance(&mut self.world, 1),
        };
        self.sched.set_quiet(false);
        if r.is_ok() {
            self.pump_ticks();
        }
        self.backstage = was;
        r
    }

    fn digest(&self) -> u64 {
        let cells = self
            .session
            .as_ref()
            .map(|s| s.tiles.borrow().runtime_cells())
            .unwrap_or_default();
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

fn net_error(msg: String) -> ArgusError {
    ArgusError::new(codes::BAD_PARAMS, msg)
}

impl Daemon {
    /// `net.sim {players, delay, latency, jitter, loss, seed}`: a whole
    /// match in this daemon, every player's world here.
    pub(super) fn net_sim(&mut self, params: &Json) -> Result<Json> {
        self.require_session()?;
        let players = json::u64_param(params, "players", 2)?;
        if !(2..=4).contains(&players) {
            return Err(bad_params(format!(
                "a match is 2 to 4 players, not {players}"
            )));
        }
        let players = players as u8;
        let game_delay = self.require_session()?.project.config.net_delay as u64;
        let delay = json::u64_param(params, "delay", game_delay)?.min(30) as u8;
        let num = |k: &str, d: f64| params.get(k).and_then(Json::as_f64).unwrap_or(d);
        let cond = Conditions {
            delay_ms: num("latency", 30.0).max(0.0),
            jitter_ms: num("jitter", 0.0).max(0.0),
            loss: num("loss", 0.0).clamp(0.0, 0.9),
        };
        let seed = json::u64_param(params, "seed", 1)?;
        if self.active != "main" {
            return Err(bad_params(
                "a match starts from the main world: switch back first",
            ));
        }
        // A match before replaces it, with its worlds.
        if let Some(old) = self.netsim.take() {
            for (name, _) in old.players.iter().skip(1) {
                self.forks.remove(name);
            }
        }
        let mut names = vec!["main".to_string()];
        for k in 2..=players {
            let name = format!("net.{k}");
            self.forks.remove(&name);
            self.sim_fork(&json!({ "name": name }))?;
            names.push(name);
        }
        let net = SimNet::new(players as usize, cond, seed);
        let game = self.world.column_digest();
        let lockstep = self.sched.driving().kind == ClockKind::Discrete;
        let mut matches = Vec::new();
        for (i, name) in names.iter().enumerate() {
            let mut cfg = Config::new(players, i as u8 + 1, game);
            cfg.delay = delay;
            cfg.lockstep = lockstep;
            let link = net.link(i);
            let m = if i == 0 {
                Match::new(cfg, link, &*self)?
            } else {
                self.in_world(name, |d| Match::new(cfg, link, &*d))?
            };
            matches.push((name.clone(), m));
        }
        // Every player's scripts see the match from its first tick.
        let playing = argus_luau::NetView {
            state: "playing".into(),
            players,
            ..argus_luau::NetView::default()
        };
        self.set_net_view(playing.clone());
        for name in names.iter().skip(1) {
            self.in_world(name, |d| {
                d.set_net_view(playing.clone());
                Ok(())
            })?;
        }
        let frame_ms = 1000.0 / self.sched.config().hz.max(1.0);
        self.netsim = Some(NetSim {
            net,
            frame_ms,
            players: matches,
            pending: vec![Vec::new(); players as usize],
            releases: Vec::new(),
        });
        // The players' hellos, so the first step is the game's.
        for _ in 0..200 {
            self.net_frame(Some(self.sched.tick()))?;
            let ns = self.netsim.as_ref().unwrap();
            if ns.players.iter().all(|(_, m)| *m.state() == State::Running) {
                break;
            }
        }
        Ok(self.net_status())
    }

    /// `net.status`: the match, and every player's world in it.
    pub(super) fn net_status(&self) -> Json {
        let Some(ns) = &self.netsim else {
            return json!({ "state": "alone" });
        };
        let worlds: Vec<Json> = ns
            .players
            .iter()
            .enumerate()
            .map(|(i, (name, m))| {
                let tick = if i == 0 {
                    self.sched.tick()
                } else {
                    self.forks.get(name).map_or(0, |s| s.sched.tick())
                };
                let s = m.stats();
                json!({
                    "world": name,
                    "slot": i + 1,
                    "tick": tick,
                    "final_to": m.final_to(),
                    "state": match m.state() {
                        State::Waiting => "waiting".to_string(),
                        State::Running => "running".to_string(),
                        State::OutOfStep(o) => o.describe(),
                        State::Ended(why) => format!("ended: {why}"),
                    },
                    "rollbacks": s.rollbacks,
                    "again": s.again,
                    "deepest": s.deepest,
                    "checks": s.checks,
                    "waits": s.waits,
                })
            })
            .collect();
        let (sent, lost) = ns.net.counts();
        let delay = ns.players.first().map(|(_, m)| m.config().delay);
        json!({ "state": "sim", "players": ns.players.len(), "delay": delay, "worlds": worlds, "messages": sent, "lost": lost })
    }

    /// Whether a scenario's match is on: steps and injects go through it.
    pub(super) fn in_net_sim(&self) -> bool {
        self.netsim.is_some()
    }

    /// An inject in a match: the slot's player's input, through the match.
    pub(super) fn net_inject(&mut self, params: &Json) -> Result<Json> {
        let action = json::str_param(params, "action")?.to_string();
        let slot = json::u64_param(params, "slot", 1)?;
        let st = self.input()?;
        let (kind, players) = {
            let st = st.borrow();
            let a = st.map().action(&action)?;
            (a.kind, self.netsim.as_ref().map_or(0, |n| n.players.len()))
        };
        if slot == 0 || slot as usize > players {
            return Err(bad_params(format!(
                "slot {slot}: the match has slots 1 to {players}"
            )));
        }
        let value = match params.get("value") {
            Some(v) => ActionValue::from_json(kind, v, &action)?,
            None => ActionValue::active_for(kind),
        };
        let frames = params.get("frames").and_then(Json::as_u64);
        let now = self.sched.tick();
        let ns = self.netsim.as_mut().unwrap();
        ns.pending[slot as usize - 1].push((action.clone(), value));
        if let Some(n) = frames {
            ns.releases.push((
                slot as u8,
                now + n.max(1),
                action.clone(),
                ActionValue::default_for(kind),
            ));
        }
        Ok(json!({ "action": action, "slot": slot, "tick": now, "until": frames.map(|n| now + n) }))
    }

    /// Run the match until the main world has run `ticks` more, then
    /// settle; the ticks the main world ran.
    pub(super) fn net_run(&mut self, ticks: u64) -> Result<u64> {
        let start = self.sched.tick();
        let target = start + ticks;
        let mut frames = 0u64;
        while self.sched.tick() < target {
            self.net_frame(None)?;
            frames += 1;
            if frames > ticks * 50 + 1200 {
                return Err(net_error(format!(
                    "the match stopped moving at tick {}: a player waits on input that never comes",
                    self.sched.tick()
                )));
            }
        }
        self.net_settle()?;
        Ok(self.sched.tick() - start)
    }

    /// Every player at the same tick, every tick final, every world the
    /// same; an error naming what differs when not.
    fn net_settle(&mut self) -> Result<()> {
        for _ in 0..3000 {
            let ticks = self.net_ticks();
            let lead = *ticks.iter().max().unwrap_or(&0);
            let ns = self.netsim.as_ref().unwrap();
            let settled = ticks.iter().all(|t| *t == lead)
                && ns.players.iter().all(|(_, m)| m.final_to() == lead);
            if settled {
                let mine = self.world.column_digest();
                for (name, _) in ns.players.iter().skip(1) {
                    let theirs = self.forks.get(name).map(|s| s.world.column_digest());
                    if theirs != Some(mine) {
                        return Err(net_error(format!(
                            "the players disagree at tick {lead}: {name}'s world is not main's"
                        )));
                    }
                }
                return Ok(());
            }
            self.net_frame(Some(lead))?;
        }
        Err(net_error(format!(
            "the match did not settle: the players are at ticks {:?}",
            self.net_ticks()
        )))
    }

    fn net_ticks(&self) -> Vec<u64> {
        let Some(ns) = &self.netsim else {
            return Vec::new();
        };
        ns.players
            .iter()
            .enumerate()
            .map(|(i, (name, _))| {
                if i == 0 {
                    self.sched.tick()
                } else {
                    self.forks.get(name).map_or(0, |s| s.sched.tick())
                }
            })
            .collect()
    }

    /// One frame of the match: the network moves on a tick's time, and each
    /// player runs a tick (with `lead`, only one behind it does).
    fn net_frame(&mut self, lead: Option<u64>) -> Result<()> {
        let mut ns = self
            .netsim
            .take()
            .ok_or_else(|| bad_params("no match: net.sim starts one"))?;
        let r = self.net_frame_in(&mut ns, lead);
        self.netsim = Some(ns);
        r
    }

    fn net_frame_in(&mut self, ns: &mut NetSim, lead: Option<u64>) -> Result<()> {
        ns.net.advance(ns.frame_ms);
        let now = self.sched.tick();
        let mut i = 0;
        while i < ns.releases.len() {
            if ns.releases[i].1 <= now {
                let (slot, _, action, rest) = ns.releases.remove(i);
                ns.pending[slot as usize - 1].push((action, rest));
            } else {
                i += 1;
            }
        }
        let mut broken = None;
        for (i, (name, m)) in ns.players.iter_mut().enumerate() {
            let changes = std::mem::take(&mut ns.pending[i]);
            m.local(&changes);
            let mut run = |d: &mut Daemon| -> Result<()> {
                let n = lead.map_or(1, |l| (d.sched.tick() < l) as u32);
                m.advance(d, n).map(|_| ())
            };
            if i == 0 {
                run(self)?;
            } else {
                self.in_world(name, run)?;
            }
            match m.state() {
                State::OutOfStep(_) => broken = Some(i),
                State::Ended(why) => {
                    return Err(net_error(format!("slot {}'s match ended: {why}", i + 1)))
                }
                _ => {}
            }
        }
        if let Some(i) = broken {
            // The players trade rows for a few frames, to name the fields.
            for _ in 0..60 {
                ns.net.advance(ns.frame_ms);
                let mut named = true;
                for (j, (name, m)) in ns.players.iter_mut().enumerate() {
                    let mut run = |d: &mut Daemon| m.advance(d, 0).map(|_| ());
                    if j == 0 {
                        run(self)?;
                    } else {
                        self.in_world(name, run)?;
                    }
                    if let State::OutOfStep(o) = m.state() {
                        named &= !o.differences.is_empty();
                    }
                }
                if named {
                    break;
                }
            }
            let words = match ns.players[i].1.state() {
                State::OutOfStep(o) => o.describe(),
                _ => String::new(),
            };
            return Err(net_error(format!("slot {}: {words}", i + 1)));
        }
        Ok(())
    }
}

/// One side of a match through a relay (`net.host`, `net.join`): the room,
/// then the match, which the daemon's frames run while it lasts.
pub(super) struct RelayPlay {
    lobby: argus_net::Lobby<argus_relay::Client>,
    game: Option<Match<argus_net::RoomLink<argus_relay::Client>, Daemon>>,
    players: u8,
    /// This side's changes (injects, the window's devices), for the match.
    local: Vec<(String, ActionValue)>,
    /// Holds that end: the tick, action, its rest value.
    releases: Vec<(u64, String, ActionValue)>,
    /// What the window's devices last gave each action.
    devices_last: std::collections::BTreeMap<String, ActionValue>,
    /// Why the last match ended.
    ended: Option<String>,
}

impl Daemon {
    /// What scripts read with `q:net()` from the next tick on.
    pub(super) fn set_net_view(&self, v: argus_luau::NetView) {
        if let Some(s) = &self.session {
            *s.net_view.borrow_mut() = v;
        }
    }

    /// The view of a relay's room or match, for this side's scripts.
    fn relay_view(&self) -> argus_luau::NetView {
        let Some(rp) = &self.relay else {
            return argus_luau::NetView::alone();
        };
        match rp.lobby.phase() {
            argus_net::Phase::Alone => argus_luau::NetView::alone(),
            argus_net::Phase::Asking => argus_luau::NetView {
                state: "asking".into(),
                ..argus_luau::NetView::default()
            },
            argus_net::Phase::Room => {
                let r = rp.lobby.room().unwrap_or_default();
                argus_luau::NetView {
                    state: "room".into(),
                    code: r.code,
                    slot: r.slot,
                    players: r.players,
                    present: r.present,
                    why: String::new(),
                }
            }
            argus_net::Phase::Playing => argus_luau::NetView {
                state: "playing".into(),
                players: rp.players,
                ..argus_luau::NetView::default()
            },
            argus_net::Phase::Ended(why) => argus_luau::NetView {
                state: "ended".into(),
                why: why.clone(),
                ..argus_luau::NetView::default()
            },
        }
    }

    fn fingerprint(&self) -> u64 {
        argus_net::fingerprint(&self.world, &argus_net::Peer::actions(self))
    }

    fn relay_client(params: &Json) -> Result<argus_relay::Client> {
        let url = json::str_param(params, "relay")?;
        argus_relay::Client::connect(url).map_err(|e| {
            ArgusError::new(
                codes::BAD_PARAMS,
                format!("the relay at {url} did not answer: {e}"),
            )
            .hint("argus relay serves one; a game reaches it as ws://<address>:7879")
        })
    }

    /// `net.host {relay, players, scene, seed, delay}`: a room on a relay
    /// for this daemon's game; the match starts once every slot is taken.
    pub(super) fn net_host(&mut self, params: &Json) -> Result<Json> {
        self.require_session()?;
        let players = json::u64_param(params, "players", 2)?;
        if !(2..=4).contains(&players) {
            return Err(bad_params(format!(
                "a match is 2 to 4 players, not {players}"
            )));
        }
        let scene = match json::opt_str(params, "scene")? {
            Some(s) => s.to_string(),
            None => self.require_session()?.scene_name.clone(),
        };
        let seed = match params.get("seed").and_then(Json::as_u64) {
            Some(s) => s,
            None => {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(1)
                    | 1
            }
        };
        let game_delay = self.require_session()?.project.config.net_delay as u64;
        let delay = json::u64_param(params, "delay", game_delay)?.min(30) as u8;
        let c = Daemon::relay_client(params)?;
        let mut lobby = argus_net::Lobby::new();
        lobby.host(c, players as u8, self.fingerprint(), &scene, seed, delay);
        self.relay = Some(RelayPlay::new(lobby));
        self.relay_wait_room()
    }

    /// `net.join {relay, code}`.
    pub(super) fn net_join(&mut self, params: &Json) -> Result<Json> {
        self.require_session()?;
        let code = json::str_param(params, "code")?.to_string();
        let c = Daemon::relay_client(params)?;
        let mut lobby = argus_net::Lobby::new();
        lobby.join(c, &code, self.fingerprint());
        self.relay = Some(RelayPlay::new(lobby));
        self.relay_wait_room()
    }

    /// Until the relay answers (a second at most): the room, or why not.
    fn relay_wait_room(&mut self) -> Result<Json> {
        for _ in 0..200 {
            let rp = self.relay.as_mut().unwrap();
            let start = rp.lobby.poll();
            if let Some(start) = start {
                self.relay_begin(start)?;
                break;
            }
            let rp = self.relay.as_ref().unwrap();
            match rp.lobby.phase() {
                argus_net::Phase::Room => break,
                argus_net::Phase::Ended(why) => {
                    let why = why.clone();
                    self.relay = None;
                    return Err(ArgusError::new(codes::BAD_PARAMS, why));
                }
                _ => std::thread::sleep(std::time::Duration::from_millis(5)),
            }
        }
        Ok(self.relay_status())
    }

    /// The match begins: the host's scene loaded afresh from its seed.
    fn relay_begin(&mut self, start: argus_net::Start) -> Result<()> {
        let rp = self.relay.take();
        let loaded = self.scene_load(&json!({ "scene": start.scene, "seed": start.seed }));
        self.relay = rp;
        loaded?;
        let mut cfg = Config::new(start.players, start.slot, self.world.column_digest());
        cfg.delay = start.delay;
        cfg.lockstep = self.sched.driving().kind == ClockKind::Discrete;
        let link = self
            .relay
            .as_ref()
            .and_then(|r| r.lobby.link())
            .expect("a started lobby has its link");
        let m = Match::new(cfg, link, &*self)?;
        let rp = self.relay.as_mut().unwrap();
        rp.game = Some(m);
        rp.players = start.players;
        // Its scripts see the match from its first tick, as the others' do.
        self.set_net_view(argus_luau::NetView {
            state: "playing".into(),
            players: start.players,
            ..argus_luau::NetView::default()
        });
        Ok(())
    }

    /// `net.leave`.
    pub(super) fn net_leave(&mut self) -> Result<Json> {
        if let Some(mut rp) = self.relay.take() {
            if let Some(m) = rp.game.as_mut() {
                m.leave();
            }
            rp.lobby.leave();
        }
        self.set_net_view(argus_luau::NetView::alone());
        Ok(json!({ "state": "alone" }))
    }

    /// Whether this daemon plays a match through a relay now.
    pub(super) fn in_relay_match(&self) -> bool {
        self.relay.as_ref().is_some_and(|r| r.game.is_some())
    }

    /// An inject in a relay match: this side's input, through the match.
    pub(super) fn relay_inject(&mut self, params: &Json) -> Result<Json> {
        let action = json::str_param(params, "action")?.to_string();
        let st = self.input()?;
        let kind = st.borrow().map().action(&action)?.kind;
        let value = match params.get("value") {
            Some(v) => ActionValue::from_json(kind, v, &action)?,
            None => ActionValue::active_for(kind),
        };
        let frames = params.get("frames").and_then(Json::as_u64);
        let now = self.sched.tick();
        let rp = self.relay.as_mut().unwrap();
        rp.local.push((action.clone(), value));
        if let Some(n) = frames {
            rp.releases.push((
                now + n.max(1),
                action.clone(),
                ActionValue::default_for(kind),
            ));
        }
        Ok(json!({ "action": action, "tick": now, "until": frames.map(|n| now + n) }))
    }

    /// The window's devices in a relay match: slot 1's bindings (the person
    /// at this machine), the changes for the match.
    pub(super) fn relay_devices(&mut self, events: &[argus_input::DeviceInput]) {
        for ev in events {
            self.devices.apply(ev);
        }
        let Some(st) = self.session.as_ref().and_then(|s| s.input.clone()) else {
            return;
        };
        let now = self.devices.resolve(st.borrow().map(), 1);
        let rp = self.relay.as_mut().unwrap();
        for (action, v) in now {
            let rest = st
                .borrow()
                .map()
                .action(&action)
                .map(|a| ActionValue::default_for(a.kind))
                .ok();
            let before = rp.devices_last.get(&action).copied().or(rest);
            if Some(v) != before {
                rp.devices_last.insert(action.clone(), v);
                rp.local.push((action, v));
            }
        }
    }

    /// One frame of a relay match, or of its room: true when the match ran
    /// the frame (the daemon's own clock stands aside).
    pub(super) fn relay_frame(&mut self) -> Result<bool> {
        let Some(rp) = self.relay.as_mut() else {
            return Ok(false);
        };
        if rp.game.is_none() {
            if let Some(start) = rp.lobby.poll() {
                self.relay_begin(start)?;
            } else {
                self.set_net_view(self.relay_view());
            }
            return Ok(false);
        }
        let mut rp = self.relay.take().unwrap();
        let r = self.relay_frame_in(&mut rp);
        self.relay = Some(rp);
        r.map(|_| true)
    }

    fn relay_frame_in(&mut self, rp: &mut RelayPlay) -> Result<()> {
        let now = self.sched.tick();
        let mut i = 0;
        while i < rp.releases.len() {
            if rp.releases[i].0 <= now {
                let (_, a, v) = rp.releases.remove(i);
                rp.local.push((a, v));
            } else {
                i += 1;
            }
        }
        let m = rp.game.as_mut().unwrap();
        m.local(&std::mem::take(&mut rp.local));
        m.advance(self, 1)?;
        let why = match m.state() {
            State::OutOfStep(o) => Some(o.describe()),
            State::Ended(w) => Some(w.clone()),
            _ => rp.lobby.closed(),
        };
        if let Some(why) = why {
            if let Some(m) = rp.game.as_mut() {
                m.leave();
            }
            rp.game = None;
            rp.lobby.end(&why);
            rp.ended = Some(why.clone());
            self.set_net_view(argus_luau::NetView {
                state: "ended".into(),
                why,
                ..argus_luau::NetView::default()
            });
        }
        Ok(())
    }

    /// The relay match's part of `net.status`.
    pub(super) fn relay_status(&self) -> Json {
        let Some(rp) = &self.relay else {
            return json!({ "state": "alone" });
        };
        let room = rp.lobby.room();
        let state = match rp.lobby.phase() {
            argus_net::Phase::Alone => "alone".to_string(),
            argus_net::Phase::Asking => "asking".to_string(),
            argus_net::Phase::Room => "room".to_string(),
            argus_net::Phase::Playing => "playing".to_string(),
            argus_net::Phase::Ended(_) => "ended".to_string(),
        };
        let stats = rp.game.as_ref().map(|m| {
            let s = m.stats();
            json!({ "delay": m.config().delay, "final_to": m.final_to(), "rollbacks": s.rollbacks, "again": s.again, "checks": s.checks, "waits": s.waits })
        });
        json!({
            "state": state,
            "code": room.as_ref().map(|r| r.code.clone()),
            "slot": room.as_ref().map(|r| r.slot),
            "players": room.as_ref().map(|r| r.players),
            "present": room.map(|r| r.present),
            "tick": self.sched.tick(),
            "match": stats,
            "why": rp.ended,
        })
    }
}

impl RelayPlay {
    fn new(lobby: argus_net::Lobby<argus_relay::Client>) -> RelayPlay {
        RelayPlay {
            lobby,
            game: None,
            players: 0,
            local: Vec::new(),
            releases: Vec::new(),
            devices_last: Default::default(),
            ended: None,
        }
    }
}
