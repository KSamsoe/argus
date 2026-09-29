//! Save games (§6, phase 3 item 0): "a save is a snapshot with a schema
//! version, written by sim.save to a player-facing location instead of the
//! dev ring". The daemon writes one on `sim.save`, a release build and the
//! web page when the game asks (`q:save`), and all three read it back the
//! same way.
//!
//! Layout: the tag `ARGS` and a version; the game's identity as JSON (game,
//! scene, seed, code, clock, tick, time, name, saved_at); then the keyframe
//! as a replay file holds one (the world as `World::encode_state` writes
//! it, with each component's schema version; the sim state; the particle
//! pools; the rooms), without the input state: a loaded game hears the
//! player's hands now, not the ones that saved it.
//!
//! A save must survive code changes. Its world is read field by field, by
//! name, and components saved at an older version go through the
//! migrations their schema files declare (`argus_ecs::Migrate`); a save
//! written by a newer version of the game is refused rather than read
//! wrong.

use crate::keyframe::Keyframe;
use argus_ecs::codec::{Reader, Writer};
use argus_ecs::{codes, ArgusError, Migrate, Result, Snapshot, StateReport, World};
use argus_particles::ParticleStore;
use argus_scene::StreamState;
use argus_sim::{SimSnapshot, SimState};
use serde_json::Value as Json;

const TAG: &[u8; 4] = b"ARGS";
const VERSION: u32 = 2;

/// A save file's bytes: `meta` (the identity) and the keyframe `k`.
pub fn encode(meta: &Json, k: &Keyframe) -> Vec<u8> {
    let mut w = Writer::new();
    w.magic(TAG, VERSION);
    w.str(&meta.to_string());
    w.u64(k.tick);
    w.f64(k.time);
    w.bytes(&k.sim.world.world().encode_state());
    k.sim.sim.encode(&mut w);
    let mut pools = Writer::new();
    ParticleStore::encode_pools(&k.pools, &mut pools);
    w.bytes(&pools.into_bytes());
    w.opt(k.rooms.as_ref(), |w, r| r.encode(w));
    w.into_bytes()
}

/// A save file, read back. Its world stays bytes until a live world (the
/// project's components registered, by this build) reads it.
pub struct SaveFile {
    /// The identity: game, scene, seed, code, clock, tick, time, name,
    /// saved_at.
    pub meta: Json,
    /// The tick it was saved at.
    pub tick: u64,
    /// Sim seconds at the save.
    pub time: f64,
    world: Vec<u8>,
    sim: SimState,
    pools: Vec<u8>,
    rooms: Option<StreamState>,
}

/// A save read into a live world.
pub struct Loaded {
    /// The keyframe to restore.
    pub keyframe: Keyframe,
    /// What reading the world did: components migrated, remapped, dropped.
    pub world: StateReport,
    /// Particle effects the save used that the project no longer has.
    pub missing_effects: Vec<String>,
}

/// A save file from its bytes.
pub fn decode(bytes: &[u8]) -> Result<SaveFile> {
    let mut r = Reader::new(bytes, "save file");
    let version = r.magic(TAG, VERSION)?;
    let meta: Json =
        serde_json::from_str(&r.str()?).map_err(|e| r.bad(format!("its identity: {e}")))?;
    let tick = r.u64()?;
    let time = r.f64()?;
    let world = r.bytes()?.to_vec();
    let sim = SimState::decode(&mut r)?;
    let pools = r.bytes()?.to_vec();
    // Rooms remember what happened in them from version 2 (phase 3 item 6).
    let rooms = r.opt(|r| StreamState::decode_as(r, version >= 2))?;
    if r.remaining() != 0 {
        return Err(r.bad(format!("{} bytes after the end", r.remaining())));
    }
    Ok(SaveFile {
        meta,
        tick,
        time,
        world,
        sim,
        pools,
        rooms,
    })
}

impl SaveFile {
    /// The scene it was saved in.
    pub fn scene(&self) -> &str {
        self.meta["scene"].as_str().unwrap_or("")
    }

