//! Argus jobs, phase 3 item 3 (§12 "Pathfinding and jobs"): "on top of it
//! a job system: entities publish tasks with requirements, agents claim
//! them by priority and distance ... obs.why can explain 'why is this
//! colonist idle' from the job system's declared events."
//!
//! A [`Job`] is a task: a kind, a priority, a skill it needs, a site where
//! the work is done, and optionally a second leg, something carried to
//! somewhere. A game publishes one by spawning it. A [`Worker`] takes the
//! best open job it can do: the highest priority first (its own plus its
//! kind's, a [`JobKind`]), then the nearest, never one it cannot reach
//! over the navigation grid. It walks there with its `NavAgent`, works,
//! carries, and the job is done: `job.done` says so and the job entity
//! goes. A job for one worker only (`only`: eating, sleeping) that
//! outranks what the worker is doing takes it off that job.
//!
//! Everything is in the world: which worker has which job, how far along
//! it is, where the worker stands to do it, when an idle worker looks
//! again. A seek, a fork, a load, or a replay goes the same way.
//!
//! A worker without a job says why in `Worker.idle` and in a `job.idle`
//! event whenever the reason changes ("none of the 4 open jobs is its to
//! do: 3 need cook, 1 is someone else's"); `jobs.why` in the protocol lists
//! every open job with what this worker makes of it ([`why`]).

#![warn(missing_docs)]

mod system;
pub mod why;

pub use system::{spot_near, UPDATE};
pub use why::explain;

use argus_ecs::{Component, Entity, Result, StrId, Vec2, World};
use argus_nav::NavStore;
use argus_sim::Scheduler;
use std::cell::RefCell;
use std::rc::Rc;

/// Error codes this crate adds (56xx).
pub mod codes {
    /// The entity named is not a worker.
    pub const NOT_A_WORKER: u32 = 5601;
}

/// Ticks an idle worker waits before it looks for a job again, when it
/// found none and nothing new was posted.
pub const LOOK_TICKS: u32 = 30;

/// A task to be done.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A task (section 12's job system): a kind, a priority, the skill it needs, a site where it is worked, and optionally something carried somewhere after. A game posts one by spawning it; jobs.update gives it to the best Worker for it, walks the worker there, counts the work, and emits job.done and despawns it when it is finished"
)]
pub struct Job {
    /// The kind.
    #[argus(
        doc = "What kind of job (harvest, haul, cook): its JobKind, if there is one, adds its priority, skill, and work"
    )]
    pub kind: StrId,
    /// How much it matters.
    #[argus(
        doc = "Priority, added to its kind's: a worker takes the highest it can do, the nearest among equals",
        range(-1000, 1000)
    )]
    pub priority: f32,
    /// The skill it needs.
    #[argus(
        doc = "A skill a worker must have (a word in Worker.skills); empty: its kind's, or anyone"
    )]
    pub requires: StrId,
    /// Where it is worked.
    #[argus(doc = "The entity where it is worked; none: the job entity's own place")]
    pub site: Entity,
    /// Where it goes after.
    #[argus(
        doc = "Where the worker carries carry once the work at the site is done; none: done at the site"
    )]
    pub to: Entity,
    /// What goes along.
    #[argus(doc = "An entity carried to to (kept at the worker's place while carried)")]
    pub carry: Entity,
    /// Seconds of work.
    #[argus(
        doc = "Seconds of work at the site at a work rate of 1; 0: its kind's",
        range(0, 100000)
    )]
    pub work: f32,
    /// Seconds done.
    #[argus(doc = "Seconds of work done (written by jobs.update)")]
    pub progress: f32,
    /// Who has it.
    #[argus(doc = "The worker who has it (written by jobs.update)")]
    pub worker: Entity,
    /// Only for one.
    #[argus(
        doc = "Only this worker may take it (a meal, a night's sleep); one that outranks what it is doing takes it off that"
    )]
    pub only: Entity,
    /// Where it stands.
    #[argus(
        doc = "Where the worker stands to do it: beside the site, or the place it carries to (written by jobs.update)"
    )]
    pub spot: Vec2,
    /// How far along.
    #[argus(
        doc = "open, going, working, or carrying (written by jobs.update; empty until it has seen the job)"
    )]
    pub stage: StrId,
}

impl Default for Job {
    fn default() -> Self {
        Job {
            kind: StrId::EMPTY,
            priority: 0.0,
            requires: StrId::EMPTY,
            site: Entity::NULL,
            to: Entity::NULL,
            carry: Entity::NULL,
            work: 0.0,
            progress: 0.0,
            worker: Entity::NULL,
            only: Entity::NULL,
            spot: Vec2::ZERO,
            stage: StrId::EMPTY,
        }
    }
}

/// Takes jobs.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Does jobs (section 12): takes the best open Job it can do, by priority and then distance, and walks to it with its NavAgent. Worker.idle and the job.idle event say why it has none"
)]
pub struct Worker {
    /// What it can do.
    #[argus(doc = "Its skills, words separated by spaces (farm cook); * is every skill")]
    pub skills: StrId,
    /// How fast it works.
    #[argus(doc = "Seconds of work it does a second", range(0, 100), tune)]
    pub rate: f32,
    /// Its job.
    #[argus(doc = "The job it has (written by jobs.update)")]
    pub job: Entity,
    /// Why it has none.
    #[argus(doc = "Why it has no job (written by jobs.update); empty while it has one")]
    pub idle: StrId,
    /// When it looks again.
    #[argus(
        doc = "Ticks until an idle worker looks for a job again (written by jobs.update; a job posted wakes it)"
    )]
    pub look: u32,
}

impl Default for Worker {
    fn default() -> Self {
        Worker {
            skills: StrId::EMPTY,
            rate: 1.0,
            job: Entity::NULL,
            idle: StrId::EMPTY,
            look: 0,
        }
    }
}

/// A kind of job, as the colony weighs it.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A kind of job as the game weighs it (section 10: in Outpost the tune fields are job priorities): the priority, skill, and work every Job of this kind starts from"
)]
pub struct JobKind {
    /// The kind.
    #[argus(doc = "The kind it is for (Job.kind)")]
    pub name: StrId,
    /// Its priority.
    #[argus(
        doc = "Priority of every job of this kind, added to the job's own",
        range(-100, 100),
        tune
    )]
    pub priority: f32,
    /// The skill.
    #[argus(doc = "The skill jobs of this kind need when they name none")]
    pub skill: StrId,
    /// Seconds of work.
    #[argus(
        doc = "Seconds of work of jobs of this kind that name none",
        range(0, 100000),
        tune
    )]
    pub work: f32,
}

impl Default for JobKind {
    fn default() -> Self {
        JobKind {
            name: StrId::EMPTY,
            priority: 0.0,
            skill: StrId::EMPTY,
            work: 1.0,
        }
    }
}

/// Register the components.
pub fn register(world: &mut World) -> Result<()> {
    world.register::<Job>()?;
    world.register::<Worker>()?;
    world.register::<JobKind>()?;
    Ok(())
}

/// Register the components and add `jobs.update` (sim stage). The
/// navigation's components must be registered first (`argus_nav::install`).
pub fn install(world: &mut World, sched: &mut Scheduler, nav: Rc<RefCell<NavStore>>) -> Result<()> {
    register(world)?;
    system::install(world, sched, nav)
}
