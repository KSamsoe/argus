//! Argus skeletal 2D (§13 "Skeletal 2D via a Spine-compatible runtime",
//! phase 5 item 8): the engine's own runtime for skeletons made in Spine.
//!
//! A skeleton is its Spine 4 JSON export (`assets/<name>.json`, read by
//! `argus_assets::spine`). A [`Skeleton`] on an entity poses it every tick:
//! bones with Spine's inheritance, one- and two-bone IK, a skin, slot
//! attachments and colours, draw order. Each slot is drawn by a part, an
//! ordinary entity under the skeleton's (`Transform`, `Parent`, `Sprite`,
//! and a [`SkeletonPart`]), so lighting, materials, the hierarchy panel,
//! and saves treat a skeleton's images as sprites.
//!
//! What plays is an animator's state, when the entity has an `Animator`:
//! each Spine animation is a clip of the skeleton (argus-anim), so the
//! state machine, its windows, frame events, anticipation, holds, and
//! crossfades work unchanged, and the pose follows the animator's frame
//! exactly. Without one, `Skeleton.animation` plays at `Skeleton.speed`.
//! Spine's own events fire as engine events when the animation passes
//! them. Everything the pose depends on is in components, so a replay or
//! a seek poses the same skeleton the same way.
//!
//! `skeletal.pose` runs in the late stage after `anim.update`. A scene
//! without a Skeleton pays one look at its archetypes a tick.

#![warn(missing_docs)]

pub mod pose;
mod system;

use argus_anim::AnimStore;
use argus_ecs::{Component, Entity, Result, StrId, World};
use argus_sim::Scheduler;
use std::cell::RefCell;
use std::rc::Rc;

pub use system::SYSTEM;

/// Poses a Spine skeleton on its entity, drawn by part entities.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Poses a Spine skeleton (an assets/*.json exported from Spine 4) on the entity, each slot drawn by a part entity under it. With an Animator (sprite = the skeleton) its state's clip is the animation; without, animation plays at speed"
)]
pub struct Skeleton {
    /// The skeleton asset.
    #[argus(doc = "The skeleton asset's name (assets/hero/hero.json is hero/hero)")]
    pub skeleton: StrId,
    /// The skin.
    #[argus(doc = "The skin whose attachments show; empty is the default skin")]
    pub skin: StrId,
    /// What plays, or what played.
    #[argus(
        doc = "Without an Animator, the animation to play (empty holds the setup pose); with one, the animation its state plays (written)"
    )]
    pub animation: StrId,
    /// Seconds into the animation.
    #[argus(
        doc = "Seconds into the animation; without an Animator it advances by speed and may be set to seek (written by skeletal.pose)"
    )]
    pub time: f32,
    /// Playback rate without an animator.
    #[argus(
        doc = "Playback rate without an Animator; 1 is as authored",
        range(0, 10),
        tune
    )]
    pub speed: f32,
    /// Loops without an animator.
    #[argus(doc = "Without an Animator, the animation loops; else it holds its last pose")]
    pub looping: bool,
    /// Crossfade seconds without an animator.
    #[argus(
        doc = "Without an Animator, seconds a change of animation crossfades for; 0 cuts",
        range(0, 10),
        tune
    )]
    pub blend: f32,
    /// World units per skeleton unit.
    #[argus(doc = "World units per skeleton unit (Spine's pixels)", tune)]
    pub scale: f32,
    /// Mirror it.
    #[argus(doc = "Mirror the skeleton horizontally (not a negative Transform scale)")]
    pub flip_x: bool,
    /// Draw order of the back slot.
    #[argus(
        doc = "Draw order of its back slot; each slot in front is 0.001 higher",
        tune
    )]
    pub z: f32,
    /// Draw at all.
    #[argus(doc = "Draw its parts at all")]
    pub visible: bool,
    /// The parts' material.
    #[argus(doc = "Material its parts draw with (as Sprite.material); empty is the plain sprite")]
    pub material: StrId,
    /// The animation shown last tick.
    #[argus(doc = "The animation shown (written by skeletal.pose)")]
    pub shown: StrId,
    /// A one-shot finished.
    #[argus(
        doc = "The animation has played through: looped once, or a one-shot held at its end (written by skeletal.pose)"
    )]
    pub ended: bool,
    /// Crossfading from.
    #[argus(doc = "The animation a crossfade comes from (written by skeletal.pose)")]
    pub from: StrId,
    /// Where it was left.
    #[argus(
        doc = "Seconds into from where the crossfade left it, held (written by skeletal.pose)"
    )]
    pub from_time: f32,
    /// Weight of the old pose.
    #[argus(doc = "Weight of from's pose, 1 fading to 0 (written by skeletal.pose)")]
    pub mix: f32,
    /// Loops seen.
    #[argus(doc = "Times the animation has looped (written by skeletal.pose)")]
    pub loops: u32,
    /// The animator's time last tick.
    #[argus(
        doc = "The Animator's time in its state last tick, to see it enter one (written by skeletal.pose)"
    )]
    pub state_time: f32,
    /// Part entities.
    #[argus(doc = "Slots drawn by part entities (written by skeletal.pose)")]
    pub parts: u32,
}

impl Default for Skeleton {
    fn default() -> Self {
        Skeleton {
            skeleton: StrId::EMPTY,
            skin: StrId::EMPTY,
            animation: StrId::EMPTY,
            time: 0.0,
            speed: 1.0,
            looping: true,
            blend: 0.0,
            scale: 1.0,
            flip_x: false,
            z: 0.0,
            visible: true,
            material: StrId::EMPTY,
            shown: StrId::EMPTY,
            ended: false,
            from: StrId::EMPTY,
            from_time: 0.0,
            mix: 0.0,
            loops: 0,
            state_time: 0.0,
            parts: 0,
        }
    }
}

/// One slot of a skeleton, drawn as a sprite.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A slot of a Skeleton, drawn by this entity's Sprite under it (spawned, placed, and despawned by skeletal.pose)"
)]
pub struct SkeletonPart {
    /// The skeleton's entity.
    #[argus(doc = "The entity whose Skeleton this is part of")]
    pub skeleton: Entity,
    /// The slot.
    #[argus(doc = "The slot it draws, 0 first in the skeleton's setup order")]
    pub slot: u32,
    /// Packed colour last written.
    #[argus(doc = "The colour last written to its Sprite, packed RGBA (written by skeletal.pose)")]
    pub rgba: u32,
}

impl Default for SkeletonPart {
    fn default() -> Self {
        SkeletonPart {
            skeleton: Entity::NULL,
            slot: 0,
            rgba: 0xffff_ffff,
        }
    }
}

/// Register `Skeleton` and `SkeletonPart`.
pub fn register(world: &mut World) -> Result<()> {
    world.register::<Skeleton>()?;
    world.register::<SkeletonPart>()?;
    Ok(())
}

/// Register the components and add `skeletal.pose` to the late stage,
/// after `anim.update`. Install argus-anim first (its store holds the
/// skeletons), and register `Sprite` first for the parts to draw.
pub fn install(
    world: &mut World,
    sched: &mut Scheduler,
    store: Rc<RefCell<AnimStore>>,
) -> Result<()> {
    register(world)?;
    system::install(world, sched, store)
}