    /// The run seed it was saved with.
    pub fn seed(&self) -> Option<u64> {
        self.meta["seed"].as_u64()
    }

    /// The save read into a copy of `live` (the project's components
    /// registered) and `store`'s effects, rows of older component versions
    /// through `migrate`. Refused when a component was saved by a newer
    /// version than this build has.
    pub fn read_into(
        &self,
        live: &World,
        store: &ParticleStore,
        migrate: Option<&mut dyn Migrate>,
    ) -> Result<Loaded> {
        let mut world = live.clone();
        let report = world.decode_state_with(&self.world, migrate)?;
        if !report.newer.is_empty() {
            let list: Vec<String> = report
                .newer
                .iter()
                .map(|(n, saved, now)| format!("{n} v{saved} (this build has v{now})"))
                .collect();
            return Err(ArgusError::new(
                codes::BAD_STATE,
                format!(
                    "the save was written by a newer version of the game: {}",
                    list.join(", ")
                ),
            )
            .hint("Load it with the version that wrote it, or newer; a save does not migrate backwards."));
        }
        let mut r = Reader::new(&self.pools, "save file pools");
        let (pools, missing_effects) = store.decode_pools(&mut r)?;
        let keyframe = Keyframe::read_back(
            self.tick,
            self.time,
            SimSnapshot {
                world: Snapshot::of(world),
                sim: self.sim.clone(),
            },
            None,
            pools,
            self.rooms.clone(),
            // A loaded game starts afresh: what touches is met again.
            Vec::new(),
        );
        Ok(Loaded {
            keyframe,
            world: report,
            missing_effects,
        })
    }
}

/// At the end of a tick (a scheduler's tick-end hook): a keyframe for each
/// `save.requested` the tick emitted (`q:save(name)` in a script), with the
/// save's name, so a save holds the world of the tick that asked for it
/// even when many ticks run in one step.
pub fn requested(
    world: &World,
    sched: &argus_sim::Scheduler,
    sources: &crate::keyframe::Sources,
    out: &mut Vec<(String, Keyframe)>,
) {
    // The world's frame moved on at the end of the tick; its events carry
    // the one before.
    let frame = world.frame().saturating_sub(1);
    // The tick's events are the last ones (the queue is in frame order): not
    // every event of a long step, every tick (found at the phase 3 gate).
    let all = world.events();
    let start = all.len() - all.iter().rev().take_while(|r| r.frame >= frame).count();
    for rec in all[start..].iter().filter(|r| r.frame == frame) {
        if let argus_ecs::Event::Game { name, fields } = &rec.event {
            if name == "save.requested" {
                out.push((request_name(fields), sources.capture(world, sched, true)));
            }
        }
    }
}

/// The name a `save.requested` or `load.requested` event carries ("auto"
/// when it says none).
pub fn request_name(fields: &[(String, argus_ecs::Value)]) -> String {
    fields
        .iter()
        .find(|(k, _)| k == "name")
        .and_then(|(_, v)| match v {
            argus_ecs::Value::Str(s) if !s.is_empty() => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "auto".to_string())
}

/// A save's name as a file name: letters, digits, `-`, `_`, and `.` kept,
/// anything else `_`; empty is "save".
pub fn file_name(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let s = s.trim_matches('.').to_string();
    format!("{}.save", if s.is_empty() { "save" } else { &s })
}

/// The report of a load as JSON: each component migrated (§6: "Health
/// migrated v2→v3 for 1,204 entities"), remapped, or dropped.
pub fn report_json(l: &Loaded) -> Json {
    serde_json::json!({
        "entities": l.world.entities,
        "migrated": l.world.migrated.iter().map(|m| serde_json::json!({
            "component": m.component,
            "from": m.from,
            "to": m.to,
            "entities": m.rows,
            "declared": m.declared,
            "left": m.left,
        })).collect::<Vec<_>>(),
        "remapped": l.world.remapped,
        "dropped": l.world.dropped,
        "missing_effects": l.missing_effects,
    })
}
