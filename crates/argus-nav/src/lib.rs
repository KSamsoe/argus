//! Argus navigation, phase 3 item 2 (§12 "Pathfinding and jobs"): "a
//! navigation grid derived from the tilemap and blocking entities, with A*
//! for individuals and flow fields for crowds, both Rust hot-path systems."
//!
//! A [`NavGrid`] is cells over the world, each blocked or open with a step
//! cost. It covers a tilemap cell for cell (solid tiles blocked), or starts
//! from a baked grid (§4: bulk data as a text index and a binary chunk,
//! [`bake`]), or is a plain rectangle of `fill`; [`NavCost`] entities over
//! it set the cost of the cells under them. The grid is built when it is
//! first needed and again when what it was built from changes.
//!
//! Searches are pure functions of the grid: [`search::astar`] from one cell
//! to another, [`search::flow`] from every cell to one. A [`NavField`] is a
//! flow field toward the entity it is on; a [`NavAgent`] walks its entity
//! along an A* path or down a field (`nav.agents`, sim stage). Everything a
//! path depends on is in the world (an agent keeps the cells its path was
//! found between and the grid's hash), and the [`NavStore`]'s caches are
//! keyed by content, so a seek, a fork, a load, or a replay walks the same
//! way.
//!
//! Scripts ask through `q:path`, `q:walkable`, `q:distance`, and
//! `q:downhill`; the protocol through `nav.*`; a test through
//! `t.expect:path(...):exists()`.

#![warn(missing_docs)]

pub mod bake;
pub mod grid;
pub mod search;
mod store;
mod system;

pub use grid::Grid;
pub use search::{Field, Route};
pub use store::{position, waypoints, Counts, NavStore, Walk};
pub use system::AGENTS;

use argus_ecs::{Component, Entity, Result, StrId, Vec2, World};
use argus_sim::Scheduler;
use std::cell::RefCell;
use std::rc::Rc;

/// Error codes this crate adds (55xx).
pub mod codes {
    /// No navigation grid there, or the entity named is not one.
    pub const NO_GRID: u32 = 5501;
    /// A baked grid could not be read, or does not match its index.
    pub const BAD_BAKE: u32 = 5502;
    /// A point or entity is not on the grid.
    pub const OFF_GRID: u32 = 5503;
}

/// A navigation grid.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A navigation grid (section 12): cells over the world, each blocked or open with a step cost, derived from a tilemap's collision (or a baked grid) and the NavCost entities over it; what A* (NavAgent, nav.path, q:path) and flow fields (NavField) search"
)]
pub struct NavGrid {
    /// Cell (0, 0)'s lower-left corner.
    #[argus(
        doc = "World position of cell (0, 0)'s lower-left corner; rows go up (a grid on a tilemap or a baked grid takes theirs)"
    )]
    pub origin: Vec2,
    /// Cell size.
    #[argus(doc = "World units a cell (a grid on a tilemap takes its tile size)")]
    pub cell: f32,
    /// Width in cells.
    #[argus(doc = "Width in cells (a grid on a tilemap takes its width)")]
    pub width: u32,
    /// Height in cells.
    #[argus(doc = "Height in cells")]
    pub height: u32,
    /// The cost of a cell nothing sets.
    #[argus(
        doc = "Step cost of a cell nothing else sets: 1 open ground, 0 blocked (a map whose open cells are NavCost entities)"
    )]
    pub fill: u8,
    /// Eight-way moves.
    #[argus(doc = "Moves go eight ways, never cutting a blocked corner, instead of four")]
    pub diagonal: bool,
    /// A tilemap to cover.
    #[argus(
        doc = "A Tilemap entity the grid covers cell for cell: solid and sloped tiles are blocked, the rest take fill"
    )]
    pub tilemap: Entity,
    /// A baked grid.
    #[argus(
        doc = "A baked grid (a project-relative *.navgrid.toml, from nav.bake) to start from instead of the tilemap"
    )]
    pub baked: StrId,
}

impl Default for NavGrid {
    fn default() -> Self {
        NavGrid {
            origin: Vec2::ZERO,
            cell: 1.0,
            width: 0,
            height: 0,
            fill: 1,
            diagonal: false,
            tilemap: Entity::NULL,
            baked: StrId::EMPTY,
        }
    }
}

/// What an entity does to the grid under it.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Sets the navigation cost of the grid cells under this entity (section 12's blocking entities): 0 blocks them (a wall, a building), 1 is open ground, more is slower going. It wins over the fill and the tilemap; where two overlap, a block wins, else the higher cost"
)]
pub struct NavCost {
    /// The cost.
    #[argus(doc = "Step cost: 0 blocked, 1 open, up to 255")]
    pub cost: u8,
    /// The box it covers.
    #[argus(
        doc = "World size of the box centred on the entity whose cells (by their centres) it sets; zero: the one cell it stands in"
    )]
    pub size: Vec2,
}

