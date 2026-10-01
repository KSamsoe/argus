//! The keyframe ring behind `sim.seek` and `sim.fork` (§6, phase 2 item 0):
//! full snapshots at a fixed interval, thinned with age, plus the input
//! journal, so any frame of the run is "the nearest earlier keyframe,
//! replayed forward".
//!
//! A keyframe is everything the sim needs that the world does not hold: the
//! scheduler's clocks and RNG streams and which systems are off, the input
//! state, the particle pools, and which rooms are loaded. A barrier keyframe
//! is taken at a scene load and after every change made between ticks (an
//! edit, a room transition, a script reload), because those are not inputs
//! and a replay must never cross one; barriers are never thinned.

use serde_json::{json, Value as Json};
use std::sync::atomic::{AtomicU64, Ordering};

// Keyframes live with the runtime (phase 2 item 7), so a release build's
// recorder and a replay file take them the same way the ring does.
pub use argus_player::keyframe::{Keyframe, Sources};

/// Every keyframe for the last five minutes of sim time: §17's seek target
/// (any point in the last five minutes under 500 ms) cannot be met by §6's
/// "every second for the last minute", which leaves a replay of up to a
/// minute at the fast-forward rate §17 also sets (2,000 ticks a second).
pub const DENSE_SECONDS: f64 = 300.0;
/// Then one keyframe a minute up to an hour, and one per ten minutes after.
pub const HOUR: f64 = 3600.0;
/// The most keyframe and barrier ticks `sim.ring` lists; past it, evenly
/// spaced ones (a long session's barriers: every room change is one).
const REPORT_TICKS: usize = 4096;

/// Runs made in this process: a ring's `run` says which run it is, so a
/// client drawing it knows when the run it drew is gone (a scene load, a
/// replay opened, another world switched to).
static RUNS: AtomicU64 = AtomicU64::new(0);

fn next_run() -> u64 {
    RUNS.fetch_add(1, Ordering::Relaxed) + 1
}

/// At most `REPORT_TICKS` of `ticks`, evenly spaced, the first and last kept.
fn spaced(ticks: Vec<u64>) -> Vec<u64> {
    let n = ticks.len();
    if n <= REPORT_TICKS {
        return ticks;
    }
    (0..REPORT_TICKS)
        .map(|i| ticks[i * (n - 1) / (REPORT_TICKS - 1)])
        .collect()
}

/// The ring.
pub struct Ring {
    /// Ticks between keyframes.
    pub every: u64,
    /// Bytes the keyframes may hold before the ring thins further.
    pub budget: usize,
    keys: Vec<Keyframe>,
    seq: u64,
    /// The furthest tick this branch of the run has reached.
    pub end: u64,
    /// The earliest tick a seek can reach: keyframes before it went to the
    /// budget.
    pub floor: u64,
    /// The daemon's change counter at the last keyframe: a change since
    /// needs a barrier before the next tick.
    pub mutations: u64,
    /// Which run this is (see `RUNS`).
    pub run: u64,
    /// How long the last keyframes taken on a tick took, in ms, newest
    /// last: §17's "full snapshot ... invisible during play".
    captures: std::collections::VecDeque<f64>,
}

impl Ring {
    /// An empty ring.
    pub fn new(every: u64, budget: usize) -> Ring {
        Ring {
            every: every.max(1),
            budget,
            keys: Vec::new(),
            seq: 0,
            end: 0,
            floor: 0,
            mutations: 0,
            run: next_run(),
            captures: std::collections::VecDeque::new(),
        }
    }

    /// A keyframe taken on a tick took this long.
    pub fn note_capture(&mut self, d: std::time::Duration) {
        if self.captures.len() == 64 {
            self.captures.pop_front();
        }
        self.captures.push_back(d.as_secs_f64() * 1e3);
    }

    /// The last keyframe's capture and the slowest of the last 64, in ms.
    pub fn capture_ms(&self) -> Option<(f64, f64)> {
        let last = *self.captures.back()?;
        let max = self.captures.iter().copied().fold(0.0, f64::max);
        Some((last, max))
    }

    /// Keep a keyframe. A plain one at a tick that already has a keyframe on
    /// this branch is the same state again (a replayed stretch) and is
    /// dropped; a barrier at the same tick is the state after a change and
    /// is kept after the one before it.
    pub fn push(&mut self, mut k: Keyframe) {
        self.end = self.end.max(k.tick);
        if !k.barrier && self.keys.iter().any(|x| x.tick == k.tick) {
            return;
        }
        self.seq += 1;
        k.seq = self.seq;
        let at = self
            .keys
            .partition_point(|x| (x.tick, x.seq) <= (k.tick, k.seq));
        self.keys.insert(at, k);
        self.thin();
        self.fit();
    }

    /// The keyframe a seek to `tick` starts from: the latest at or before it.
    pub fn at_or_before(&self, tick: u64) -> Option<&Keyframe> {
        self.keys.iter().rev().find(|k| k.tick <= tick)
    }

