//! Argus matches (docs/multiplayer-checklist.md): two to four players,
//! each owning a slot of the game's input, playing one run.
//!
//! [`Match`] is one player's side: it takes the player's own actions,
//! sends them to the others over a [`Link`], and runs the game a tick at a
//! time through [`Peer`], predicting and rolling back in real time, in
//! lockstep for turns. [`wire`] is what goes between players; [`SimNet`]
//! is a network inside one process, for tests.

#![warn(missing_docs)]

pub mod link;
pub mod lobby;
pub mod relay;
pub mod session;
pub mod wire;

pub use link::{Conditions, Link, SimLink, SimNet};
pub use lobby::{Lobby, Phase, Room, RoomLink, Start, Wire};
pub use session::{Advance, Change, Config, Difference, Match, OutOfStep, Peer, State, Stats};
pub use wire::{Frame, Msg, VERSION};

/// Error codes this crate adds (the 64xx block).
pub mod codes {
    /// A match's players or slot are not ones it can play.
    pub const BAD_CONFIG: u32 = 6401;
    /// A rollback reached a tick with no snapshot kept: an engine bug.
    pub const NO_SNAPSHOT: u32 = 6402;
}
