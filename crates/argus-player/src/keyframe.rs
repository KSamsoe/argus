//! Keyframes (§6, phase 2 item 0): everything the sim needs that the world
//! does not hold, beside a snapshot of the world. The daemon's ring keeps
//! them for `sim.seek` and `sim.fork`; a replay file carries one; the
//! release build's recorder keeps two (phase 2 item 7).
//!
//! A keyframe is the world, the scheduler's clocks and RNG streams and which
//! systems are off, the input state, the particle pools, which rooms are
//! loaded, and the colliders overlapping at the end of the tick before
//! (phase 3 item 5), which the next tick's overlap events are measured
//! against.

use argus_ecs::{Entity, World};
use argus_input::{InputSnapshot, InputState};
use argus_particles::{ParticleStore, Pool};
use argus_scene::StreamState;
use argus_sim::{Scheduler, SimSnapshot};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

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
    /// The pairs of colliders overlapping at the end of the tick before.
    pub contacts: Vec<(Entity, Entity)>,
    /// Taken at a load or after a change between ticks: never thinned.
    pub barrier: bool,
    /// Approximate bytes held.
    pub bytes: usize,
    /// Order among keyframes at one tick, set by the ring that keeps it.
    pub seq: u64,
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
        contacts: Vec<(Entity, Entity)>,
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
            contacts,
            barrier: true,
            seq: 0,
        }
    }
}

/// What a keyframe is captured from, shared with the runtime that owns them.
#[derive(Clone)]
pub struct Sources {
    /// The session's input state.
    pub input: Option<Rc<RefCell<InputState>>>,
    /// The particle pools.
    pub particles: Rc<RefCell<ParticleStore>>,
    /// The rooms loaded, kept current by the owner (they change only
    /// between ticks).
    pub rooms: Rc<RefCell<Option<StreamState>>>,
    /// The colliders `physics.overlap` saw overlapping last tick.
    pub contacts: Rc<RefCell<argus_physics::Contacts>>,
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
            contacts: self.contacts.borrow().pairs.clone(),
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
        self.contacts.borrow_mut().pairs = k.contacts.clone();
    }
}
