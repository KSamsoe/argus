//! Profiling as a query (§5). The scheduler times every system every tick
//! at low cost; this module keeps a ring of recent ticks and answers
//! `prof.systems`, `prof.worst`, and `prof.frame` from it. Wall-clock
//! durations are recorded here and nowhere else in the sim.

use crate::clock::ClockKind;
use crate::system::Stage;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

/// One tick's timing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickRecord {
    /// Tick number on its clock.
    pub tick: u64,
    /// Which clock ticked.
    pub clock: ClockKind,
    /// Whole tick, including stage overhead and maintenance.
    pub total: Duration,
    /// Per-system durations in run order.
    pub systems: Vec<(Arc<str>, Duration)>,
    /// Entity count per archetype id at the end of the tick.
    pub archetypes: Vec<(u32, usize)>,
}

/// Aggregate for one system over the recent window (`prof.systems`).
#[derive(Clone, Debug, PartialEq)]
pub struct SystemProf {
    /// System name.
    pub name: String,
    /// Stage.
    pub stage: Stage,
    /// Mean per tick, ms.
    pub mean_ms: f64,
    /// 95th percentile per tick, ms.
    pub p95_ms: f64,
    /// Worst tick, ms.
    pub max_ms: f64,
    /// Ticks sampled.
    pub samples: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct Profiler {
    ring: VecDeque<TickRecord>,
    cap: usize,
}

impl Profiler {
    pub fn new(cap: usize) -> Profiler {
        Profiler {
            ring: VecDeque::with_capacity(cap.max(1)),
            cap: cap.max(1),
        }
    }

    pub fn record(&mut self, rec: TickRecord) {
        if self.ring.len() == self.cap {
            self.ring.pop_front();
        }
        self.ring.push_back(rec);
    }

    pub fn last(&self) -> Option<&TickRecord> {
        self.ring.back()
    }

    pub fn len(&self) -> usize {
        self.ring.len()
    }

    /// Slowest `n` recent ticks, slowest first.
    pub fn worst(&self, n: usize) -> Vec<TickRecord> {
        let mut v: Vec<TickRecord> = self.ring.iter().cloned().collect();
        v.sort_by(|a, b| b.total.cmp(&a.total).then(a.tick.cmp(&b.tick)));
        v.truncate(n);
        v
    }

    /// Per-system aggregates over the last `last` ticks, in the given order.
    pub fn systems(&self, last: usize, order: &[(Arc<str>, Stage)]) -> Vec<SystemProf> {
        let window: Vec<&TickRecord> = self.ring.iter().rev().take(last.max(1)).collect();
        order
            .iter()
            .map(|(name, stage)| {
                let mut samples: Vec<f64> = window
                    .iter()
                    .filter_map(|t| {
                        t.systems
                            .iter()
                            .find(|(n, _)| n == name)
                            .map(|(_, d)| d.as_secs_f64() * 1e3)
                    })
                    .collect();
                samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let n = samples.len();
                let (mean, p95, max) = if n == 0 {
                    (0.0, 0.0, 0.0)
                } else {
                    let mean = samples.iter().sum::<f64>() / n as f64;
                    let idx = ((n as f64 * 0.95).ceil() as usize).clamp(1, n) - 1;
                    (mean, samples[idx], samples[n - 1])
                };
                SystemProf {
                    name: name.to_string(),
                    stage: *stage,
                    mean_ms: mean,
                    p95_ms: p95,
                    max_ms: max,
                    samples: n,
                }
            })
            .collect()
    }

    /// Whole-tick p95 over the last `last` ticks, ms.
    pub fn tick_p95_ms(&self, last: usize) -> f64 {
        let mut v: Vec<f64> = self
            .ring
            .iter()
            .rev()
            .take(last.max(1))
            .map(|t| t.total.as_secs_f64() * 1e3)
            .collect();
        if v.is_empty() {
            return 0.0;
        }
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let idx = ((v.len() as f64 * 0.95).ceil() as usize).clamp(1, v.len()) - 1;
        v[idx]
    }
}
