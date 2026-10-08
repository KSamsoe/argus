//! Argus physics, phase 1 item 6 (§12 "Physics, in tiers"): two of the
//! three tiers, sharing the one broadphase.
//!
//! - **Overlap.** An entity with a [`Collider`] and no body: a box or a
//!   circle that reports contacts and nothing else (Swarm's bullets,
//!   Delve's triggers, Outpost's zones). `physics.overlap` emits
//!   `overlap.begin` and `overlap.end` for every pair whose layers agree.
//! - **Kinematic.** A [`Collider`] plus a [`KinematicBody`]: a box moved
//!   against every Tilemap's collision layers and the solid bodies, with
//!   slopes, one-way platforms, moving platforms, and step-up. A
//!   [`CharacterController`] on top turns actions into a platformer's run
//!   and jump, with coyote time, jump buffering, and corner correction as
//!   tune fields. `physics.move` reports landings, walls, ceilings, corner
//!   corrections, and crushes as events.
//! - **Dynamic.** A [`Collider`] plus argus-rigid's `RigidBody`: the
//!   optional rigid-body module (phase 5 item 7), on Rapier, for a game
//!   whose `argus.toml` lists `modules = ["rigid"]`. `physics.rigid` runs
//!   between `physics.move` and `physics.overlap`.
//!
//! The order in a tick: `physics.control` runs first in the sim stage and
//! turns this tick's actions into velocities; gameplay systems then read
//! and adjust them; `physics.move` and `physics.overlap` run last in the
//! stage, so what gameplay set is what moves, and the stage's last word on
//! positions is physics'. A script in the late stage reads the tick's
//! contacts with `q:events("overlap.begin")`.

#![warn(missing_docs)]

pub mod env;
pub mod mover;
mod systems;

use argus_ecs::{Component, Entity, Result, StrId, Vec2, World};
use argus_sim::Scheduler;
use argus_tiles::MapStore;
use std::cell::RefCell;
use std::rc::Rc;

pub use systems::{CONTROL, MOVE, OVERLAP};

/// A shape for the physics tiers, centred on the entity's position plus
/// `offset`. On its own it is the overlap tier.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A box (size) or circle (radius above 0) centred on the Transform plus offset. Alone it reports contacts as overlap.begin and overlap.end events; with a KinematicBody it is the body's shape (always a box)"
)]
pub struct Collider {
    /// Box width and height.
    #[argus(doc = "Box width and height in world units")]
    pub size: Vec2,
    /// Circle radius; above 0 makes the shape a circle.
    #[argus(
        doc = "Circle radius in world units; above 0 the shape is a circle and size is ignored",
        range(0, 100000)
    )]
    pub radius: f32,
    /// Shape centre relative to the position.
    #[argus(doc = "Shape centre relative to the Transform position")]
    pub offset: Vec2,
    /// Layer bits this collider is on.
    #[argus(doc = "Layer bits this collider is on")]
    pub layer: u32,
    /// Layer bits it meets.
    #[argus(
        doc = "Layer bits this collider meets; two colliders meet only when each one's mask has a bit of the other's layer"
    )]
    pub mask: u32,
}

impl Default for Collider {
    fn default() -> Self {
        Collider {
            size: Vec2::new(1.0, 1.0),
            radius: 0.0,
            offset: Vec2::ZERO,
            layer: 1,
            mask: u32::MAX,
        }
    }
}

/// The kinematic tier: a body moved against tiles and solid bodies.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A kinematic body: moved by its velocity against every Tilemap's collision and the solid bodies. An actor (the default) stops at walls, lands on floors, and rides platforms; a solid (a moving platform, a door) goes where its velocity takes it and carries or pushes the actors in its way"
)]
pub struct KinematicBody {
    /// Velocity.
    #[argus(doc = "Velocity in world units per second")]
    pub velocity: Vec2,
    /// Downward acceleration.
    #[argus(
        doc = "Downward acceleration in units per second squared; a CharacterController sets it every tick"
    )]
    pub gravity: f32,
    /// Fastest fall; 0 is no limit.
    #[argus(doc = "Fastest fall in units per second; 0 is no limit")]
    pub max_fall: f32,
    /// A solid instead of an actor.
    #[argus(doc = "A solid: moves freely, blocks actors, carries riders, pushes what it hits")]
    pub solid: bool,
    /// A one-way solid.
    #[argus(doc = "With solid: blocks only from above, a platform to jump up through")]
    pub one_way: bool,
    /// Standing on something.
    #[argus(doc = "Standing on something (written by physics.move)")]
    pub grounded: bool,
    /// The solid it stands on.
    #[argus(doc = "The solid body it stands on, if any (written by physics.move)")]
    pub ground: Entity,
    /// Standing on a one-way platform.
    #[argus(
        doc = "What it stands on is one-way, so it can drop through (written by physics.move)"
    )]
    pub on_one_way: bool,
    /// Rise per unit x of the ground under its centre.
    #[argus(
        doc = "Rise per unit x of the ground under its centre; 0 on flat ground (written by physics.move)"
    )]
    pub slope: f32,
    /// Pressed against a wall this tick.
    #[argus(doc = "Stopped by a wall this tick: -1 left, 1 right, 0 none (written by physics.move)", range(-1, 1))]
    pub wall: i8,
    /// Bonked a ceiling this tick.
    #[argus(doc = "Stopped by a ceiling this tick (written by physics.move)")]
    pub ceiling: bool,
    /// Seconds left of falling through one-way platforms.
    #[argus(
        doc = "Seconds left of falling through one-way platforms (a controller's drop sets it)"
    )]
    pub drop: f32,
}

