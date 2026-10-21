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

/// A game's shape, which a relay checks a joining player against: its
/// components and their fields, its actions, and the engine's version. The
/// same game built for the web or the desktop, or open in the daemon, has
/// the same one.
pub fn fingerprint(world: &argus_ecs::World, actions: &[String]) -> u64 {
    let mut h = argus_ecs::intern::text_hash(env!("CARGO_PKG_VERSION"));
    let mix = |h: &mut u64, s: &str| {
        *h = (h.rotate_left(5) ^ argus_ecs::intern::text_hash(s))
            .wrapping_mul(0x0000_0100_0000_01b3);
    };
    let mut names: Vec<String> = world
        .registry()
        .schemas()
        .iter()
        .map(|c| {
            let fields: Vec<String> = c
                .fields
                .iter()
                .map(|f| format!("{}:{:?}", f.name, f.ty))
                .collect();
            format!("{}({})", c.name, fields.join(","))
        })
        .collect();
    names.sort();
    for n in &names {
        mix(&mut h, n);
    }
    for a in actions {
        mix(&mut h, a);
    }
    h
}
