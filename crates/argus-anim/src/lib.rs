//! Argus sprite animation, phase 1 item 8 (§13 "Sprite animation").
//!
//! An [`Animator`] on an entity runs a state machine declared in TOML (an
//! animator asset, `type = "animator"`, see `argus_assets::animator`).
//! States play clips of the entity's sprite; transitions are field
//! comparisons, checked in order, optionally only inside a window of the
//! clip's frames, optionally with a crossfade; every state can carry events
//! by frame, which are ordinary engine events (`footstep {entity, state,
//! frame}`) and can ask the mixer for a sound. A state's rate, the length
//! of its wind-up (anticipation), and its hold are numbers or field paths,
//! so a game points them at its own tune fields; `Animator.speed` is one
//! too.
//!
//! `anim.update` runs first in the late stage, after gameplay and physics
//! have moved things, so transitions see this tick's state, and a late
//! script reacts to a frame event the tick it fires (`q:events("hitbox.on")`).
//! It writes the entity's `Sprite.frame` (and the crossfade's
//! `Sprite.blend_frame` and `Sprite.blend`) when the entity has a Sprite.

#![warn(missing_docs)]

mod compile;
mod system;

use argus_assets::sidecar::Direction;
use argus_assets::{AnimatorSpec, Clip, Importer, SkeletonAsset};
use argus_ecs::{Component, Result, StrId, World};
use argus_sim::Scheduler;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

pub use system::SYSTEM;

/// Runs an animator state machine on its entity.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Runs an animator state machine (an assets/*.toml with type = \"animator\") on the entity's sprite: the state, its clip's frame, events by frame. Set play to a state name to enter it"
)]
pub struct Animator {
    /// The animator asset.
    #[argus(doc = "The animator asset's name (assets/anim/hero.toml is anim/hero)")]
    pub machine: StrId,
    /// Enter this state next tick.
    #[argus(doc = "Set to a state's name to enter it on the next tick; cleared when entered")]
    pub play: StrId,
    /// Playback rate.
    #[argus(
        doc = "Playback rate for every clip; 1 is as authored",
        range(0, 10),
        tune
    )]
    pub speed: f32,
    /// Whose clips play.
    #[argus(
        doc = "The sprite asset whose clips play; empty uses the entity's Skeleton.skeleton, else its Sprite.atlas"
    )]
    pub sprite: StrId,
    /// The current state.
    #[argus(doc = "The current state (written by anim.update)")]
    pub state: StrId,
    /// Seconds in the state.
    #[argus(doc = "Seconds in the current state (written by anim.update)")]
    pub time: f32,
    /// Position in the clip.
    #[argus(doc = "Position in the state's clip, 0 first (written by anim.update)")]
    pub frame: u32,
    /// Times the clip looped.
    #[argus(doc = "Times the clip has looped (written by anim.update)")]
    pub loops: u32,
    /// The clip has played through once.
    #[argus(
        doc = "The clip has played through once: a looping clip has wrapped, a one-shot has finished (written by anim.update)"
    )]
    pub ended: bool,
    /// Seconds into the current frame.
    #[argus(doc = "Seconds into the current frame (written by anim.update)")]
    pub phase: f32,
    /// Crossfade seconds left.
    #[argus(doc = "Seconds of crossfade left (written by anim.update)")]
    pub blend_left: f32,
    /// Crossfade length.
    #[argus(doc = "Length of the crossfade in progress (written by anim.update)")]
    pub blend_total: f32,
    /// Seconds the current frame lasts.
    #[argus(
        doc = "Seconds the current frame lasts, its wind-up or hold included: phase / frame_len is how far through it the clip is (written by anim.update)"
    )]
    pub frame_len: f32,
}

impl Default for Animator {
    fn default() -> Self {
        Animator {
            machine: StrId::EMPTY,
            play: StrId::EMPTY,
            speed: 1.0,
            sprite: StrId::EMPTY,
            state: StrId::EMPTY,
            time: 0.0,
            frame: 0,
            loops: 0,
            ended: false,
            phase: 0.0,
            blend_left: 0.0,
            blend_total: 0.0,
            frame_len: 0.0,
        }
    }
}

