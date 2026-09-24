//! Replay files (phase 2 item 2): a keyframe on disk, the input commands
//! after it, and the world at the frame the file reproduces. A mark keeps
//! one, so it reproduces after the daemon that took it is gone; the test
//! runner's replay export (item 3) and a release build's report key
//! (item 7) write the same file.
//!
//! Layout: the tag `ARGR` and a version; the run's identity as JSON (scene,
//! seed, code, clock, `from`, `to`); the keyframe (the world as
//! `World::encode_state` writes it, the sim state, the input state, the
//! particle pools, the rooms); the journal of input commands from `from`
//! to `to`; and the world at `to`, which a reader compares a replay with.

use crate::ring::Keyframe;
use argus_ecs::codec::{Reader, Writer};
use argus_ecs::{ArgusError, Result, Snapshot, StateReport, World};
use argus_input::{InputSnapshot, JournalEntry};
use argus_particles::ParticleStore;
use argus_scene::StreamState;
use argus_sim::{SimSnapshot, SimState};
use serde_json::Value as Json;
use std::path::Path;

const TAG: &[u8; 4] = b"ARGR";
const VERSION: u32 = 1;

/// A replay file, read back. The two worlds stay bytes until a live world
/// (with the components registered) reads them.
pub struct ReplayFile {
    /// The run's identity and the span: scene, seed, code, clock, from, to.
    pub meta: Json,
    /// The keyframe's tick.
    pub from: u64,
    /// The frame the file reproduces.
    pub to: u64,
    /// Sim seconds at the keyframe.
    pub time: f64,
    world: Vec<u8>,
    sim: SimState,
    input: Option<InputSnapshot>,
    pools: Vec<u8>,
    rooms: Option<StreamState>,
    /// The input commands from the keyframe on.
    pub journal: Vec<JournalEntry>,
    end: Option<Vec<u8>>,
}

/// What a keyframe read back into a live world dropped or remapped.
pub struct ReadBack {
    /// The keyframe, ready to restore.
    pub keyframe: Keyframe,
    /// The world part's report (components remapped or dropped since).
    pub world: StateReport,
    /// Particle effects the file's pools used that this project no longer has.
    pub missing_effects: Vec<String>,
}

/// Write a replay file: `k` and the commands after it (`journal`, already
/// cut to the span), `end` the world at `meta.to`.
pub fn write(
    path: &Path,
    meta: &Json,
    k: &Keyframe,
    base: usize,
    journal: &[JournalEntry],
    end: Option<&World>,
) -> Result<usize> {
    let mut w = Writer::new();
    w.magic(TAG, VERSION);
    w.str(&meta.to_string());
    w.u64(k.tick);
    w.f64(k.time);
    w.bytes(&k.sim.world.world().encode_state());
    k.sim.sim.encode(&mut w);
    w.opt(k.input.as_ref(), |w, s| s.encode(w, base));
    let mut pools = Writer::new();
    ParticleStore::encode_pools(&k.pools, &mut pools);
    w.bytes(&pools.into_bytes());
    w.opt(k.rooms.as_ref(), |w, r| r.encode(w));
    w.count(journal.len());
    for e in journal {
        e.encode(&mut w);
    }
    w.opt(end, |w, world| w.bytes(&world.encode_state()));
    let bytes = w.into_bytes();
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| io(path, e))?;
    }
    std::fs::write(path, &bytes).map_err(|e| io(path, e))?;
    Ok(bytes.len())
}

fn io(path: &Path, e: impl std::fmt::Display) -> ArgusError {
    ArgusError::new(
        argus_ecs::codes::BAD_STATE,
        format!("{}: {e}", path.display()),
    )
}

/// Read a replay file.
pub fn read(path: &Path) -> Result<ReplayFile> {
    let bytes = std::fs::read(path).map_err(|e| io(path, e))?;
    let mut r = Reader::new(&bytes, "replay file");
    r.magic(TAG, VERSION)?;
    let meta: Json =
        serde_json::from_str(&r.str()?).map_err(|e| r.bad(format!("its identity: {e}")))?;
    let from = r.u64()?;
    let time = r.f64()?;
    let world = r.bytes()?.to_vec();
    let sim = SimState::decode(&mut r)?;
    let input = r.opt(|r| InputSnapshot::decode(r))?;
    let pools = r.bytes()?.to_vec();
    let rooms = r.opt(|r| StreamState::decode(r))?;
    let mut journal = Vec::new();
    for _ in 0..r.count()? {
        journal.push(JournalEntry::decode(&mut r)?);
    }
    let end = r.opt(|r| r.bytes().map(<[u8]>::to_vec))?;
    if r.remaining() != 0 {
        return Err(r.bad(format!("{} bytes after the end", r.remaining())));
    }
    let to = meta.get("to").and_then(Json::as_u64).unwrap_or(from);
    Ok(ReplayFile {
        meta,
        from,
        to,
        time,
        world,
        sim,
        input,
        pools,
        rooms,
        journal,
        end,
    })
}

impl ReplayFile {
    /// The keyframe, read into a copy of `live` (a world with the
    /// project's components registered) and `store`'s effects.
    pub fn keyframe(&self, live: &World, store: &ParticleStore) -> Result<ReadBack> {
        let mut world = live.clone();
        let report = world.decode_state(&self.world)?;
        let mut r = Reader::new(&self.pools, "replay file pools");
        let (pools, missing_effects) = store.decode_pools(&mut r)?;
        let keyframe = Keyframe::read_back(
            self.from,
            self.time,
            SimSnapshot {
                world: Snapshot::of(world),
                sim: self.sim.clone(),
            },
            self.input.clone(),
            pools,
            self.rooms.clone(),
        );
        Ok(ReadBack {
            keyframe,
            world: report,
            missing_effects,
        })
    }

    /// The world at `to`, read into a copy of `live`, if the file has it.
    pub fn end_world(&self, live: &World) -> Result<Option<World>> {
        let Some(bytes) = &self.end else {
            return Ok(None);
        };
        let mut world = live.clone();
        world.decode_state(bytes)?;
        Ok(Some(world))
    }
}
