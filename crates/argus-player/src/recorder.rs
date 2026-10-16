//! The recorder a release build keeps (§15, phase 2 item 7): "the last
//! thirty seconds of actions and a keyframe, so that a player pressing a
//! report key or hitting a crash produces a replay file the team can open".
//!
//! A keyframe is taken every thirty seconds of sim time (thirty turns on a
//! discrete clock) and the two newest are kept, with the input commands
//! since the older one; the rest of the input journal and log is forgotten.
//! So memory stays bounded however long the game runs, and a report always
//! reaches back between thirty and sixty seconds, or turns (all of it,
//! early on). On a discrete clock the turns are the ticks whose systems
//! ran: the wheel's, a typed character's, and a menu's ticks are frozen and
//! take none, so a player reading back through a log before reporting
//! does not push the turn they meant out of the report (issue 36). A
//! report is a replay file (`crate::replay`): the older keyframe, the
//! commands after it, and the world now, which `argus mark import` turns
//! into a mark the agent opens like any other.

use crate::keyframe::{Keyframe, Sources};
use argus_ecs::World;
use argus_sim::{ClockKind, Scheduler};
use serde_json::{json, Value as Json};
use std::collections::VecDeque;

/// Seconds between keyframes on a continuous clock.
pub const WINDOW_SECONDS: f64 = 30.0;
/// Turns between keyframes on a discrete clock.
pub const WINDOW_TURNS: u64 = 30;
/// Ticks between keyframes at most on a discrete clock, however few were
/// turns, so a long stretch of scrolling or typing keeps memory bounded.
pub const MOST_TICKS: u64 = 3000;

/// Two keyframes and what came after the older.
pub struct Recorder {
    /// Ticks (a continuous clock) or turns (a discrete one) between
    /// keyframes.
    pub every: u64,
    keys: VecDeque<Keyframe>,
}

impl Recorder {
    /// A recorder for a scheduler's driving clock, with its first keyframe
    /// taken now.
    pub fn new(world: &World, sched: &Scheduler, sources: &Sources) -> Recorder {
        let d = sched.driving();
        let every = match d.kind {
            ClockKind::Continuous => ((d.hz() * WINDOW_SECONDS).round() as u64).max(1),
            ClockKind::Discrete => WINDOW_TURNS,
        };
        let mut r = Recorder {
            every,
            keys: VecDeque::new(),
        };
        r.keys.push_back(sources.capture(world, sched, true));
        r
    }

    /// After ticks ran: a keyframe when one is due, and the oldest one, with
    /// the input before it, forgotten.
    pub fn after_ticks(&mut self, world: &World, sched: &Scheduler, sources: &Sources) {
        let last = self.keys.back().map_or(0, |k| k.tick);
        let due = match sched.driving().kind {
            ClockKind::Continuous => sched.tick() >= last + self.every,
            ClockKind::Discrete => {
                sched.turns_since(last) as u64 >= self.every || sched.tick() >= last + MOST_TICKS
            }
        };
        if !due {
            return;
        }
        self.keys.push_back(sources.capture(world, sched, false));
        while self.keys.len() > 2 {
            self.keys.pop_front();
        }
        let (Some(input), Some(first)) = (&sources.input, self.keys.front()) else {
            return;
        };
        let Some(n) = first.input.as_ref().map(|s| s.cursor()) else {
            return;
        };
        let n = input.borrow_mut().forget(n, first.tick);
        for k in self.keys.iter_mut() {
            if let Some(s) = k.input.as_mut() {
                s.forget(n);
            }
        }
    }

    /// The tick the next report starts from.
    pub fn from(&self) -> u64 {
        self.keys.front().map_or(0, |k| k.tick)
    }

    /// Keyframes held (one or two).
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Always false: a recorder holds its first keyframe from the start.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// A replay file of what the recorder holds: `meta` is the run's
    /// identity (scene, seed, code, clock, note, kind); `from` and `to` are
    /// filled in. `end` is the world now, which a reader compares with; a
    /// crash report leaves it out, since the world may be half a tick on.
    pub fn report(
        &self,
        mut meta: Json,
        sched: &Scheduler,
        sources: &Sources,
        end: Option<&World>,
    ) -> Vec<u8> {
        let k = self.keys.front().expect("a recorder holds a keyframe");
        let base = k.input.as_ref().map_or(0, |s| s.cursor());
        let journal = sources
            .input
            .as_ref()
            .map(|i| i.borrow().journal().get(base..).unwrap_or(&[]).to_vec())
            .unwrap_or_default();
        meta["from"] = json!(k.tick);
        meta["to"] = json!(sched.tick());
        meta["clock"] = json!(sched.driving().to_string());
        let cells = match (end, &sources.tiles) {
            (Some(_), Some(t)) => t.borrow().runtime_cells(),
            _ => Vec::new(),
        };
        crate::replay::encode(&meta, k, base, &journal, end, &cells)
    }
}
