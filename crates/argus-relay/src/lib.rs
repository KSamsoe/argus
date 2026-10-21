//! The relay (docs/multiplayer-checklist.md item 2): a small server the
//! players of a match all connect out to, so they reach each other from
//! behind home routers, from a web page or a desktop game alike. A player
//! hosts a room and gets its code; others join with it; the relay passes
//! each one's messages to the rest and keeps nothing.
//!
//! [`Relay`] is the server (`argus relay`); [`Client`] a player's end, an
//! [`argus_net::Link`]. [`ws`] is the WebSocket underneath, which the
//! daemon's browser listener shares.

#![warn(missing_docs)]

pub mod addr;
pub mod client;
pub mod hash;
pub mod server;
pub mod ws;

/// What players and the relay say, which a web page speaks too.
pub use argus_net::relay as proto;

pub use client::Client;
pub use proto::{Down, Up};
pub use server::{Handle, Limits, Relay};
