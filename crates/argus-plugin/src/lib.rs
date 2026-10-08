//! Rust systems for an Argus game (§7: hot-path systems "are Rust, either
//! in the engine or in a project plugin crate, and are compiled to a
//! dynamic library the daemon loads").
//!
//! A plugin is a crate in the project's `plugins/`, named in `argus.toml`
//! (`plugins = ["swarm-hot"]`), built as a `cdylib`. The daemon builds it
//! with cargo, loads it, checks it against the game, and runs its systems
//! in the scheduler beside the Luau ones; `script.reload` builds it again
//! and swaps it in without losing a frame. A compile error is a
//! `script.errors` row with its file and line; a panic stops only the
//! system that panicked.
//!
//! ```toml
//! # plugins/swarm-hot/Cargo.toml
//! [package]
//! name = "swarm-hot"
//! version = "0.1.0"
//! edition = "2021"
//!
//! [lib]
//! crate-type = ["cdylib"]
//!
//! [dependencies]
//! argus-plugin = { path = "<the engine>/crates/argus-plugin" }
//!
//! [workspace]
//! ```
//!
//! ```
//! use argus_plugin::prelude::*;
//!
//! component! {
//!     /// components/swarm.luau's Enemy, field for field.
//!     pub struct Enemy { pub speed: f32, pub damage: i32, pub wave: u32 }
//! }
//!
//! fn chase(ctx: &Ctx) -> Result {
//!     let dt = ctx.dt();
//!     ctx.each::<(&Enemy, &mut Transform)>(|_, (en, t)| {
//!         t.pos.x += en.speed * dt as f32;
//!     })
//! }
//!
//! plugin! {
//!     components: [Enemy, Transform],
//!     systems: [
//!         System::new("sim.chase", "sim")
//!             .reads(&["Enemy", "Transform"])
//!             .writes(&["Transform"])
//!             .run(chase),
//!     ],
//! }
//! ```
//!
//! The two rules of §7 hold as for Luau. A system touches only what it
//! declared: every fetch is checked against its declaration, and one
//! outside it stops the system. And a system holds no references across
//! frames: everything a [`Ctx`] gives lives as long as the run.

#![warn(missing_docs)]

pub mod abi;
pub mod ctx;
pub mod decl;
pub mod view;

pub use ctx::{Ctx, Entity, Error, Query, Ref, Result, Rng, StrId, Value, Vec2};
pub use decl::{Layout, System, SystemFn};
pub use view::engine::{GlobalTransform, Transform};
pub use view::{Fetch, FieldTy, One, View};

/// Everything a plugin uses.
pub mod prelude {
    pub use crate::ctx::{Ctx, Entity, Error, Ref, Result, StrId, Value, Vec2};
    pub use crate::decl::System;
    pub use crate::view::engine::{GlobalTransform, Transform};
    pub use crate::{component, plugin};
}

#[cfg(test)]
mod tests {
    use super::*;

    component! {
        struct Probe { a: u8, b: f32, c: Vec2, d: Entity, e: bool, f: StrId }
    }

    #[test]
    fn the_compile_time_fingerprint_is_the_abi_one() {
        let at_compile = <Probe as View>::FINGERPRINT;
        let at_run = abi::fingerprint(
            core::mem::size_of::<Probe>(),
            core::mem::align_of::<Probe>(),
            Probe::FIELDS.iter().map(|(n, t, o)| (*n, *t, *o)),
        );
        assert_eq!(at_compile, at_run);
        assert_eq!(
            Probe::FIELDS,
            &[
                ("a", abi::ty::U8, 0),
                ("b", abi::ty::F32, 4),
                ("c", abi::ty::VEC2, 8),
                ("d", abi::ty::ENTITY, 16),
                ("e", abi::ty::BOOL, 24),
                ("f", abi::ty::STR, 28),
            ]
        );
        assert_eq!(core::mem::size_of::<Probe>(), 32);
    }
}
