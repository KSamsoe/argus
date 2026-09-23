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

use argus_assets::{AnimatorSpec, Clip, Importer};
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
    #[argus(doc = "The sprite asset whose clips play; empty uses the entity's Sprite.atlas")]
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
    /// Sprites by asset name.
    pub sprites: BTreeMap<String, SpriteClips>,
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
        self.generation += 1;
    }
}

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
