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
//! in with the game's own bindings.

pub mod app;
pub mod launch;
pub mod link;
pub mod viewport;

pub use app::{Editor, Problem};