impl Default for KinematicBody {
    fn default() -> Self {
        KinematicBody {
            velocity: Vec2::ZERO,
            gravity: 0.0,
            max_fall: 0.0,
            solid: false,
            one_way: false,
            grounded: false,
            ground: Entity::NULL,
            on_one_way: false,
            slope: 0.0,
            wall: 0,
            ceiling: false,
            drop: 0.0,
        }
    }
}

/// A platformer's run and jump over a [`KinematicBody`], every feel
/// parameter a tune field (§10: the human tunes it with a slider while
/// playing, and the tuning log records the path).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Turns actions into a platformer's run and jump on the entity's KinematicBody. Reads the Actions component when there is one (move_action, jump_action), else move_x, move_y, and jump as a script wrote them. The jump is set by its height and its time to the apex"
)]
pub struct CharacterController {
    /// Top running speed.
    #[argus(doc = "Top running speed in units per second", range(0, 1000), tune)]
    pub run_speed: f32,
    /// Speeding up on the ground.
    #[argus(
        doc = "Speeding up on the ground, units per second squared",
        range(0, 100000),
        tune
    )]
    pub ground_accel: f32,
    /// Slowing down or turning on the ground.
    #[argus(
        doc = "Slowing down or turning on the ground, units per second squared",
        range(0, 100000),
        tune
    )]
    pub ground_decel: f32,
    /// Speeding up in the air.
    #[argus(
        doc = "Speeding up in the air, units per second squared",
        range(0, 100000),
        tune
    )]
    pub air_accel: f32,
    /// Slowing down or turning in the air.
    #[argus(
        doc = "Slowing down or turning in the air, units per second squared",
        range(0, 100000),
        tune
    )]
    pub air_decel: f32,
    /// Jump height.
    #[argus(doc = "Height of a full jump in world units", range(0, 1000), tune)]
    pub jump_height: f32,
    /// Time to the top of a full jump.
    #[argus(
        doc = "Seconds from take-off to the top of a full jump; with jump_height it sets gravity and take-off speed",
        range(0.01, 10),
        tune
    )]
    pub jump_time: f32,
    /// Gravity multiplier while falling.
    #[argus(
        doc = "Gravity multiplier while falling; above 1 falls faster than it rose",
        range(0, 10),
        tune
    )]
    pub fall_gravity: f32,
    /// Upward speed kept when jump is released early.
    #[argus(
        doc = "Share of upward speed kept when jump is let go early (variable jump height); 1 keeps it all",
        range(0, 1),
        tune
    )]
    pub jump_cut: f32,
    /// Fastest fall.
    #[argus(
        doc = "Fastest fall in units per second; 0 is no limit",
        range(0, 1000),
        tune
    )]
    pub max_fall: f32,
    /// Coyote time.
    #[argus(
        doc = "Seconds after running off a ledge during which a jump still works",
        range(0, 1),
        tune
    )]
    pub coyote_time: f32,
    /// Jump buffering.
    #[argus(
        doc = "Seconds a jump pressed in the air is remembered, so it fires on landing",
        range(0, 1),
        tune
    )]
    pub jump_buffer: f32,
    /// Corner correction.
    #[argus(
        doc = "Up to this far, in world units, a bonked head slides around a ceiling corner and feet that just miss a ledge are lifted onto it",
        range(0, 10),
        tune
    )]
    pub corner_correction: f32,
    /// Step height.
    #[argus(
        doc = "Ledges up to this high are stepped onto without jumping, in world units",
        range(0, 10),
        tune
    )]
    pub step_height: f32,
    /// Share of a platform's velocity kept when jumping off it.
    #[argus(
        doc = "Share of a moving platform's velocity kept when jumping off it",
        range(0, 2),
        tune
    )]
    pub platform_boost: f32,
    /// The action that runs.
    #[argus(doc = "Action map name of the run axis (an axis or a stick); empty means move")]
    pub move_action: StrId,
    /// The action that jumps.
    #[argus(doc = "Action map name of the jump button; empty means jump")]
    pub jump_action: StrId,
    /// Run intent.
    #[argus(doc = "Run intent, -1 to 1 (from the action, or written by a script)", range(-1, 1))]
    pub move_x: f32,
    /// Vertical intent; below -0.5 with a jump press drops through a one-way platform.
    #[argus(doc = "Vertical intent, -1 to 1; holding below -0.5 while pressing jump drops through a one-way platform", range(-1, 1))]
    pub move_y: f32,
    /// Jump held.
    #[argus(doc = "Jump held (from the action, or written by a script)")]
    pub jump: bool,
    /// Coyote time left.
    #[argus(doc = "Seconds of coyote time left")]
    pub coyote: f32,
    /// Buffered jump time left.
    #[argus(doc = "Seconds left of a buffered jump press")]
    pub buffer: f32,
    /// Rising from a jump the player still holds.
    #[argus(doc = "Rising from a jump; letting go now cuts it")]
    pub rising: bool,
    /// Jump held last tick.
    #[argus(doc = "Jump was held last tick (for press and release edges)")]
    pub jump_was: bool,
}

