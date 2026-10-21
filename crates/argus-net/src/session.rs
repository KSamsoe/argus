//! A match (docs/multiplayer-checklist.md item 1): two to four players,
//! each owning a slot, whose actions go to the others every tick.
//!
//! **Real time.** A tick runs at once with what is known: a player whose
//! input for it has not arrived is predicted to have changed nothing (their
//! last values hold). When their input arrives and a predicted tick was
//! wrong, the game goes back to the snapshot it kept of that tick and runs
//! the ticks again with what is now known, before the frame is drawn. It
//! keeps snapshots of the last [`Config::window`] ticks, and waits rather
//! than predict further ahead of the slowest player. Each player's own
//! input is delayed by [`Config::delay`] ticks, so on a link quicker than
//! that nothing is ever predicted.
//!
//! **Turns.** A turn-based game is played in lockstep: a turn runs when
//! every player's command for it is in. A player with nothing to do sends
//! an empty one as soon as another player has sent theirs, so one player's
//! press moves the game for everyone, as it does alone.
//!
//! **Out of step.** At the end of every [`Config::check_every`]th tick,
//! once every player's input for it is known, each player sends their
//! world's [`Peer::digest`]. When two differ, the match stops, and the
//! players trade hashes and then rows of the entities that differ, so each
//! one's [`OutOfStep`] names the tick and the fields.

use crate::link::Link;
use crate::wire::{Frame, Msg, VERSION};
use argus_ecs::{ArgusError, Result};
use argus_input::ActionValue;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// A slot's action changing at a tick: slot, action, value.
pub type Change = (u8, String, ActionValue);

/// The game a match plays: one player's world, stepped a tick at a time.
pub trait Peer {
    /// A snapshot of everything a tick changes.
    type Saved;
    /// The next tick to run on the game's driving clock.
    fn tick(&self) -> u64;
    /// The action map's actions in its order (by name), which the wire
    /// numbers them by.
    fn actions(&self) -> Vec<String>;
    /// A snapshot of now.
    fn save(&self) -> Self::Saved;
    /// Back to a snapshot.
    fn load(&mut self, saved: &Self::Saved);
    /// Whether these changes are a command, which takes a turn in a
    /// turn-based game: a press, not a release or a pointer moving.
    fn takes_turn(&self, changes: &[(String, ActionValue)]) -> bool {
        changes.iter().any(|(_, v)| match v {
            ActionValue::Button(b) => *b,
            ActionValue::Axis(x) => *x != 0.0,
            ActionValue::Axis2(x, y) => *x != 0.0 || *y != 0.0,
        })
    }
    /// Run one tick, `changes` applied as it begins. `again`: a tick run
    /// before, run again after a rollback, whose sounds have been heard.
    fn step(&mut self, changes: &[Change], again: bool) -> Result<()>;
    /// A rollback to `tick` begins: the ticks from it run again.
    fn rolled_back(&mut self, _tick: u64) {}
    /// The ticks run again after a rollback are done.
    fn caught_up(&mut self) {}
    /// Ticks before `tick` are final: every player's input for them is in,
    /// and they ran with it. What a tick does outside the game (a save, a
    /// message to the page, an achievement) is safe to do.
    fn confirmed(&mut self, _tick: u64) {}
    /// A digest of the world now, cheap enough to take every second.
    fn digest(&self) -> u64;
    /// A snapshot's world, a line a field (`e:12 Transform.pos = ...`).
    fn rows_of(&self, saved: &Self::Saved) -> Vec<String>;
}

/// How a match is played. Every player must agree on all but `slot`.
#[derive(Clone, Debug)]
pub struct Config {
    /// Players, 2 to 4; their slots are 1 to `players`.
    pub players: u8,
    /// This player's slot.
    pub slot: u8,
    /// Ticks each player's input is delayed (real time).
    pub delay: u8,
    /// Ticks a player may run ahead of the slowest player's input.
    pub window: u8,
    /// Ticks between digest checks.
    pub check_every: u64,
    /// The game's fingerprint: a player whose differs is refused.
    pub game: u64,
    /// Lockstep, for a turn-based game.
    pub lockstep: bool,
}

