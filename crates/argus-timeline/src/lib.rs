//! Argus timelines, phase 3 item 5 (§13 "Tweens and sequencing"): "above
//! tweens sits a timeline: a def table of tracks (tweens, animator states,
//! camera cuts, dialogue lines, events) against time, playable from Luau,
//! scrubbable in the editor."
//!
//! The table is `timeline`, declared by the engine so every project has
//! one ([`table::schema`], rows in `defs/timeline.toml`). A row without a
//! `timeline` field is a timeline; a row with one is a cue on it: at a
//! time, of a kind, on a track. A tween cue starts a [`argus_tween::Tween`];
//! an anim cue sends an animator to a state; a camera cue makes a camera
//! draw for a while (the cameras' own `blend` says whether it glides); a
//! line cue starts a conversation from the dialogue table; an event cue
//! makes edits and emits events in the dialogue's small language. A cue
//! may wait: the timeline holds until its conversation ends, its tween
//! finishes, its animator state plays through, or, on an event, until its
//! condition holds.
//!
//! A timeline playing is a [`Timeline`] entity: a script spawns one with
//! its name and the entities its cues call `actor` and `other`
//! (`q:spawn({"Timeline"})`), and `timeline.update` plays it. Everything a
//! timeline is doing is in the world (its time, the next cue, what it waits
//! for, its camera cuts as [`TimelineCut`] entities, what it started marked
//! with [`TimelinePart`]), so sequencing lives in data and not in a
//! script's memory (§19's question on coroutine state), a replay plays it
//! the same, and scrubbing is a seek: the protocol's `timeline.seek` goes
//! to the first frame at which a timeline had reached a time.

#![warn(missing_docs)]

mod system;
pub mod table;

pub use system::UPDATE;
pub use table::{schema, Timelines, TABLE};

use argus_ecs::{Component, Entity, Result, StrId, World};
use argus_sim::Scheduler;
use std::cell::RefCell;
use std::rc::Rc;

/// A timeline playing.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A timeline playing (section 13): the cues of a row of the timeline table fired at their times, its cuts, tweens, conversations, and edits. Its time, next cue, and what it waits for are here; timeline.update emits timeline.started, timeline.cue, timeline.held, timeline.resumed, and timeline.ended, and despawns it when it ends. Set skip to jump to its end"
)]
pub struct Timeline {
    /// Which timeline.
    #[argus(doc = "The timeline it plays: a row of the timeline table (timeline:outro, or outro)")]
    pub name: StrId,
    /// Who `actor` is.
    #[argus(doc = "Who its cues call actor (the default who)")]
    pub actor: Entity,
    /// Who `other` is.
    #[argus(doc = "Who its cues call other")]
    pub other: Entity,
    /// Playback rate.
    #[argus(
        doc = "Playback rate: 1 as authored; 0 stands still",
        range(0, 100),
        tune
    )]
    pub speed: f32,
    /// Stand still.
    #[argus(doc = "Stands still where it is while set")]
    pub paused: bool,
    /// Play again.
    #[argus(doc = "Plays again from the start when it ends, until skipped or despawned")]
    pub looping: bool,
    /// Jump to the end.
    #[argus(
        doc = "Set to jump to its end: tweens finish, animator states are entered, events and edits are made, lines are not said, cuts end; then timeline.ended {skipped: true}"
    )]
    pub skip: bool,
    /// Seconds in.
    #[argus(
        doc = "Seconds into this play; every cue at or before it has fired (written by timeline.update)"
    )]
    pub time: f32,
    /// Its end.
    #[argus(doc = "Its length in seconds (written by timeline.update)")]
    pub length: f32,
    /// Cues fired.
    #[argus(doc = "Cues fired this play, so the next one to fire (written by timeline.update)")]
    pub next: u32,
    /// Waiting.
    #[argus(doc = "Holding for a cue that waits (written by timeline.update)")]
    pub held: bool,
    /// Waiting for.
    #[argus(
        doc = "What it waits for: a conversation, a tween, or an animated entity; none while an event's condition is awaited (written by timeline.update)"
    )]
    pub waiting: Entity,
    /// Started.
    #[argus(doc = "It has begun (written by timeline.update)")]
    pub started: bool,
    /// When.
    #[argus(
        doc = "The tick it began on, where a seek to one of its times starts looking (written by timeline.update)"
    )]
    pub began: u64,
    /// Plays.
    #[argus(doc = "Plays finished, when it loops (written by timeline.update)")]
    pub plays: u32,
}

impl Default for Timeline {
    fn default() -> Self {
        Timeline {
            name: StrId::EMPTY,
            actor: Entity::NULL,
            other: Entity::NULL,
            speed: 1.0,
            paused: false,
            looping: false,
            skip: false,
            time: 0.0,
            length: 0.0,
            next: 0,
            held: false,
            waiting: Entity::NULL,
            started: false,
            began: 0,
            plays: 0,
        }
    }
}

/// What a timeline started: on a tween or a conversation it made.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Marks a tween or a conversation a timeline started (made by timeline.update), so a skip finishes it"
)]
pub struct TimelinePart {
    /// Whose.
    #[argus(doc = "The Timeline that started it")]
    pub timeline: Entity,
    /// Which cue.
    #[argus(doc = "The cue, counted from 0 in time order")]
    pub cue: u32,
}

impl Default for TimelinePart {
    fn default() -> Self {
        TimelinePart {
            timeline: Entity::NULL,
            cue: 0,
        }
    }
}

/// A camera cue's span.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A camera cut a timeline made (made and removed by timeline.update): the camera draws until the timeline's time reaches until, or it ends; then the camera is as active as it was"
)]
pub struct TimelineCut {
    /// Whose.
    #[argus(doc = "The Timeline that cut to it")]
    pub timeline: Entity,
    /// Which cue.
    #[argus(doc = "The cue, counted from 0 in time order")]
    pub cue: u32,
    /// The camera.
    #[argus(doc = "The camera that draws")]
    pub camera: Entity,
    /// Until when.
    #[argus(doc = "The timeline's time the cut ends at; below 0, when the timeline ends")]
    pub until: f32,
    /// Its state before.
    #[argus(doc = "Whether the camera was active before, as it is left after")]
    pub was_active: bool,
}

impl Default for TimelineCut {
    fn default() -> Self {
        TimelineCut {
            timeline: Entity::NULL,
            cue: 0,
            camera: Entity::NULL,
            until: -1.0,
            was_active: false,
        }
    }
}

/// Error codes this crate adds (59xx).
pub mod codes {
    /// A timeline row that does not read.
    pub const BAD_TIMELINE: u32 = 5901;
}

/// Register the components.
pub fn register(world: &mut World) -> Result<()> {
    world.register::<Timeline>()?;
    world.register::<TimelinePart>()?;
    world.register::<TimelineCut>()?;
    Ok(())
}

/// Register the components and add `timeline.update` (first in the late
/// stage, so a cue's tween, animator state, and camera take effect the tick
/// it fires). The tween, animator, camera, and dialogue components must be
/// registered first.
pub fn install(
    world: &mut World,
    sched: &mut Scheduler,
    lines: Rc<RefCell<Timelines>>,
) -> Result<()> {
    register(world)?;
    system::install(world, sched, lines)
}
