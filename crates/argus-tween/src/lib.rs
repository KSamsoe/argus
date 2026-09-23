//! Argus tweens, phase 1 item 9 (§13 "Tweens and sequencing"): a tween is
//! a component.
//!
//! A [`Tween`] on an entity of its own drives one field of another entity
//! (`target`, `path`) from a value to a value over a duration, through an
//! easing curve, after an optional delay, repeating and reversing if asked,
//! and emits its `on_complete` event when it finishes. Squash-and-stretch
//! (`Transform.scale`), UI slides (`Transform.pos`), and camera nudges
//! (`Camera.offset`) are all tweens. Because a tween is an entity with a
//! component, what is animating right now is a query (`world.query {where:
//! {has: ["Tween"]}}`), and because it advances by sim time in the tick,
//! a replay reproduces it exactly.
//!
//! `tween.update` runs in the late stage after the animator and before
//! scripts and the camera. It writes whatever field a tween names, and
//! declares each target component as a write the first time it touches it,
//! so `who_writes Transform` names it.

#![warn(missing_docs)]

pub mod ease;
mod system;

use argus_ecs::{Component, Entity, Result, StrId, Vec2, World};
use argus_sim::Scheduler;

pub use system::SYSTEM;

/// Drives one field of `target` over time.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A tween: drives one field of target (path: Component.field, or Component.vec.x) from from to to over duration with an easing curve, then emits on_complete and despawns (unless keep). A scalar field uses the x of from and to"
)]
pub struct Tween {
    /// The entity whose field moves.
    #[argus(doc = "The entity whose field it drives")]
    pub target: Entity,
    /// The field.
    #[argus(doc = "The field: Component.field (a number or vec2) or Component.vec.x / .y")]
    pub path: StrId,
    /// Start value.
    #[argus(doc = "Start value (x for a number); ignored when from_current")]
    pub from: Vec2,
    /// End value.
    #[argus(doc = "End value (x for a number)")]
    pub to: Vec2,
    /// Start from the field's value when it starts.
    #[argus(doc = "Start from the field's value at the moment the tween starts")]
    pub from_current: bool,
    /// Seconds.
    #[argus(doc = "Seconds from start to end, per play", range(0, 3600), tune)]
    pub duration: f32,
    /// Seconds before it starts.
    #[argus(doc = "Seconds to wait before starting", range(0, 3600), tune)]
    pub delay: f32,
    /// Easing curve.
    #[argus(
        doc = "Easing curve: linear, step, in_/out_/in_out_ quad, cubic, quart, sine, expo, back, elastic, bounce"
    )]
    pub easing: StrId,
    /// Extra plays; -1 forever.
    #[argus(doc = "Plays after the first; -1 repeats forever", range(-1, 1000000))]
    pub repeats: i32,
    /// Alternate direction each play.
    #[argus(doc = "Every other play runs backwards (there and back)")]
    pub yoyo: bool,
    /// Event emitted when it completes.
    #[argus(
        doc = "Event emitted when the last play ends: {entity: target, tween, path}; empty for none"
    )]
    pub on_complete: StrId,
    /// Keep the entity when done.
    #[argus(doc = "Keep the tween's entity when it is done (it is despawned otherwise)")]
    pub keep: bool,
    /// Seconds into the current play.
    #[argus(doc = "Seconds into the current play (written by tween.update)")]
    pub time: f32,
    /// The value last written.
    #[argus(doc = "The value last written to the field (written by tween.update)")]
    pub value: Vec2,
    /// Started (from captured).
    #[argus(doc = "Past its delay, with its start value taken (written by tween.update)")]
    pub started: bool,
    /// Finished.
    #[argus(doc = "Finished (written by tween.update)")]
    pub done: bool,
    /// Plays completed.
    #[argus(doc = "Plays completed (written by tween.update)")]
    pub plays: i32,
}

impl Default for Tween {
    fn default() -> Self {
        Tween {
            target: Entity::NULL,
            path: StrId::EMPTY,
            from: Vec2::ZERO,
            to: Vec2::ZERO,
            from_current: true,
            duration: 0.25,
            delay: 0.0,
            easing: StrId::EMPTY,
            repeats: 0,
            yoyo: false,
            on_complete: StrId::EMPTY,
            keep: false,
            time: 0.0,
            value: Vec2::ZERO,
            started: false,
            done: false,
            plays: 0,
        }
    }
}

/// Register `Tween`.
pub fn register(world: &mut World) -> Result<()> {
    world.register::<Tween>()?;
    Ok(())
}

/// Register `Tween` and add `tween.update` to the late stage.
pub fn install(world: &mut World, sched: &mut Scheduler) -> Result<()> {
    register(world)?;
    system::install(world, sched)
}