impl Default for CharacterController {
    fn default() -> Self {
        CharacterController {
            run_speed: 7.0,
            ground_accel: 70.0,
            ground_decel: 90.0,
            air_accel: 45.0,
            air_decel: 25.0,
            jump_height: 3.2,
            jump_time: 0.4,
            fall_gravity: 1.7,
            jump_cut: 0.45,
            max_fall: 18.0,
            coyote_time: 0.1,
            jump_buffer: 0.12,
            corner_correction: 0.25,
            step_height: 0.3,
            platform_boost: 1.0,
            move_action: StrId::EMPTY,
            jump_action: StrId::EMPTY,
            move_x: 0.0,
            move_y: 0.0,
            jump: false,
            coyote: 0.0,
            buffer: 0.0,
            rising: false,
            jump_was: false,
        }
    }
}

impl CharacterController {
    /// Gravity while rising: `2 h / t^2`.
    pub fn gravity(&self) -> f32 {
        2.0 * self.jump_height / (self.jump_time * self.jump_time)
    }
    /// Take-off speed: `2 h / t`.
    pub fn takeoff(&self) -> f32 {
        2.0 * self.jump_height / self.jump_time
    }
}

/// Register the physics components (and `Tilemap`, which it reads).
pub fn register(world: &mut World) -> Result<()> {
    // Registering is idempotent: a second call returns the same id.
    world.register::<argus_tiles::Tilemap>()?;
    world.register::<Collider>()?;
    world.register::<KinematicBody>()?;
    world.register::<CharacterController>()?;
    Ok(())
}

/// Register the components and add `physics.control`, `physics.move`,
/// and `physics.overlap` to the sim stage. `tiles` is the session's map
/// store, which the daemon edits through, so a tile edit is ground on the
/// next tick.
pub fn install(
    world: &mut World,
    sched: &mut Scheduler,
    tiles: Rc<RefCell<MapStore>>,
) -> Result<()> {
    install_with(
        world,
        sched,
        tiles,
        Rc::new(RefCell::new(Contacts::default())),
    )
}

/// [`install`], with the pairs `physics.overlap` remembers between ticks
/// held where a keyframe can take and put them back.
pub fn install_with(
    world: &mut World,
    sched: &mut Scheduler,
    tiles: Rc<RefCell<MapStore>>,
    contacts: Rc<RefCell<Contacts>>,
) -> Result<()> {
    register(world)?;
    systems::install(world, sched, tiles, contacts)
}

/// The pairs of colliders that overlapped at the end of the last tick,
/// which the next tick's `overlap.begin` and `overlap.end` are measured
/// against. They are not the world's, so a keyframe keeps them (phase 3
/// item 5: a seek back from a hero standing in a doorway said the doorway
/// was left on the first tick it replayed).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Contacts {
    /// Sorted pairs, the lower entity first.
    pub pairs: Vec<(Entity, Entity)>,
}
