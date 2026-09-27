//! Argus scheduler — phase 0, piece 2 of the Argus Engine build plan.
//!
//! Runs systems over an `argus_ecs::World` in the fixed stage order
//! `input → sim → late → hierarchy → present` (§3), on two clock drivers
//! behind one abstraction (§6): a continuous fixed-rate clock and a discrete
//! command-driven clock. Every system declares its stage, ordering, read and
//! write sets, clock binding, and optional named RNG stream; the scheduler
//! enforces the declarations, seeds each stream from `hash(run_seed, name)`,
//! times every system every tick and counts its allocations for `prof.*` (§5), and catches faults at
//! the system boundary so the world stays intact (§2).
//!
//! `SPEC-TRACE.md` next to this crate maps the spec to code and tests.

#![deny(missing_docs)]
#![warn(clippy::all)]

pub mod alloc;
pub mod clock;
pub mod prof;
pub mod rng;
pub mod scheduler;
pub mod system;

pub use clock::{Clock, ClockKind};
pub use prof::{
    AllocProf, Capture, CapturedSpan, CapturedTick, SystemProf, SystemSample, TickRecord,
};
pub use rng::{stream_seed, Pcg32};
pub use scheduler::{
    FrameReport, Observer, RunUntil, Scheduler, SchedulerConfig, SimSnapshot, SimState, SystemInfo,
};
pub use system::{ClockBinding, Stage, SystemCtx, SystemDesc, SystemFn};

/// Error codes this crate adds to the 4xxx block. See `argus_ecs::codes`.
pub mod codes {
    /// A system with that name is already registered.
    pub const DUPLICATE_SYSTEM: u32 = 4401;
    /// No system with that name (in `after`, `enable`, `disable`, `remove`).
    pub const UNKNOWN_SYSTEM: u32 = 4402;
    /// The `present` stage is read-only; a system there declared writes.
    pub const STAGE_READ_ONLY: u32 = 4403;
    /// `after` constraints form a cycle.
    pub const ORDER_CYCLE: u32 = 4404;
    /// Operation needs the other clock (e.g. `speed` on a discrete scene).
    pub const WRONG_CLOCK: u32 = 4405;
    /// The system asked for randomness without declaring a stream.
    pub const NO_RNG_STREAM: u32 = 4406;
    /// Bad clock spec string.
    pub const BAD_CLOCK_SPEC: u32 = 4407;
    /// `world_mut` from a system not declared exclusive.
    pub const NOT_EXCLUSIVE: u32 = 4408;
}
