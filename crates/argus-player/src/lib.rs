//! The Argus player runtime (phase 2 item 7).
//!
//! A game's world is assembled one way ([`assemble`]) for the daemon, the
//! web page, and a release build, so what the agent tests is what the
//! player runs. Around it: [`Player`], a world without the daemon, which the
//! web page and the release build step, feed, and draw; [`keyframe`]s and
//! [`replay`] files, which the daemon's ring and marks share with the
//! release build's recorder; and [`pack`]s, the project as it ships.
//!
//! Nothing here listens on a socket, keeps an index, or writes `.argus/`
//! (§15): the dev surface is the daemon crate, which a release build does
//! not link.

#![warn(missing_docs)]

pub mod assemble;
#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
pub mod desktop;
pub mod keyframe;
pub mod pack;
#[cfg(all(feature = "render", feature = "audio"))]
mod player;
pub mod recorder;
pub mod replay;
pub mod save;
pub mod savegame;
pub mod sha256;

pub use assemble::{
    assemble, set_fonts, with_engine_defs, Assembly, Options, ProjectDefs, UiTexts,
};
pub use pack::{pack, unpack};
#[cfg(all(feature = "render", feature = "audio"))]
pub use player::Player;

/// The speed a `speed.requested` event asks for (`q:speed(x)`, phase 3
/// item 3): ticks a frame, 0 paused.
pub fn requested_speed(fields: &[(String, argus_ecs::Value)]) -> Option<f64> {
    fields
        .iter()
        .find(|(k, _)| k == "x")
        .and_then(|(_, v)| match v {
            argus_ecs::Value::Float(f) => Some(*f),
            argus_ecs::Value::Int(i) => Some(*i as f64),
            argus_ecs::Value::UInt(u) => Some(*u as f64),
            _ => None,
        })
}
