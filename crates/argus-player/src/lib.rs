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
pub mod http;
pub mod keyframe;
pub mod mods;
#[cfg(all(feature = "render", feature = "audio"))]
pub mod netplay;
pub mod pack;
#[cfg(all(feature = "render", feature = "audio"))]
mod player;
pub mod read;
pub mod recorder;
pub mod replay;
pub mod reports;
pub mod save;
pub mod savegame;
pub mod settings;
pub mod sha256;

pub use assemble::{
    assemble, read_timelines, set_fonts, with_engine_defs, Assembly, Options, ProjectDefs,
    ProjectPrefabs, UiTexts,
};
pub use pack::{pack, unpack};
#[cfg(all(feature = "render", feature = "audio"))]
pub use player::Player;

// `q:speed(x)` (phase 3 item 3) is answered by the scheduler at the end of
// the tick that asked (argus-sim), so a host has nothing to do for it.
