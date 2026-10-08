//! The dynamic physics tier (§12 "Dynamic: real rigid bodies with joints,
//! from an optional Rapier module for the games that need a physics toy,
//! not a default"), phase 5 item 7.
//!
//! An entity with a [`RigidBody`] and a `Collider` (argus-physics: a box,
//! or a circle with a radius) is a rigid body: the physics step moves it
//! under gravity, contacts, and joints, and writes back its `Transform`
//! and its velocity. A [`Joint`] on any entity ties two bodies, or a body
//! to the world: a hinge, a weld, a slider, a rope, or a spring, with
//! limits, a motor, and a force that breaks it. An optional [`RigidWorld`]
//! sets gravity and the solver's iterations.
//!
//! The three tiers are chosen per entity and live in one world (§12): the
//! solid tiles of every Tilemap are fixed colliders here, and the kinematic
//! tier's bodies push rigid bodies (they are moved first, by
//! `physics.move`, and are not pushed back). The shared broadphase indexes
//! rigid bodies like any entity, so `within` and `nearest` find them, and
//! `physics.overlap`, which runs after this tier, reports their overlaps.
//!
//! What gameplay sees: `contact.begin {entity, other, impulse}` and
//! `contact.end {entity, other}` for every pair that starts or stops
//! touching (a rigid body against a body, a kinematic body, or a tilemap),
//! and `joint.broken {joint, a, b}`. A script throws a body by writing its
//! velocity, and teleports it by writing its Transform.
//!
//! The module is optional: a game names it in `argus.toml`
//! (`modules = ["rigid"]`), a build without it does not carry Rapier, and
//! a scene without rigid bodies pays one check a tick. The same run
//! reaches the same world (§6): bodies enter Rapier in entity order,
//! contacts are reported sorted, Rapier's own state (its contact caches
//! and solver warm-start) goes into every keyframe, and its
//! `enhanced-determinism` makes its arithmetic the same on every
//! platform.

#![warn(missing_docs)]

mod store;
mod system;
mod tiles;

pub use store::{RigidState, RigidStore};

use argus_ecs::{Component, Entity, Result, StrId, Vec2, World};
use argus_sim::Scheduler;
use argus_tiles::MapStore;
use std::cell::RefCell;
use std::rc::Rc;

/// The system's name.
pub const RIGID: &str = "physics.rigid";

/// Error codes, the 62xx block.
pub mod codes {
    /// A rigid-body state that does not read back (a replay or report
    /// made by another version of the module).
    pub const BAD_STATE: u32 = 6201;
}

/// A rigid body.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A rigid body (the dynamic tier): moved by the physics step under gravity, contacts, and joints, its shape the entity's Collider (a box, or a circle when its radius is above 0). Dynamic, unless fixed (it never moves) or kinematic (it moves by its velocity, pushes dynamic bodies, and is not pushed). Write velocity to throw it, Transform to teleport it"
)]
pub struct RigidBody {
    /// Velocity.
    #[argus(doc = "Velocity in world units per second (written by physics.rigid)")]
    pub velocity: Vec2,
    /// Angular velocity.
    #[argus(doc = "Spin in radians per second, counter-clockwise (written by physics.rigid)")]
    pub spin: f32,
    /// Downward pull, as a share of the world's gravity.
    #[argus(doc = "Share of the world's gravity it feels; 0 floats", range(-10, 10), tune)]
    pub gravity_scale: f32,
    /// Linear damping.
    #[argus(doc = "How fast it slows by itself, per second", range(0, 100), tune)]
    pub damping: f32,
    /// Angular damping.
    #[argus(
        doc = "How fast its spin slows by itself, per second",
        range(0, 100),
        tune
    )]
    pub angular_damping: f32,
    /// Mass per area.
    #[argus(
        doc = "Mass per unit of area; heavier bodies push lighter ones",
        range(0.001, 1000),
        tune
    )]
    pub density: f32,
    /// Friction.
    #[argus(
        doc = "Friction against what it touches, 0 ice to 1 rubber",
        range(0, 10),
        tune
    )]
    pub friction: f32,
    /// Bounciness.
    #[argus(doc = "Bounciness, 0 a thud to 1 a perfect bounce", range(0, 1), tune)]
    pub restitution: f32,
    /// Never moves.
    #[argus(doc = "A fixed body: never moves; others rest on it")]
    pub fixed: bool,
    /// Moves by its velocity only.
    #[argus(doc = "A kinematic body: moves by its velocity, pushes dynamic bodies, is not pushed")]
    pub kinematic: bool,
    /// No turning.
    #[argus(doc = "Never turns, whatever hits it")]
    pub lock_rotation: bool,
    /// Continuous collision for fast bodies.
    #[argus(doc = "Swept against what it could pass through in one tick (a bullet); costs more")]
    pub ccd: bool,
    /// May sleep at rest.
    #[argus(doc = "May fall asleep at rest, costing nothing until something wakes it")]
    pub can_sleep: bool,
    /// Asleep now.
    #[argus(doc = "Asleep now (written by physics.rigid)")]
    pub sleeping: bool,
}

