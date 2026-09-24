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

use argus_ecs::{Entity, World};
use argus_input::{InputSnapshot, InputState};
use argus_particles::{ParticleStore, Pool};
use argus_scene::StreamState;
use argus_sim::{Scheduler, SimSnapshot};
use serde_json::{json, Value as Json};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

/// Every keyframe for the last five minutes of sim time: §17's seek target
/// (any point in the last five minutes under 500 ms) cannot be met by §6's
/// "every second for the last minute", which leaves a replay of up to a
/// minute at the fast-forward rate §17 also sets (2,000 ticks a second).
pub const DENSE_SECONDS: f64 = 300.0;
/// Then one keyframe a minute up to an hour, and one per ten minutes after.
pub const HOUR: f64 = 3600.0;

/// Everything a seek restores.
#[derive(Clone)]
pub struct Keyframe {
    /// The next tick to run on the driving clock; the world's frame.
    pub tick: u64,
    /// Sim seconds at capture, which age thinning measures.
    pub time: f64,
    /// The world, the clocks, the RNG streams, which systems are off.
    pub sim: SimSnapshot,
    /// The input state; the journal after it is replayed.
    pub input: Option<InputSnapshot>,
    /// The particle pools, which live outside the world.
    pub pools: BTreeMap<Entity, Pool>,
    /// The rooms loaded, for a scene with rooms.
    pub rooms: Option<StreamState>,
    /// Taken at a load or after a change between ticks: never thinned.
    pub barrier: bool,
    /// Approximate bytes held.
    pub bytes: usize,
    seq: u64,
}

impl Keyframe {
    /// A keyframe read back from a replay file (phase 2 item 2): a
    /// barrier, since nothing before it is known.
    pub fn read_back(
        tick: u64,
        time: f64,
        sim: SimSnapshot,
        input: Option<InputSnapshot>,
        pools: BTreeMap<Entity, Pool>,
        rooms: Option<StreamState>,
    ) -> Keyframe {
        let pool_bytes: usize = pools.values().map(|p| p.len() * 24).sum();
        Keyframe {
            tick,
            time,
            bytes: sim.world.bytes() + pool_bytes + 1024,
            sim,
            input,
            pools,
            rooms,
            barrier: true,
            seq: 0,
        }
    }
}

/// What the tick-end hook captures from, shared with the daemon.
#[derive(Clone)]
pub struct Sources {
    /// The session's input state.
    pub input: Option<Rc<RefCell<InputState>>>,
    /// The particle pools.
    pub particles: Rc<RefCell<ParticleStore>>,
    /// The rooms loaded, kept current by the daemon (they change only
    /// between ticks).
    pub rooms: Rc<RefCell<Option<StreamState>>>,
}

impl Sources {
    /// A keyframe of the world and the sim now.
    pub fn capture(&self, world: &World, sched: &Scheduler, barrier: bool) -> Keyframe {
        let sim = sched.snapshot(world);
        let pools = self.particles.borrow().pools.clone();
        let pool_bytes: usize = pools.values().map(|p| p.len() * 24).sum();
        Keyframe {
            tick: sched.tick(),
            time: sched.driving().time(),
            input: self.input.as_ref().map(|i| i.borrow().snapshot()),
            rooms: self.rooms.borrow().clone(),
            bytes: sim.world.bytes() + pool_bytes + 1024,
            sim,
            pools,
            barrier,
            seq: 0,
        }
    }

    /// Put a keyframe's state back (the world and the scheduler are the
    /// caller's), keeping the play state the caller has now.
    pub fn restore(&self, k: &Keyframe, world: &mut World, sched: &mut Scheduler) {
        let (playing, paused) = (sched.is_playing(), sched.is_paused());
        let mut sim = k.sim.clone();
        sim.sim.playing = playing;
        sim.sim.paused = paused;
        sched.restore(world, &sim);
        if let (Some(i), Some(s)) = (&self.input, &k.input) {
            i.borrow_mut().restore(s);
        }
        self.particles.borrow_mut().pools = k.pools.clone();
        *self.rooms.borrow_mut() = k.rooms.clone();
    }
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
        }
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
