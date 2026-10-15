//! Action maps and the input log, phase 0 piece 7 (§12 "Input actions",
//! §6 "the input log records actions", §15 "inputs arrive only as
//! per-player-slot actions").
//!
//! Gameplay never sees keys. `input.toml` declares actions (`move`, `jump`,
//! `confirm`) with bindings per device and per player slot. The engine
//! turns the map into one component, `Actions`, with a field per action
//! (buttons also get `_pressed` and `_released` edge fields), written every
//! tick onto every entity that has a [`PlayerSlot`] by the `input.actions`
//! system in the `input` stage. Gameplay systems read `Actions` like any
//! other component, from Rust or Luau, and never learn where a value came
//! from.
//!
//! Phase 0 has no devices: values come from `input.inject`, `input.script`,
//! or a replayed log. Every value change is a log entry keyed by tick (a
//! continuous run) or turn (a discrete run), so a run is reproducible from
//! scene, seed, code, and this log (§6), and a replay survives a rebind.

#![warn(missing_docs)]

pub mod devices;
pub mod glyphs;
pub mod map;
pub mod rebind;
pub mod state;
pub mod system;
pub mod touch;

pub use devices::{key_name, DeviceInput, Devices};
pub use glyphs::{Family, InputGlyph, GLYPHS};
pub use map::{Action, ActionKind, ActionMap, Slot};
pub use rebind::{InputBinding, InputToggle, REBIND};
pub use state::{
    ActionValue, Command, InjectReport, InputSnapshot, InputState, JournalEntry, Listen, LogEntry,
    Message, Rebound, MAX_CHANNEL, MAX_MESSAGE,
};
pub use system::{install, register, PlayerSlot, ACTIONS, MESSAGE_EVENT, SYSTEM};
pub use touch::{TouchKind, TouchPhase, TouchShown};

/// Error codes this crate adds to the 4xxx block.
pub mod codes {
    /// The project has no `input.toml`, so there are no actions to inject.
    pub const NO_INPUT_MAP: u32 = 4901;
    /// No action with that name.
    pub const UNKNOWN_ACTION: u32 = 4902;
    /// A value of the wrong shape for the action's kind.
    pub const BAD_VALUE: u32 = 4903;
    /// A player slot the map does not declare.
    pub const BAD_SLOT: u32 = 4904;
    /// `input.toml` is malformed.
    pub const BAD_MAP: u32 = 4905;
    /// An input log could not be read.
    pub const BAD_LOG: u32 = 4906;
}