/// A sprite's clips and frame durations, as the animator plays them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpriteClips {
    /// Clips by name.
    pub clips: BTreeMap<String, Clip>,
    /// Each frame's duration in ms, for clips without an fps.
    pub frame_ms: Vec<u32>,
}

/// The machines and sprite clips animators play, from the importer. The
/// daemon refreshes it after every import, so a changed animator file or
/// sprite sheet takes effect on the next tick.
#[derive(Clone, Debug, Default)]
pub struct AnimStore {
    /// Machines by asset name.
    pub machines: BTreeMap<String, AnimatorSpec>,
    /// Sprites by asset name. A Spine skeleton is one too (phase 5 item
    /// 8): each of its animations a clip of [`skeleton_frames`] frames at
    /// the rate that plays it in its own length, so an animator plays a
    /// skeleton as it plays a sheet, and argus-skeletal poses it at the
    /// animator's frame.
    pub sprites: BTreeMap<String, SpriteClips>,
    /// Spine skeletons by asset name (phase 5 item 8), for argus-skeletal.
    pub skeletons: BTreeMap<String, SkeletonAsset>,
    /// Bumped on every change.
    pub generation: u64,
}

impl AnimStore {
    /// Take every animator and sprite from an importer. A machine whose
    /// file now fails to import keeps its last good version, so a typo
    /// while editing does not freeze every character that uses it.
    pub fn set_assets(&mut self, assets: &Importer) {
        let mut machines: BTreeMap<String, AnimatorSpec> = assets
            .animators()
            .map(|(n, a)| (n.clone(), a.clone()))
            .collect();
        for (name, old) in std::mem::take(&mut self.machines) {
            if assets.failures().contains_key(&name) {
                machines.entry(name).or_insert(old);
            }
        }
        self.machines = machines;
        self.sprites = assets
            .sprites()
            .map(|(n, s)| {
                (
                    n.clone(),
                    SpriteClips {
                        clips: s.clips.clone(),
                        frame_ms: s.frames.iter().map(|f| f.duration_ms).collect(),
                    },
                )
            })
            .collect();
        let mut skeletons: BTreeMap<String, SkeletonAsset> = assets
            .skeletons()
            .map(|(n, s)| (n.clone(), s.clone()))
            .collect();
        for (name, old) in std::mem::take(&mut self.skeletons) {
            if assets.failures().contains_key(&name) {
                skeletons.entry(name).or_insert(old);
            }
        }
        for (name, sk) in &skeletons {
            let mut clips = SpriteClips::default();
            for (anim, a) in &sk.animations {
                let n = skeleton_frames(a.duration, sk.fps);
                let fps = if a.duration > 0.0 {
                    n as f32 / a.duration
                } else {
                    sk.fps
                };
                clips.clips.insert(
                    anim.clone(),
                    Clip {
                        frames: (0..n).collect(),
                        fps: Some(fps),
                        looping: true,
                        direction: Direction::Forward,
                    },
                );
            }
            self.sprites.insert(name.clone(), clips);
        }
        self.skeletons = skeletons;
        self.generation += 1;
    }
}

/// How many frames an animator plays a skeleton's animation in: its
/// length at the skeleton's frame rate, rounded up, at least one and at
/// most [`MAX_SKELETON_FRAMES`] (a longer one plays in coarser frames).
/// The last frame ends on the animation's last key.
pub fn skeleton_frames(duration: f32, fps: f32) -> u32 {
    let n = ((duration * fps.max(1.0)) - 1e-3).ceil();
    if n.is_nan() {
        return 1;
    }
    n.clamp(1.0, MAX_SKELETON_FRAMES as f32) as u32
}

/// The most frames a skeleton's clip has: 36 minutes at 30 a second.
pub const MAX_SKELETON_FRAMES: u32 = 1 << 16;

/// Register `Animator`.
pub fn register(world: &mut World) -> Result<()> {
    world.register::<Animator>()?;
    Ok(())
}

/// Register `Animator` and add `anim.update` to the late stage. Register
/// `Sprite` first (the renderer does) for the frames to reach it.
pub fn install(
    world: &mut World,
    sched: &mut Scheduler,
    store: Rc<RefCell<AnimStore>>,
) -> Result<()> {
    register(world)?;
    system::install(world, sched, store)
}
