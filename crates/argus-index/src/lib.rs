//! The SQLite index, phase 0 piece 6 (§4 "The index"): `.argus/index.db`,
//! updated on load and on every file change, holding entities by scene,
//! components and fields by entity, references between assets, prefabs,
//! defs, and entities, prefab and def catalogs, component schemas,
//! systems, and a full-text column over doc strings.
//!
//! Every `world.*` and `scene.*` read is served from this index when the
//! world is not loaded, and from the live world when it is, with identical
//! result shapes. That is guaranteed by construction: [`query`] is the one
//! evaluator both paths run, over a [`query::Source`] that is either the
//! live world (the daemon's adapter) or [`SceneSource`] here; [`shapes`]
//! builds the `world.get` and `world.summary` results for both.
//!
//! The index is a cache. It is never committed and can be deleted at any
//! time; the next open rebuilds it.

#![warn(missing_docs)]

pub mod index;
pub mod query;
pub mod shapes;
pub mod source;

pub use index::{Index, SyncReport};
pub use query::{Source, DEFAULT_LIMIT};
pub use source::SceneSource;

/// Error codes this crate adds to the 4xxx block.
pub mod codes {
    /// SQLite failed (corrupt cache, locked file). Deleting `.argus/index.db` recovers.
    pub const INDEX: u32 = 4801;
    /// The scene (or entity) is not in the index.
    pub const NOT_INDEXED: u32 = 4802;
}

pub(crate) fn sql_err(e: rusqlite::Error) -> argus_ecs::ArgusError {
    argus_ecs::ArgusError::new(codes::INDEX, format!("index: {e}"))
        .hint("The index is a cache: delete .argus/index.db and reopen the project to rebuild it.")
}