    /// Where a replay of the `lead` ticks up to `tick` starts: the latest
    /// keyframe at or before `tick - lead`, or, when a change between ticks
    /// came after that, the barrier it made, since a replay never crosses
    /// one (a mark's lead-in, phase 2 item 2).
    pub fn lead_in(&self, tick: u64, lead: u64) -> Option<&Keyframe> {
        let want = tick.saturating_sub(lead);
        self.keys
            .iter()
            .rev()
            .find(|k| k.barrier && k.tick <= tick && k.tick > want)
            .or_else(|| self.at_or_before(want))
    }

    /// Start over from one keyframe (a replay file's): the run before it
    /// is another run.
    pub fn restart(&mut self, k: Keyframe) {
        self.keys.clear();
        self.end = k.tick;
        self.floor = k.tick;
        self.run = next_run();
        self.push(k);
    }

    /// The run branched at `tick`: what came after it on the old branch
    /// never happens now.
    pub fn prune_after(&mut self, tick: u64) {
        self.keys.retain(|k| k.tick <= tick);
        self.end = tick;
    }

    /// Keyframes held.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// No keyframes.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Bytes held.
    pub fn bytes(&self) -> usize {
        self.keys.iter().map(|k| k.bytes).sum()
    }

    /// Is there a barrier keyframe (a change made between ticks) at `tick`?
    pub fn barrier_at(&self, tick: u64) -> bool {
        self.keys.iter().any(|k| k.barrier && k.tick == tick)
    }

    /// Ticks of the keyframes held, oldest first.
    pub fn ticks(&self) -> Vec<u64> {
        self.keys.iter().map(|k| k.tick).collect()
    }

    /// Age thinning (§6, stretched; see `DENSE_SECONDS`): every keyframe
    /// for the last five minutes of sim time, the first of each minute up
    /// to an hour, the first of each ten minutes beyond. Barriers, the
    /// oldest, and the newest stay.
    fn thin(&mut self) {
        let Some(newest) = self.keys.last().map(|k| k.time) else {
            return;
        };
        let n = self.keys.len();
        let mut seen: Vec<(u8, i64)> = Vec::new();
        let mut keep = vec![true; n];
        for (i, k) in self.keys.iter().enumerate() {
            if k.barrier || i == 0 || i + 1 == n {
                continue;
            }
            let age = newest - k.time;
            let bucket = if age <= DENSE_SECONDS {
                continue;
            } else if age <= HOUR {
                (1u8, (k.time / 60.0).floor() as i64)
            } else {
                (2u8, (k.time / 600.0).floor() as i64)
            };
            if seen.contains(&bucket) {
                keep[i] = false;
            } else {
                seen.push(bucket);
            }
        }
        let mut i = 0;
        self.keys.retain(|_| {
            i += 1;
            keep[i - 1]
        });
    }

    /// Stay under the budget: the oldest plain keyframes go first (a seek
    /// into their stretch only replays longer); if barriers alone are over,
    /// the oldest go and the floor moves up, since a replay may not cross a
    /// change between ticks.
    fn fit(&mut self) {
        let mut bytes = self.bytes();
        while bytes > self.budget && self.keys.len() > 1 {
            let last = self.keys.len() - 1;
            let i = (1..last).find(|&i| !self.keys[i].barrier).unwrap_or(0);
            let k = self.keys.remove(i);
            bytes -= k.bytes;
            if i == 0 || k.barrier {
                self.floor = self.keys[0].tick;
            }
        }
    }

    /// `sim.ring`'s report (phase 4 item 5, the editor's scrubber):
    /// `status`, which run this is, the keyframes' ticks and the barriers',
    /// and where the stretch with every keyframe begins, since a seek there
    /// replays at most `every` ticks and one before it up to a minute.
    pub fn report(&self) -> Json {
        let mut out = self.status();
        let dense_from = self.keys.last().and_then(|newest| {
            self.keys
                .iter()
                .find(|k| newest.time - k.time <= DENSE_SECONDS)
                .map(|k| k.tick)
        });
        out["run"] = json!(self.run);
        out["dense_from"] = json!(dense_from);
        out["capture_ms"] = match self.capture_ms() {
            Some((last, max)) => json!({ "last": last, "max": max }),
            None => Json::Null,
        };
        out["ticks"] = json!(spaced(self.ticks()));
        out["barrier_ticks"] = json!(spaced(
            self.keys
                .iter()
                .filter(|k| k.barrier)
                .map(|k| k.tick)
                .collect()
        ));
        out
    }

    /// `sim.snapshot`'s report.
    pub fn status(&self) -> Json {
        json!({
            "keyframes": self.keys.len(),
            "bytes": self.bytes(),
            "budget": self.budget,
            "every": self.every,
            "oldest": self.keys.first().map(|k| k.tick),
            "newest": self.keys.last().map(|k| k.tick),
            "barriers": self.keys.iter().filter(|k| k.barrier).count(),
            "end": self.end,
            "floor": self.floor,
        })
    }
}
