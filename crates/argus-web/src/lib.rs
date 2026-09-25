//! The Argus web player, phase 2 item 1 (the web spike).
//!
//! A [`Player`] is a project's world assembled the way the daemon assembles
//! it, without the daemon: no socket, no index, no history, no window host.
//! The page steps it, feeds it keys, and draws it into a canvas. It builds
//! natively too, so the same code runs on both sides of the determinism
//! question (§6: a run is reproducible on the same platform and build; is it
//! across the desktop and the browser?).
//!
//! A project reaches the browser as a pack ([`pack`], [`unpack`]): every file
//! of it but `.argus/`, mounted in memory through `argus-os`, so the loaders
//! read it as they read a directory.
//!
//! The player and the packs live in `argus-player` (phase 2 item 7), whose
//! assembly the daemon shares; this crate is the page's side of them.

#![warn(missing_docs)]

pub use argus_player::{pack, unpack, Player};

#[cfg(target_arch = "wasm32")]
mod wasi;
#[cfg(target_arch = "wasm32")]
pub mod web;
