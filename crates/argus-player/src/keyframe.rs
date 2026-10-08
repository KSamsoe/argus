//! Keyframes (§6, phase 2 item 0): everything the sim needs that the world
//! does not hold, beside a snapshot of the world. The daemon's ring keeps
//! them for `sim.seek` and `sim.fork`; a replay file carries one; the
//! release build's recorder keeps two (phase 2 item 7).
//!
//! A keyframe is the world, the scheduler's clocks and RNG streams and which
//! systems are off, the input state, the particle pools, which rooms are
//! loaded, and the colliders overlapping at the end of the tick before
//! (phase 3 item 5), which the next tick's overlap events are measured
//! against; and the cells of the runtime maps, which the game's scripts
//! wrote (phase 5 item 4).

use argus_ecs::{Entity, World};
use argus_input::{InputSnapshot, InputState};
use argus_particles::{ParticleStore, Pool};
use argus_scene::{Scene, StreamState};
use argus_sim::{Scheduler, SimSnapshot};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

/// Where the rigid bodies' state lives (phase 5 item 7): the module's
/// store when the game uses it, nothing in a build without the module.
#[cfg(feature = "rigid")]
pub type RigidSource = Option<Rc<RefCell<argus_rigid::RigidStore>>>;
/// Where the rigid bodies' state lives: a build without the module has none.
#[cfg(not(feature = "rigid"))]
pub type RigidSource = ();

/// The rigid bodies' state in a keyframe: Rapier's contact caches and
/// solver warm-start, without which a replay parts from its run.
#[derive(Clone, Default)]
pub struct RigidKeep(#[cfg(feature = "rigid")] pub Option<argus_rigid::RigidState>);

impl RigidKeep {
    /// Taken from the store.
    #[allow(unused_variables)]
    pub fn capture(src: &RigidSource) -> RigidKeep {
        #[cfg(feature = "rigid")]
        return RigidKeep(src.as_ref().map(|s| s.borrow().state()));
        #[cfg(not(feature = "rigid"))]
        RigidKeep()
    }

    /// Put back into the store.
    #[allow(unused_variables)]
    pub fn restore(&self, src: &RigidSource) {
        #[cfg(feature = "rigid")]
        if let (Some(s), Some(k)) = (src, &self.0) {
            s.borrow_mut().restore(k);
        }
    }

    /// Bytes held.
    pub fn bytes(&self) -> usize {
        #[cfg(feature = "rigid")]
        return self.0.as_ref().map_or(0, |s| s.bytes());
        #[cfg(not(feature = "rigid"))]
        0
    }

    /// As bytes, for a replay file: none without bodies.
    pub fn encode(&self) -> Option<Vec<u8>> {
        #[cfg(feature = "rigid")]
        return self.0.as_ref().map(|s| s.encode());
        #[cfg(not(feature = "rigid"))]
        None
    }

    /// From a replay file's bytes.
    #[allow(unused_variables)]
    pub fn decode(bytes: Option<&[u8]>) -> argus_ecs::Result<RigidKeep> {
        #[cfg(feature = "rigid")]
        return Ok(RigidKeep(match bytes {
            Some(b) => Some(argus_rigid::RigidState::decode(b)?),
            None => None,
        }));
        #[cfg(not(feature = "rigid"))]
        match bytes {
            Some(_) => Err(argus_ecs::ArgusError::new(
                argus_ecs::codes::BAD_STATE,
                "the replay holds rigid bodies, and this build has no rigid-body module",
            )),
            None => Ok(RigidKeep()),
        }
    }
}

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
    /// The runtime maps' cells.
    pub tiles: argus_tiles::RuntimeCells,
    /// The rigid bodies' state (phase 5 item 7).
    pub rigid: RigidKeep,
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
    #[allow(clippy::too_many_arguments)]
    pub fn read_back(
        tick: u64,
        time: f64,
        sim: SimSnapshot,
        input: Option<InputSnapshot>,
        pools: BTreeMap<Entity, Pool>,
        rooms: Option<StreamState>,
        contacts: Vec<(Entity, Entity)>,
        tiles: argus_tiles::RuntimeCells,
    ) -> Keyframe {
        let pool_bytes: usize = pools.values().map(|p| p.len() * 24).sum();
        Keyframe {
            tick,
            time,
            bytes: sim.world.bytes() + pool_bytes + cell_bytes(&tiles) + 1024,
            sim,
            input,
            pools,
            rooms,
            contacts,
            tiles,
            rigid: RigidKeep::default(),
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
    /// The scene, whose rooms loaded (and what unloaded rooms remember) a
    /// keyframe keeps: rooms stream inside a tick (phase 3 item 6).
    pub scene: Option<Rc<RefCell<Scene>>>,
    /// The colliders `physics.overlap` saw overlapping last tick.
    pub contacts: Rc<RefCell<argus_physics::Contacts>>,
    /// The maps, whose runtime ones' cells a keyframe keeps.
    pub tiles: Option<Rc<RefCell<argus_tiles::MapStore>>>,
    /// The rigid bodies' store, when the game uses the module.
    pub rigid: RigidSource,
}

/// A run's digest (§6, phase 5 item 4): the world's, and the runtime
/// maps' cells with it when any holds a tile. Two runs with the same one
/// are the same run; a game without runtime maps gets its world's digest,
/// as it always did.
pub fn run_digest(world: &World, cells: &argus_tiles::RuntimeCells) -> u64 {
    match argus_tiles::cells_digest(cells) {
        None => world.digest(),
        Some(c) => {
            let mut b = world.digest().to_le_bytes().to_vec();
            b.extend_from_slice(&c.to_le_bytes());
            argus_tiles::fnv1a64(&b)
        }
    }
}

/// Bytes the runtime maps' cells hold.
fn cell_bytes(t: &argus_tiles::RuntimeCells) -> usize {
    t.iter()
        .flat_map(|(_, c)| c.iter().map(|x| x.3.len()))
        .sum()
}

impl Sources {
    /// A keyframe of the world and the sim now.
    pub fn capture(&self, world: &World, sched: &Scheduler, barrier: bool) -> Keyframe {
        let sim = sched.snapshot(world);
        let pools = self.particles.borrow().pools.clone();
        let pool_bytes: usize = pools.values().map(|p| p.len() * 24).sum();
        let tiles = self
            .tiles
            .as_ref()
            .map(|t| t.borrow().runtime_cells())
            .unwrap_or_default();
        let rigid = RigidKeep::capture(&self.rigid);
        Keyframe {
            tick: sched.tick(),
            time: sched.driving().time(),
            input: self.input.as_ref().map(|i| i.borrow().snapshot()),
            rooms: self.scene.as_ref().map(|s| s.borrow().stream_state()),
            contacts: self.contacts.borrow().pairs.clone(),
            bytes: sim.world.bytes() + pool_bytes + cell_bytes(&tiles) + rigid.bytes() + 1024,
            tiles,
            rigid,
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
        if let (Some(s), Some(r)) = (&self.scene, &k.rooms) {
            s.borrow_mut().set_stream_state(r);
        }
        self.contacts.borrow_mut().pairs = k.contacts.clone();
        if let Some(t) = &self.tiles {
            t.borrow_mut().set_runtime_cells(&k.tiles);
        }
        k.rigid.restore(&self.rigid);
    }
}