impl Default for NavCost {
    fn default() -> Self {
        NavCost {
            cost: 0,
            size: Vec2::ZERO,
        }
    }
}

/// A flow field toward this entity.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A flow field toward this entity (section 12, for crowds): every grid cell's cost to reach the cell it stands in, found again when it moves to another cell or the grid changes. Any number of NavAgents with field set to it, or scripts asking q:distance and q:downhill, find their way from it in constant time"
)]
pub struct NavField {
    /// The grid.
    #[argus(doc = "The grid; none: the grid under the entity")]
    pub grid: Entity,
    /// How far it reaches.
    #[argus(
        doc = "How far it reaches, in steps of cost 1 (a diagonal is 1.4); 0: the whole grid",
        range(0, 100000)
    )]
    pub range: f32,
}

impl Default for NavField {
    fn default() -> Self {
        NavField {
            grid: Entity::NULL,
            range: 0.0,
        }
    }
}

/// Walks its entity along a path.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Walks this entity on a grid (section 12, for individuals): to goal, to target, or down a NavField. nav.agents finds the path with A* when the goal's cell or the grid changes, moves the entity at speed, and emits nav.arrived and nav.blocked"
)]
pub struct NavAgent {
    /// The grid.
    #[argus(doc = "The grid; none: the grid under the entity")]
    pub grid: Entity,
    /// Where to go.
    #[argus(
        doc = "Where to go, in world units (when target and field are none). It walks there from the first tick, and the default is (0, 0): an agent meant to stand still until it has somewhere to be gets its own position as its goal"
    )]
    pub goal: Vec2,
    /// An entity to go to.
    #[argus(doc = "An entity to go to; the path is found again when it moves to another cell")]
    pub target: Entity,
    /// A field to go down.
    #[argus(
        doc = "A NavField entity to go down instead of a path of its own (a member of a crowd)"
    )]
    pub field: Entity,
    /// Speed.
    #[argus(
        doc = "World units a second; 0: the path is found and followed as the game moves the entity",
        range(0, 1000),
        tune
    )]
    pub speed: f32,
    /// Close enough.
    #[argus(doc = "How close to a waypoint or the goal counts as there, in world units")]
    pub arrive: f32,
    /// What it is doing.
    #[argus(
        doc = "moving, arrived, or blocked (written by nav.agents; empty before its first tick)"
    )]
    pub state: StrId,
    /// The next waypoint.
    #[argus(
        doc = "The next waypoint: the centre of the next cell, or the goal (written by nav.agents)"
    )]
    pub next: Vec2,
    /// The way left.
    #[argus(doc = "Length of the way left, in world units (written by nav.agents)")]
    pub remaining: f32,
    /// The cell the path was found from.
    #[argus(
        doc = "The cell the path was found from (written by nav.agents: with to and grid_hash it finds the same path again after a seek or a load)"
    )]
    pub from: u32,
    /// The cell it was found to.
    #[argus(doc = "The cell the path was found to (written by nav.agents)")]
    pub to: u32,
    /// How far along.
    #[argus(
        doc = "The waypoint it is heading for, counted along the path (written by nav.agents)"
    )]
    pub step: u32,
    /// The grid it was found on.
    #[argus(doc = "The grid's hash when the path was found (written by nav.agents)")]
    pub grid_hash: u32,
}

impl Default for NavAgent {
    fn default() -> Self {
        NavAgent {
            grid: Entity::NULL,
            goal: Vec2::ZERO,
            target: Entity::NULL,
            field: Entity::NULL,
            speed: 4.0,
            arrive: 0.05,
            state: StrId::EMPTY,
            next: Vec2::ZERO,
            remaining: 0.0,
            from: u32::MAX,
            to: u32::MAX,
            step: 0,
            grid_hash: 0,
        }
    }
}

/// Register the components.
pub fn register(world: &mut World) -> Result<()> {
    world.register::<NavGrid>()?;
    world.register::<NavCost>()?;
    world.register::<NavField>()?;
    world.register::<NavAgent>()?;
    Ok(())
}

/// Register the components and add `nav.agents` (sim stage, last).
pub fn install(
    world: &mut World,
    sched: &mut Scheduler,
    store: Rc<RefCell<NavStore>>,
) -> Result<()> {
    register(world)?;
    system::install(world, sched, store)
}
