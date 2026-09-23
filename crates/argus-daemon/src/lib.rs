//! The Argus daemon, phase 0 piece 4 (§2 "Process model", §5 "Protocol").
//!
//! One process per project holding exactly one world. Every client reaches
//! it through JSON-RPC 2.0 over a local socket (a Windows named pipe or a
//! Unix domain socket) or over stdio when embedded. "Editing" is the world
//! with the sim stages disabled; "playing" is the same world with them
//! enabled; entering play snapshots and stopping restores (§2).
//!
//! The method set is the phase-0 subset of §5: `session`, `world`, `scene`,
//! `edit`, `sim`, `script`, plus `obs.events`/`obs.subscribe` (the L2 layer
//! of §8, which the phase-0 gate needs) and `prof.*`. [`docs::METHODS`] is
//! the one table the CLI and the MCP tool descriptions are generated from
//! (§16), so a method exists exactly once.
//!
//! Every read obeys the §5 contract: `limit` defaulting to 50, `total`
//! always present, a `next` cursor, compact row arrays. Every error is an
//! [`argus_ecs::ArgusError`] carried unchanged as the JSON-RPC error: code,
//! message, and `data: {hint, ref}`.

#![warn(missing_docs)]

pub mod daemon;
pub mod docs;
pub mod edit;
pub mod history;
pub mod json;
pub mod launch;
pub mod query;
pub mod ring;
pub mod rpc;
pub mod server;

pub use daemon::{ConnId, Daemon};
pub use docs::{MethodDoc, METHODS};
pub use launch::{launch, Launch};
pub use server::{serve, socket_name_for, Transport};

/// Error codes this crate adds to the 4xxx block.
pub mod codes {
    /// A write hit a lock held by another author (§5, §10).
    pub const LOCKED: u32 = 4701;
    /// `edit.*` without an open transaction, or an unknown transaction id.
    pub const NO_TRANSACTION: u32 = 4702;
    /// No project is open in this session.
    pub const NO_PROJECT: u32 = 4703;
    /// Parameters are missing or of the wrong shape.
    pub const BAD_PARAMS: u32 = 4704;
    /// Nothing to undo or redo for this author.
    pub const NOTHING_TO_UNDO: u32 = 4705;
    /// A continuation cursor is not one this daemon issued.
    pub const BAD_CURSOR: u32 = 4706;
    /// The method name is not in the phase-0 set.
    pub const UNKNOWN_METHOD: u32 = 4707;
    /// An `edit.lock` id is unknown or owned by someone else.
    pub const BAD_LOCK: u32 = 4708;
    /// The scenario runner could not run (a broken harness or tests directory), as opposed to a failing test.
    pub const TEST_RUNNER: u32 = 4709;
    /// The renderer's: this machine has no graphics adapter at all, not
    /// even a software one (argus-render's 5001, here for clients that
    /// build without the renderer).
    pub const NO_ADAPTER: u32 = 5001;
}