impl Config {
    /// A real-time match of `players`, this one in `slot`: 2 ticks of
    /// delay, a window of 8, a check every 60 ticks.
    pub fn new(players: u8, slot: u8, game: u64) -> Config {
        Config {
            players,
            slot,
            delay: 2,
            window: 8,
            check_every: 60,
            game,
            lockstep: false,
        }
    }
}

/// A field that differs between two players' worlds.
#[derive(Clone, Debug, PartialEq)]
pub struct Difference {
    /// The other player.
    pub slot: u8,
    /// `e:12 Transform.pos`.
    pub field: String,
    /// The value here (`None`: this world has no such field).
    pub here: Option<String>,
    /// The value in the other player's world.
    pub there: Option<String>,
}

/// Two players' worlds differed.
#[derive(Clone, Debug, PartialEq)]
pub struct OutOfStep {
    /// The tick at whose end the digests differed.
    pub tick: u64,
    /// The players whose digest differed from this one's.
    pub with: Vec<u8>,
    /// The fields that differ, once the other player's rows have come.
    pub differences: Vec<Difference>,
}

impl OutOfStep {
    /// In words, for a report's note: the tick, and each field's two values.
    pub fn describe(&self) -> String {
        let with: Vec<String> = self.with.iter().map(|s| format!("slot {s}")).collect();
        let mut out = format!(
            "out of step at the end of tick {} with {}",
            self.tick,
            with.join(" and ")
        );
        if self.differences.is_empty() {
            out.push_str(" (which fields was not found)");
        }
        for d in &self.differences {
            let v = |x: &Option<String>| x.clone().unwrap_or_else(|| "nothing".into());
            out.push_str(&format!(
                "\n{}: {} here, {} in slot {}'s",
                d.field,
                v(&d.here),
                v(&d.there),
                d.slot
            ));
        }
        out
    }
}

/// Where a match is.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// Waiting for every player to say hello.
    Waiting,
    /// Playing.
    Running,
    /// Stopped: two worlds differed.
    OutOfStep(OutOfStep),
    /// Over: a player left, or could not play.
    Ended(String),
}

/// What one [`Match::advance`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Advance {
    /// Ticks run forward.
    pub ran: u32,
    /// Ticks run again after a rollback.
    pub again: u32,
    /// It stopped short: waiting for another player's input.
    pub waiting: bool,
}

/// Counts over the match, for a test or `q:net()`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    /// Rollbacks.
    pub rollbacks: u64,
    /// Ticks run again in all.
    pub again: u64,
    /// The most ticks one rollback ran again.
    pub deepest: u64,
    /// Calls to advance that waited.
    pub waits: u64,
    /// Checks every player agreed on.
    pub checks: u64,
}

#[derive(Default)]
struct Remote {
    /// Its frames by tick, those not yet pruned.
    frames: BTreeMap<u64, Vec<Change>>,
    /// The first tick whose frame is not in (every one before is).
    known_to: u64,
    /// The first tick of this player's own input it does not have.
    has_mine_to: u64,
    hello: bool,
    /// Its inputs have come: it is playing, so it heard this player's hello.
    heard: bool,
}

