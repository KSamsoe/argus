//! Argus ECS core — phase 0, piece 1 of the Argus Engine build plan.
//!
//! This crate is the data layer everything else in Argus sits on: an archetype
//! ECS with structure-of-arrays columns, introspectable component schemas
//! (declared from Rust via `#[derive(Component)]` or built at runtime for the
//! Luau path), a first-class authored-vs-runtime entity split with stable
//! string ids, a transform hierarchy, declared read/write access for queries,
//! end-of-frame maintenance that nulls dangling entity references, and
//! memcpy-cheap snapshots that also serve as forks.
//!
//! Design references are section numbers of the Argus Engine design document
//! (`docs/argus-engine-design-v0.3.html`): §2 process model, §3 data model,
//! §5 field paths and error shape, §6 snapshots, §7 access rules, §17 targets.
//! `SPEC-TRACE.md` next to this crate maps every sentence of the spec that
//! touches the ECS core to the code and the test that covers it.
//!
//! What is deliberately *not* here (next pieces of phase 0): the stage
//! scheduler and clocks, the Luau binding, TOML scene loading and prefab
//! overrides, the SQLite index, the JSON-RPC daemon, action maps, profiling,
//! the CLI, and the scenario test runner. Each of those consumes this crate;
//! none of them needs to change it.

#![deny(missing_docs)]
#![warn(clippy::all)]

extern crate self as argus_ecs;

pub mod archetype;
pub mod codec;
pub mod column;
pub mod entity;
pub mod error;
pub mod event;
pub mod hierarchy;
pub mod intern;
mod persist;
pub mod query;
pub mod schema;
pub mod snapshot;
pub mod spatial;
pub mod text;
pub mod value;
pub mod world;

pub use archetype::{Archetype, ArchetypeId, ArchetypeStat};
pub use column::{ColMut, ColRef, Column};
pub use entity::Entity;
pub use error::{codes, ArgusError, Result};
pub use event::{Event, EventRecord};
pub use hierarchy::{propagate, GlobalTransform, Parent, Transform};
pub use intern::{Interner, StrId};
pub use persist::{Migrate, Migrated, Row, StateReport};
pub use query::{Access, ArchetypeView, DynColumn, Fetch, Query};
pub use schema::{
    Component, ComponentId, ComponentKind, ComponentSchema, DefRef, FieldSchema, FieldValue,
    Registry, SchemaBuilder,
};
pub use snapshot::Snapshot;
pub use spatial::SpatialHash;
pub use value::{FieldType, Value, Vec2};
pub use world::{World, WorldSummary};

/// Re-export of the derive macro so users only depend on `argus-ecs`.
pub use argus_ecs_derive::Component;

/// Items the derive macro needs in scope; not part of the public API.
#[doc(hidden)]
pub mod __private {
    pub use crate::schema::{ComponentSchema, FieldSchema, FieldValue};
    pub use crate::value::{FieldType, Value};
    pub use core::mem::{align_of, offset_of, size_of};
}
