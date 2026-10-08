//! Where the dynamic tier's state lives between ticks: Rapier's world, and
//! what ties it to the ECS. None of it is a component, so a keyframe takes
//! it ([`RigidStore::state`]) and a seek puts it back
//! ([`RigidStore::restore`]): Rapier's contact caches and solver warm-start
//! carry from one tick to the next, and a run replayed from a keyframe
//! without them would part from the run it replays.

use argus_ecs::{ArgusError, Entity, Result, Vec2};
use rapier2d::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What the last tick wrote into an entity, so the next one can tell a
/// change made by gameplay (a throw, a teleport) from its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Wrote {
    pub(crate) pos: [f32; 2],
    pub(crate) rot: f32,
    pub(crate) vel: [f32; 2],
    pub(crate) spin: f32,
}

/// A body's settings and shape as last given to Rapier.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Given {
    pub(crate) kind: u8,
    pub(crate) gravity_scale: f32,
    pub(crate) damping: f32,
    pub(crate) angular_damping: f32,
    pub(crate) lock_rotation: bool,
    pub(crate) ccd: bool,
    pub(crate) can_sleep: bool,
    pub(crate) size: [f32; 2],
    pub(crate) radius: f32,
    pub(crate) offset: [f32; 2],
    pub(crate) density: f32,
    pub(crate) friction: f32,
    pub(crate) restitution: f32,
    pub(crate) layer: u32,
    pub(crate) mask: u32,
}

/// One rigid body.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Body {
    pub(crate) body: RigidBodyHandle,
    pub(crate) collider: ColliderHandle,
    pub(crate) wrote: Wrote,
    pub(crate) given: Given,
}

/// A kinematic-tier body, as Rapier sees it: moved to where
/// `physics.move` put it, pushing what is in the way.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Proxy {
    pub(crate) body: RigidBodyHandle,
    pub(crate) collider: ColliderHandle,
    pub(crate) shape: [f32; 4],
}

/// One joint, and what it was built from.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct JointRec {
    pub(crate) joint: ImpulseJointHandle,
    pub(crate) key: Vec<u32>,
    pub(crate) break_force: f32,
}

/// One tilemap's solid tiles as fixed colliders, and what they were
/// built from.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct MapRec {
    pub(crate) body: RigidBodyHandle,
    pub(crate) key: (String, [u32; 2], [u64; 2]),
}

/// Everything that carries from one tick to the next.
#[derive(Clone, Serialize, Deserialize)]
pub struct RigidState {
    pub(crate) gravity: [f32; 2],
    pub(crate) params: IntegrationParameters,
    pub(crate) islands: IslandManager,
    pub(crate) broad_phase: BroadPhaseBvh,
    pub(crate) narrow_phase: NarrowPhase,
    pub(crate) bodies: RigidBodySet,
    pub(crate) colliders: ColliderSet,
    pub(crate) impulse_joints: ImpulseJointSet,
    pub(crate) multibody_joints: MultibodyJointSet,
    pub(crate) soft_bodies: SoftBodySet,
    /// Keyed by an entity's bits (`Entity::to_bits`), in entity order.
    pub(crate) rigid: BTreeMap<u64, Body>,
    pub(crate) proxies: BTreeMap<u64, Proxy>,
    pub(crate) joints: BTreeMap<u64, JointRec>,
    pub(crate) maps: BTreeMap<u64, MapRec>,
    pub(crate) ground: Option<RigidBodyHandle>,
    /// Pairs touching at the end of the last tick, the lower entity first.
    pub(crate) touching: Vec<(u64, u64)>,
}

impl std::fmt::Debug for RigidState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RigidState")
            .field("bodies", &self.rigid.len())
            .field("joints", &self.joints.len())
            .field("touching", &self.touching.len())
            .finish()
    }
}

impl Default for RigidState {
    fn default() -> Self {
        RigidState {
            gravity: [0.0, -9.81],
            params: IntegrationParameters::default(),
            islands: IslandManager::new(),
            broad_phase: BroadPhaseBvh::default(),
            narrow_phase: NarrowPhase::new(),
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            impulse_joints: ImpulseJointSet::new(),
            multibody_joints: MultibodyJointSet::new(),
            soft_bodies: SoftBodySet::default(),
            rigid: BTreeMap::new(),
            proxies: BTreeMap::new(),
            joints: BTreeMap::new(),
            maps: BTreeMap::new(),
            ground: None,
            touching: Vec::new(),
        }
    }
}

impl RigidState {
    /// Nothing is simulated.
    pub fn is_empty(&self) -> bool {
        self.rigid.is_empty() && self.joints.is_empty()
    }

    /// Rigid bodies held.
    pub fn bodies(&self) -> usize {
        self.rigid.len()
    }

    /// Approximate bytes held, for a keyframe's size.
    pub fn bytes(&self) -> usize {
        1024 + self.bodies.len() * 900
            + self.colliders.len() * 400
            + self.impulse_joints.len() * 300
    }

    /// As bytes, for a replay file or a player's report.
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// Read what [`RigidState::encode`] wrote.
    pub fn decode(bytes: &[u8]) -> Result<RigidState> {
        serde_json::from_slice(bytes).map_err(|e| {
            ArgusError::new(
                crate::codes::BAD_STATE,
                format!("the rigid bodies' state does not read: {e}"),
            )
            .hint("It was written by another version of the rigid-body module; replay from a keyframe of this one.")
        })
    }
}

/// The dynamic tier's store: its state, and Rapier's pipeline, whose
/// buffers are a workspace and not part of the state.
#[derive(Default)]
pub struct RigidStore {
    pub(crate) state: RigidState,
    pub(crate) pipeline: PhysicsPipeline,
    pub(crate) ccd: CCDSolver,
}

impl std::fmt::Debug for RigidStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.state.fmt(f)
    }
}

impl RigidStore {
    /// An empty store.
    pub fn new() -> RigidStore {
        RigidStore::default()
    }

    /// What a keyframe keeps.
    pub fn state(&self) -> RigidState {
        self.state.clone()
    }

    /// Put a keyframe's state back.
    pub fn restore(&mut self, state: &RigidState) {
        self.state = state.clone();
    }

    /// The state now.
    pub fn now(&self) -> &RigidState {
        &self.state
    }

    /// The rigid body an entity is, if it is one: its position, angle, and
    /// velocity in Rapier.
    pub fn body(&self, e: Entity) -> Option<(Vec2, f32, Vec2)> {
        let b = self.state.rigid.get(&e.to_bits())?;
        let rb = self.state.bodies.get(b.body)?;
        let t = rb.translation();
        let v = rb.linvel();
        Some((
            Vec2::new(t.x, t.y),
            rb.rotation().angle(),
            Vec2::new(v.x, v.y),
        ))
    }
}