impl Default for RigidBody {
    fn default() -> Self {
        RigidBody {
            velocity: Vec2::ZERO,
            spin: 0.0,
            gravity_scale: 1.0,
            damping: 0.0,
            angular_damping: 0.0,
            density: 1.0,
            friction: 0.5,
            restitution: 0.0,
            fixed: false,
            kinematic: false,
            lock_rotation: false,
            ccd: false,
            can_sleep: true,
            sleeping: false,
        }
    }
}

/// A joint between two rigid bodies, or a body and the world.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A joint tying rigid body a to rigid body b, or to the world when b is none. kind: revolute (a hinge at the anchors), fixed (welded), prismatic (a slider along axis), rope (at most length apart), spring (pulled towards length). Anchors are in each body's own frame; with no b, anchor_b is a world point"
)]
pub struct Joint {
    /// The first body.
    #[argus(doc = "The first body (an entity with a RigidBody)")]
    pub a: Entity,
    /// The second body, or none.
    #[argus(doc = "The second body; none ties a to the world at anchor_b")]
    pub b: Entity,
    /// What kind.
    #[argus(doc = "revolute, fixed, prismatic, rope, or spring")]
    pub kind: StrId,
    /// Where on a.
    #[argus(doc = "The anchor on a, in a's frame")]
    pub anchor_a: Vec2,
    /// Where on b, or in the world.
    #[argus(doc = "The anchor on b, in b's frame; with no b, a point in the world")]
    pub anchor_b: Vec2,
    /// A slider's direction.
    #[argus(doc = "A prismatic joint's direction of travel, in a's frame")]
    pub axis: Vec2,
    /// Lower limit.
    #[argus(
        doc = "Lower limit: an angle in radians (revolute) or a distance (prismatic); equal to max is no limit"
    )]
    pub min: f32,
    /// Upper limit.
    #[argus(doc = "Upper limit; equal to min is no limit")]
    pub max: f32,
    /// A rope's length, a spring's rest length.
    #[argus(
        doc = "A rope's longest stretch, a spring's rest length",
        range(0, 100000),
        tune
    )]
    pub length: f32,
    /// A spring's stiffness.
    #[argus(doc = "A spring's stiffness", range(0, 1000000), tune)]
    pub stiffness: f32,
    /// A spring's damping.
    #[argus(doc = "A spring's damping", range(0, 100000), tune)]
    pub damping: f32,
    /// A motor's target speed.
    #[argus(doc = "A revolute or prismatic motor's target speed (radians or units per second)", range(-1000, 1000), tune)]
    pub motor_speed: f32,
    /// A motor's strength.
    #[argus(
        doc = "The most force a motor uses; 0 is no motor",
        range(0, 1000000),
        tune
    )]
    pub motor_force: f32,
    /// What breaks it.
    #[argus(
        doc = "The force that breaks the joint; 0 never breaks",
        range(0, 1000000),
        tune
    )]
    pub break_force: f32,
    /// Whether the two bodies collide.
    #[argus(doc = "The two bodies still collide with each other")]
    pub collide: bool,
    /// Broken.
    #[argus(doc = "Broken by break_force (written by physics.rigid); the joint is gone")]
    pub broken: bool,
}

impl Default for Joint {
    fn default() -> Self {
        Joint {
            a: Entity::NULL,
            b: Entity::NULL,
            kind: StrId::EMPTY,
            anchor_a: Vec2::ZERO,
            anchor_b: Vec2::ZERO,
            axis: Vec2::new(1.0, 0.0),
            min: 0.0,
            max: 0.0,
            length: 1.0,
            stiffness: 50.0,
            damping: 2.0,
            motor_speed: 0.0,
            motor_force: 0.0,
            break_force: 0.0,
            collide: false,
            broken: false,
        }
    }
}

/// The dynamic tier's settings, on any one entity.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "The rigid bodies' gravity and solver; one entity's is the world's (the defaults without one)"
)]
pub struct RigidWorld {
    /// Gravity.
    #[argus(doc = "Gravity in world units per second squared", tune)]
    pub gravity: Vec2,
    /// Solver iterations.
    #[argus(
        doc = "Solver iterations a tick: more is stiffer stacks and joints, and costs more",
        range(1, 32),
        tune
    )]
    pub iterations: u8,
}

impl Default for RigidWorld {
    fn default() -> Self {
        RigidWorld {
            gravity: Vec2::new(0.0, -9.81),
            iterations: 4,
        }
    }
}

/// Register the components (and the physics crate's, which it reads).
pub fn register(world: &mut World) -> Result<()> {
    argus_physics::register(world)?;
    world.register::<RigidBody>()?;
    world.register::<Joint>()?;
    world.register::<RigidWorld>()?;
    Ok(())
}

/// Register the components and add `physics.rigid` to the sim stage,
/// between the kinematic move and the overlap pass. `store` is where the
/// physics state lives between ticks, which a keyframe takes and puts
/// back; `tiles` the session's maps, whose solid tiles it collides with.
/// The physics crate's systems must be installed first.
pub fn install(
    world: &mut World,
    sched: &mut Scheduler,
    tiles: Rc<RefCell<MapStore>>,
    store: Rc<RefCell<RigidStore>>,
) -> Result<()> {
    register(world)?;
    system::install(world, sched, tiles, store)
}
