//! The protocol from the outside (§5, §11): what a client needs to reach a
//! daemon and speak to it, and nothing that holds a world. The CLI, the
//! MCP shim, and the editor are built on it; the editor on it alone, so
//! it cannot do anything the CLI can't (P1).
//!
//! - [`socket`]: a project's socket name and `.argus/daemon.json`.
//! - [`Client`]: one connection, one JSON line per message.
//! - [`words`]: the CLI's `key=value` words read into params.
//! - [`command`]: params written back as an `argus` line ("show as
//!   command", §11), which [`words`] reads back to the same params.

pub mod client;
pub mod command;
pub mod socket;
pub mod words;

pub use client::Client;
