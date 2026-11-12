//! Authored content for the Argus Engine, phase 0 piece 5 (§4 of the design
//! document): the project file, TOML scenes and rooms, one authored entity
//! per file with prefab overrides and per-field provenance (§3), data tables
//! in `defs/`, path references with `asset.move` and the moves log, and bulk
//! data as a text index plus a binary chunk.
//!
//! Files are the source of truth (§4). This crate reads them into typed,
//! validated documents, instantiates them into an [`argus_ecs::World`], and
//! writes overrides back without disturbing anything else in the file. The
//! SQLite index (piece 6) and the daemon (piece 4) sit on top of it.
//!
//! Errors are [`argus_ecs::ArgusError`]s in the 46xx block with `file:line`
//! refs into the TOML, so an agent can fix a scene from the message alone.

#![warn(missing_docs)]

pub mod binary;
pub mod bulk;
pub mod defs;
pub mod doc;
pub mod moves;
pub mod prefab;
pub mod project;
pub mod resolve;
pub mod scene;
pub mod stream;
pub mod tomlv;

pub use bulk::{Bulk, BulkIndex, Rect};
pub use defs::{DefRow, DefSchema, DefTable, Defs};
pub use doc::{ComponentDoc, EntityDoc, FieldDoc};
pub use moves::{MoveKind, MoveRecord, MoveReport};
pub use prefab::PrefabLibrary;
pub use project::{GameInfo, Project, ProjectConfig, NET_DELAY};
pub use resolve::{Provenance, ResolvedComponent, ResolvedEntity, ResolvedField};
pub use scene::{
    EntityFile, LoadReport, Memory, Room, RoomDoc, RoomGraph, Scene, SceneDoc, SeedPolicy,
    StreamState,
};

/// Error codes this crate adds to the 4xxx block.
pub mod codes {
    /// A TOML file did not parse.
    pub const BAD_TOML: u32 = 4601;
    /// A required file or directory is missing (`argus.toml`, `scene.toml`, a chunk).
    pub const MISSING_FILE: u32 = 4602;
    /// A prefab, entity, def, or asset reference points at nothing (§4).
    pub const MISSING_REFERENCE: u32 = 4603;
    /// A prefab inherits from itself, directly or through its bases.
    pub const PREFAB_CYCLE: u32 = 4604;
    /// A scene, room, or entity file has the wrong shape.
    pub const BAD_SCENE: u32 = 4605;
    /// A room name is not in the scene's room graph.
    pub const UNKNOWN_ROOM: u32 = 4606;
    /// A def table has no schema, or a row does not fit its schema.
    pub const BAD_DEF: u32 = 4607;
    /// A bulk chunk does not match its index (size or hash).
    pub const BULK_MISMATCH: u32 = 4608;
    /// An operation that writes files was given a runtime entity.
    pub const NOT_AUTHORED: u32 = 4609;
    /// `asset.move` was given paths it cannot move.
    pub const BAD_MOVE: u32 = 4610;
    /// A scene with open issues cannot be instantiated.
    pub const SCENE_NOT_CLEAN: u32 = 4611;
}
