//! Two clock drivers behind one abstraction (§6 "Two clocks").
//!
//! - **Continuous**: the sim advances at a fixed rate whether or not anything
//!   happened. `speed` is how many sim ticks one render frame owes: 1 is real
//!   time at the configured rate, 3 is 3×, 0 is pause. Swarm and Hollow.
//! - **Discrete**: the sim advances only when a command is issued
//!   (`sim.advance`). Between advances nothing simulates. Delve.
//!
//! Wall-clock time never enters here: `time()` is tick count times timestep.

use argus_ecs::{ArgusError, Result};
use std::fmt;

/// Which driver a clock is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClockKind {
    /// Fixed-rate ticks; render decoupled.
    Continuous,
    /// Ticks only on command.
    Discrete,
}

impl fmt::Display for ClockKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ClockKind::Continuous => "continuous",
            ClockKind::Discrete => "discrete",
        })
    }
}

/// A tick counter with a timestep and, for continuous clocks, a speed.
#[derive(Clone, Debug, PartialEq)]
pub struct Clock {
    /// Driver kind.
    pub kind: ClockKind,
    /// Seconds per tick (continuous) or 1.0 per turn (discrete).
    pub timestep: f64,
    /// Ticks completed.
    pub tick: u64,
    /// Sim ticks per render frame (continuous). 0 pauses.
    pub speed: f64,
    accumulator: f64,
}

impl Clock {
    /// Continuous clock at `hz` ticks per second, speed 1.
    pub fn continuous(hz: f64) -> Clock {
        assert!(hz > 0.0, "clock rate must be positive");
        Clock {
            kind: ClockKind::Continuous,
            timestep: 1.0 / hz,
            tick: 0,
            speed: 1.0,
            accumulator: 0.0,
        }
    }

    /// Discrete clock: one turn per advance, timestep 1.
    pub fn discrete() -> Clock {
        Clock {
            kind: ClockKind::Discrete,
            timestep: 1.0,
            tick: 0,
            speed: 0.0,
            accumulator: 0.0,
        }
    }

    /// Parse a run-identity spec: `continuous@60` or `discrete` (§6).
    pub fn parse(spec: &str) -> Result<Clock> {
        let bad = || {
            ArgusError::new(
                crate::codes::BAD_CLOCK_SPEC,
                format!("bad clock spec {spec:?}"),
            )
            .hint("Use continuous@60 (any positive rate) or discrete.")
        };
        if spec == "discrete" {
            return Ok(Clock::discrete());
        }
        let hz = spec.strip_prefix("continuous@").ok_or_else(bad)?;
        let hz: f64 = hz.parse().map_err(|_| bad())?;
        if hz <= 0.0 || !hz.is_finite() {
            return Err(bad());
        }
        Ok(Clock::continuous(hz))
    }

    /// Ticks per second for a continuous clock.
    pub fn hz(&self) -> f64 {
        1.0 / self.timestep
    }

    /// Sim time: tick count times timestep. Never wall-clock.
    pub fn time(&self) -> f64 {
        self.tick as f64 * self.timestep
    }

    /// Set the speed multiplier. Fractional speeds accumulate across frames.
    pub fn set_speed(&mut self, x: f64) -> Result<()> {
        if self.kind != ClockKind::Continuous {
            return Err(ArgusError::new(
                crate::codes::WRONG_CLOCK,
                "speed applies to the continuous clock; this scene's sim is discrete",
            )
            .hint("Use sim.advance to move a discrete clock."));
        }
        if !x.is_finite() || x < 0.0 {
            return Err(ArgusError::new(
                argus_ecs::codes::OUT_OF_RANGE,
                format!("speed must be a finite number >= 0; got {x}"),
            )
            .hint("0 pauses; 1 is real time; 10 runs ten ticks per frame."));
        }
        self.speed = x;
        if x == 0.0 {
            self.accumulator = 0.0;
        }
        Ok(())
    }

    /// How many ticks the next render frame owes. Continuous only.
    pub(crate) fn ticks_for_frame(&mut self) -> u32 {
        if self.kind != ClockKind::Continuous || self.speed <= 0.0 {
            return 0;
        }
        self.accumulator += self.speed;
        let n = self.accumulator.floor();
        self.accumulator -= n;
        n as u32
    }

    /// How far the next tick is owed, in [0, 1): what a renderer
    /// interpolates by between the last two sim states (section 6).
    /// Always 1 on the discrete clock, where nothing moves between advances.
    pub fn alpha(&self) -> f32 {
        if self.kind != ClockKind::Continuous {
            return 1.0;
        }
        self.accumulator.clamp(0.0, 1.0) as f32
    }

    /// Reset tick count and accumulator, keeping rate and speed.
    pub fn reset(&mut self) {
        self.tick = 0;
        self.accumulator = 0.0;
    }
}

impl fmt::Display for Clock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            ClockKind::Continuous => write!(f, "continuous@{}", self.hz()),
            ClockKind::Discrete => f.write_str("discrete"),
        }
    }
}
