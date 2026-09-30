//! The Argus editor (§11): a separate process that speaks only the
//! protocol. "This is a constraint, not a convenience: if the editor ever
//! needs a private channel to the daemon, the protocol is missing something
//! the agent also needs." It depends on `argus-client` and the UI, and on
//! no engine crate (tests/protocol_only.rs), so it cannot do anything the
//! CLI can't (P1).
//!
//! Phase 4 builds it panel by panel (docs/phase4-checklist.md). Item 0 is
//! its frame: the connection, the play controls, the change feed, and
//! "show as command". Item 1 is the viewport: the game's frames, played
//! in with the game's own bindings. Item 2 is the hierarchy and the
//! inspector, with each field's provenance as a colour. Item 3 is the
//! tuning panel: a slider for every tune field, its moves logged. Item 4
//! is the agent panel: its questions, proposals, the person's marks, and
//! what it did. Item 5 is the replay scrubber under the viewport. Item 6
//! is the profiler view, with §17's targets beside what is measured. Item
//! 7 is tilemap painting, in the viewport.

pub mod agent;
pub mod app;
pub mod hierarchy;
pub mod inspector;
pub mod launch;
pub mod link;
pub mod profile;
pub mod scrubber;
pub mod tiles;
pub mod tuning;
pub mod viewport;

pub use app::{Editor, Problem};