/// One player's side of a match.
pub struct Match<L: Link, P: Peer> {
    cfg: Config,
    link: L,
    actions: Vec<String>,
    index: HashMap<String, u16>,
    state: State,
    /// This player's frames by tick (only ticks with changes).
    local: BTreeMap<u64, Vec<(String, ActionValue)>>,
    /// The first tick this player's input is not fixed for.
    local_to: u64,
    /// Changes since the last fixed frame.
    pending: Vec<(String, ActionValue)>,
    remotes: BTreeMap<u8, Remote>,
    /// Start-of-tick snapshots of ticks some input was predicted for.
    saved: BTreeMap<u64, P::Saved>,
    /// Ticks run with a slot's input predicted.
    guessed: BTreeSet<(u64, u8)>,
    /// The earliest tick a late input showed was predicted wrong.
    wrong_from: Option<u64>,
    /// Ticks before this are final.
    final_to: u64,
    digests: BTreeMap<u64, u64>,
    at_check: BTreeMap<u64, P::Saved>,
    sent_checks: BTreeSet<u64>,
    theirs: BTreeMap<u64, BTreeMap<u8, u64>>,
    /// Hashes another player sent for a tick this one has not reached.
    asked: BTreeMap<u64, Vec<(u8, EntityHashes)>>,
    stats: Stats,
}

/// Entities (`e:12`) and the hashes of their rows.
type EntityHashes = Vec<(String, u64)>;

/// Each entity's rows, by the entity (`e:12`), for the out-of-step trade.
fn by_entity(rows: &[String]) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for r in rows {
        let e = r.split(' ').next().unwrap_or("").to_string();
        out.entry(e).or_default().push(r.clone());
    }
    out
}

fn hash_rows(rows: &[String]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for r in rows {
        for b in r.as_bytes().iter().chain(b"\n") {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// A row as the field and its value.
fn split_row(r: &str) -> (String, String) {
    match r.split_once(" = ") {
        Some((k, v)) => (k.to_string(), v.to_string()),
        None => (r.to_string(), String::new()),
    }
}

impl<L: Link, P: Peer> Match<L, P> {
    /// A match for `peer`'s game over `link`. It starts once every player
    /// has said hello; [`Match::advance`] says it.
    pub fn new(cfg: Config, link: L, peer: &P) -> Result<Match<L, P>> {
        if !(2..=4).contains(&cfg.players) || cfg.slot == 0 || cfg.slot > cfg.players {
            return Err(ArgusError::new(
                crate::codes::BAD_CONFIG,
                format!(
                    "a match is 2 to 4 players, each in a slot from 1; got slot {} of {}",
                    cfg.slot, cfg.players
                ),
            ));
        }
        let actions = peer.actions();
        let index = actions
            .iter()
            .enumerate()
            .map(|(i, a)| (a.clone(), i as u16))
            .collect();
        // Lockstep has no delay: a turn waits for every command anyway.
        let delay = if cfg.lockstep { 0 } else { cfg.delay as u64 };
        let start = peer.tick() + delay;
        let remotes = (1..=cfg.players)
            .filter(|s| *s != cfg.slot)
            .map(|s| {
                (
                    s,
                    Remote {
                        known_to: start,
                        has_mine_to: start,
                        ..Remote::default()
                    },
                )
            })
            .collect();
        Ok(Match {
            local_to: start,
            final_to: peer.tick(),
            cfg,
            link,
            actions,
            index,
            state: State::Waiting,
            local: BTreeMap::new(),
            pending: Vec::new(),
            remotes,
            saved: BTreeMap::new(),
            guessed: BTreeSet::new(),
            wrong_from: None,
            digests: BTreeMap::new(),
            at_check: BTreeMap::new(),
            sent_checks: BTreeSet::new(),
            theirs: BTreeMap::new(),
            asked: BTreeMap::new(),
            stats: Stats::default(),
        })
    }

    /// Where the match is.
    pub fn state(&self) -> &State {
        &self.state
    }

    /// Counts so far.
    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// The config.
    pub fn config(&self) -> &Config {
        &self.cfg
    }

    /// Ticks before this are final on this player's machine.
    pub fn final_to(&self) -> u64 {
        self.final_to
    }

    /// The link, to reach the relay or the test's network behind it.
    pub fn link_mut(&mut self) -> &mut L {
        &mut self.link
    }

    /// This player's actions that changed since the last call: what its
    /// devices give the actions now. In real time a later change of an
    /// action replaces an earlier one not yet sent (one value an action a
    /// tick); in turns every change is kept in order, since a release and
    /// the next press may go in one turn.
    pub fn local(&mut self, changes: &[(String, ActionValue)]) {
        for (a, v) in changes {
            if !self.index.contains_key(a) {
                continue;
            }
            if !self.cfg.lockstep {
                self.pending.retain(|(b, _)| b != a);
            }
            self.pending.push((a.clone(), *v));
        }
    }

    /// Leave the match, telling the others.
    pub fn leave(&mut self) {
        self.link.send(
            Msg::Bye {
                from: self.cfg.slot,
            }
            .encode(),
        );
        if matches!(self.state, State::Waiting | State::Running) {
            self.state = State::Ended("left the match".into());
        }
    }

    /// Take what arrived, roll back if it showed a prediction wrong, and
    /// run up to `ticks` ticks. A real-time game asks for the ticks its
    /// clock owes; a turn-based one for as many turns as are ready.
    pub fn advance(&mut self, peer: &mut P, ticks: u32) -> Result<Advance> {
        let mut out = Advance::default();
        self.receive(peer);
        if self.state == State::Waiting {
            self.link.send(self.hello().encode());
            out.waiting = true;
            return Ok(out);
        }
        if self.state != State::Running {
            return Ok(out);
        }
        if let Some(from) = self.wrong_from.take() {
            out.again = self.roll_back(peer, from)?;
        }
        for _ in 0..ticks {
            if !self.ready(peer) {
                out.waiting = true;
                break;
            }
            self.run(peer, false)?;
            out.ran += 1;
        }
        if out.waiting {
            self.stats.waits += 1;
        }
        self.send_inputs();
        self.confirm(peer);
        Ok(out)
    }

    fn hello(&self) -> Msg {
        Msg::Hello {
            version: VERSION,
            game: self.cfg.game,
            slot: self.cfg.slot,
            players: self.cfg.players,
            delay: if self.cfg.lockstep { 0 } else { self.cfg.delay },
        }
    }

    /// The tick of the slowest player's input: the first tick some
    /// player's input is not in for.
    fn slowest(&self) -> u64 {
        self.remotes
            .values()
            .map(|r| r.known_to)
            .min()
            .unwrap_or(u64::MAX)
    }

    /// May the next tick run? Fixes this player's own input for it first.
    fn ready(&mut self, peer: &P) -> bool {
        let now = peer.tick();
        if self.cfg.lockstep {
            // A command takes a turn; without one this player answers with
            // what it has (an empty command) once another is waiting on it.
            let wanted = self.remotes.values().map(|r| r.known_to).max().unwrap_or(0);
            if self.local_to <= now && (peer.takes_turn(&self.pending) || wanted > self.local_to) {
                self.fix_local();
            }
            return self.local_to > now && self.slowest() > now;
        }
        if now >= self.slowest() + self.cfg.window as u64 {
            return false;
        }
        while self.local_to <= now + self.cfg.delay as u64 {
            self.fix_local();
        }
        true
    }

    fn fix_local(&mut self) {
        let f = std::mem::take(&mut self.pending);
        if !f.is_empty() {
            self.local.insert(self.local_to, f);
        }
        self.local_to += 1;
    }

    /// Every slot's changes at `tick`, predicted (nothing) where not known.
    fn inputs(&mut self, tick: u64) -> Vec<Change> {
        let me = self.cfg.slot;
        let mut out: Vec<Change> = self
            .local
            .get(&tick)
            .map(|f| f.iter().map(|(a, v)| (me, a.clone(), *v)).collect())
            .unwrap_or_default();
        for (s, r) in &self.remotes {
            match r.frames.get(&tick) {
                Some(f) => out.extend(f.iter().cloned()),
                None if tick >= r.known_to => {
                    self.guessed.insert((tick, *s));
                }
                None => {}
            }
        }
        out.sort_by_key(|c| c.0);
        out
    }

    /// Run the next tick: a snapshot first if any input of it is a guess.
    fn run(&mut self, peer: &mut P, again: bool) -> Result<()> {
        let now = peer.tick();
        if now >= self.slowest() {
            self.saved.insert(now, peer.save());
        } else {
            self.saved.remove(&now);
        }
        let inputs = self.inputs(now);
        peer.step(&inputs, again)?;
        if (now + 1).is_multiple_of(self.cfg.check_every) {
            self.digests.insert(now, peer.digest());
            self.at_check.insert(now, peer.save());
        }
        Ok(())
    }

    fn roll_back(&mut self, peer: &mut P, from: u64) -> Result<u32> {
        let to = peer.tick();
        if from >= to {
            return Ok(0);
        }
        let Some(s) = self.saved.remove(&from) else {
            return Err(ArgusError::new(
                crate::codes::NO_SNAPSHOT,
                format!("no snapshot of tick {from} to go back to (now {to})"),
            ));
        };
        peer.load(&s);
        peer.rolled_back(from);
        self.guessed.retain(|(t, _)| *t < from);
        let mut n = 0;
        while peer.tick() < to {
            self.run(peer, true)?;
            n += 1;
        }
        peer.caught_up();
        self.stats.rollbacks += 1;
        self.stats.again += n as u64;
        self.stats.deepest = self.stats.deepest.max(n as u64);
        Ok(n)
    }

    fn send_inputs(&mut self) {
        let first = self
            .remotes
            .values()
            .map(|r| r.has_mine_to)
            .min()
            .unwrap_or(self.local_to);
        // A player far behind gets the rest in later messages.
        let last = self.local_to.min(first + 256);
        let frames: Vec<Frame> = (first..last)
            .map(|t| {
                self.local
                    .get(&t)
                    .map(|f| f.iter().map(|(a, v)| (self.index[a], *v)).collect())
                    .unwrap_or_default()
            })
            .collect();
        let acks = self.remotes.iter().map(|(s, r)| (*s, r.known_to)).collect();
        self.link.send(
            Msg::Inputs {
                from: self.cfg.slot,
                acks,
                first,
                frames,
            }
            .encode(),
        );
    }

    fn receive(&mut self, peer: &P) {
        for bytes in self.link.recv() {
            let Ok(m) = Msg::decode(&bytes) else {
                continue;
            };
            self.take(m, peer);
        }
    }

    fn take(&mut self, m: Msg, peer: &P) {
        let me = self.cfg.slot;
        match m {
            Msg::Hello {
                version,
                game,
                slot,
                players,
                delay,
            } => {
                let ours = if self.cfg.lockstep { 0 } else { self.cfg.delay };
                let why = if version != VERSION {
                    Some(format!(
                        "slot {slot} speaks match version {version}, this one {VERSION}"
                    ))
                } else if game != self.cfg.game {
                    Some(format!(
                        "slot {slot} plays another game or another build of it"
                    ))
                } else if players != self.cfg.players || delay != ours {
                    Some(format!(
                        "slot {slot} has {players} players and delay {delay}, this one {} and {ours}",
                        self.cfg.players
                    ))
                } else {
                    None
                };
                if let Some(why) = why {
                    if matches!(self.state, State::Waiting | State::Running) {
                        // So it refuses this one too.
                        self.link.send(self.hello().encode());
                        self.state = State::Ended(why);
                    }
                    return;
                }
                let Some(r) = self.remotes.get_mut(&slot) else {
                    return;
                };
                r.hello = true;
                let heard = r.heard;
                if self.state == State::Running && !heard {
                    // It is still waiting, so it has not heard ours.
                    self.link.send(self.hello().encode());
                } else if self.state == State::Waiting && self.remotes.values().all(|r| r.hello) {
                    self.state = State::Running;
                }
            }
            Msg::Inputs {
                from,
                acks,
                first,
                frames,
            } => {
                if from == me {
                    return;
                }
                let now = peer.tick();
                let actions = &self.actions;
                let Some(r) = self.remotes.get_mut(&from) else {
                    return;
                };
                // Inputs from a player means it heard every hello.
                r.hello = true;
                r.heard = true;
                if let Some((_, t)) = acks.iter().find(|(s, _)| *s == me) {
                    r.has_mine_to = r.has_mine_to.max(*t);
                }
                for (i, f) in frames.into_iter().enumerate() {
                    let t = first + i as u64;
                    if t < r.known_to || r.frames.contains_key(&t) {
                        continue;
                    }
                    let changes: Vec<Change> = f
                        .into_iter()
                        .filter_map(|(a, v)| actions.get(a as usize).map(|a| (from, a.clone(), v)))
                        .collect();
                    if self.guessed.remove(&(t, from)) && !changes.is_empty() && t < now {
                        self.wrong_from = Some(self.wrong_from.map_or(t, |w| w.min(t)));
                    }
                    r.frames.insert(t, changes);
                }
                while r.frames.contains_key(&r.known_to) {
                    r.known_to += 1;
                }
                if self.state == State::Waiting && self.remotes.values().all(|r| r.hello) {
                    self.state = State::Running;
                }
            }
            Msg::Check { from, tick, digest } => {
                self.theirs.entry(tick).or_default().insert(from, digest);
                self.compare(peer);
            }
            Msg::Hashes {
                from,
                tick,
                entities,
            } => self.hashes(peer, from, tick, entities),
            Msg::Rows { from, tick, rows } => self.rows(peer, from, tick, rows),
            Msg::Bye { from } => {
                if matches!(self.state, State::Waiting | State::Running) {
                    self.state = State::Ended(format!("slot {from} left the match"));
                }
            }
            // The lobby's: a match has started by now.
            Msg::Start { .. } => {}
        }
    }

    /// Ticks every input is in for and that ran with it are final.
    fn confirm(&mut self, peer: &mut P) {
        let to = self.slowest().min(peer.tick());
        if to > self.final_to {
            self.final_to = to;
            peer.confirmed(to);
        }
        // A snapshot or a guess is wanted only where an input may still
        // come; a frame until its tick is final (it may not have run yet).
        let slowest = self.slowest();
        let keep = self.final_to;
        self.saved.retain(|t, _| *t >= slowest);
        self.guessed.retain(|(t, _)| *t >= slowest);
        let keep_mine = self
            .remotes
            .values()
            .map(|r| r.has_mine_to)
            .min()
            .unwrap_or(0)
            .min(keep);
        self.local.retain(|t, _| *t >= keep_mine);
        for r in self.remotes.values_mut() {
            r.frames.retain(|t, _| *t >= keep);
        }
        let due: Vec<(u64, u64)> = self
            .digests
            .range(..self.final_to)
            .filter(|(t, _)| !self.sent_checks.contains(t))
            .map(|(t, d)| (*t, *d))
            .collect();
        for (tick, digest) in due {
            self.sent_checks.insert(tick);
            self.link.send(
                Msg::Check {
                    from: self.cfg.slot,
                    tick,
                    digest,
                }
                .encode(),
            );
        }
        self.compare(peer);
    }

    /// Every check this player and all the others have sent, compared.
    fn compare(&mut self, peer: &P) {
        if self.state != State::Running {
            return;
        }
        let others = self.cfg.players as usize - 1;
        let ready: Vec<u64> = self
            .theirs
            .iter()
            .filter(|(t, m)| m.len() == others && self.sent_checks.contains(t))
            .map(|(t, _)| *t)
            .collect();
        for tick in ready {
            let mine = self.digests[&tick];
            let with: Vec<u8> = self.theirs[&tick]
                .iter()
                .filter(|(_, d)| **d != mine)
                .map(|(s, _)| *s)
                .collect();
            self.theirs.remove(&tick);
            if with.is_empty() {
                self.stats.checks += 1;
                // The state at a check is kept until it is agreed.
                self.digests.retain(|t, _| *t > tick);
                self.at_check.retain(|t, _| *t > tick);
                self.sent_checks.retain(|t| *t > tick);
                continue;
            }
            self.out_of_step(peer, tick, with);
            return;
        }
    }

    fn out_of_step(&mut self, peer: &P, tick: u64, with: Vec<u8>) {
        self.state = State::OutOfStep(OutOfStep {
            tick,
            with,
            differences: Vec::new(),
        });
        let rows = self
            .at_check
            .get(&tick)
            .map(|s| peer.rows_of(s))
            .unwrap_or_default();
        let entities = by_entity(&rows)
            .into_iter()
            .map(|(e, r)| (e, hash_rows(&r)))
            .collect();
        self.link.send(
            Msg::Hashes {
                from: self.cfg.slot,
                tick,
                entities,
            }
            .encode(),
        );
        for (from, theirs) in self.asked.remove(&tick).unwrap_or_default() {
            self.hashes(peer, from, tick, theirs);
        }
    }

    /// Another player's hashes: send the rows of the entities that differ.
    fn hashes(&mut self, peer: &P, from: u8, tick: u64, theirs: Vec<(String, u64)>) {
        let reached = self.at_check.contains_key(&tick) && self.sent_checks.contains(&tick);
        if !reached {
            // Answered when this player reaches the tick and finds it too.
            self.asked.entry(tick).or_default().push((from, theirs));
            return;
        }
        if self.state == State::Running {
            // It found the difference first; this answer follows ours.
            self.asked.entry(tick).or_default().push((from, theirs));
            self.out_of_step(peer, tick, vec![from]);
            return;
        }
        let Some(s) = self.at_check.get(&tick) else {
            return;
        };
        let mine = by_entity(&peer.rows_of(s));
        let theirs: BTreeMap<String, u64> = theirs.into_iter().collect();
        let mut rows = Vec::new();
        for (e, r) in &mine {
            if theirs.get(e) != Some(&hash_rows(r)) {
                rows.extend(r.iter().cloned());
            }
        }
        // An entity only it has: an empty answer still names it.
        for e in theirs.keys() {
            if !mine.contains_key(e) {
                rows.push(format!("{e} (none)"));
            }
        }
        self.link.send(
            Msg::Rows {
                from: self.cfg.slot,
                tick,
                rows,
            }
            .encode(),
        );
    }

    /// Another player's rows of the entities that differ: name the fields.
    fn rows(&mut self, peer: &P, from: u8, tick: u64, theirs: Vec<String>) {
        let State::OutOfStep(o) = &self.state else {
            return;
        };
        if o.tick != tick {
            return;
        }
        let Some(s) = self.at_check.get(&tick) else {
            return;
        };
        let mine = by_entity(&peer.rows_of(s));
        let theirs_by = by_entity(&theirs);
        let mut diffs = Vec::new();
        for (e, their_rows) in &theirs_by {
            let here: BTreeMap<String, String> = mine
                .get(e)
                .map(|r| r.iter().map(|x| split_row(x)).collect())
                .unwrap_or_default();
            let there: BTreeMap<String, String> = their_rows
                .iter()
                .filter(|r| !r.ends_with(" (none)"))
                .map(|x| split_row(x))
                .collect();
            let keys: BTreeSet<&String> = here.keys().chain(there.keys()).collect();
            for k in keys {
                let (a, b) = (here.get(k), there.get(k));
                if a != b {
                    diffs.push(Difference {
                        slot: from,
                        field: k.clone(),
                        here: a.cloned(),
                        there: b.cloned(),
                    });
                }
            }
        }
        if let State::OutOfStep(o) = &mut self.state {
            o.differences.extend(diffs);
        }
    }
}
