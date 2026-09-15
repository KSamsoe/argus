//! Profiling as a query (§5). The scheduler times every system every tick
//! at low cost and, when the counting allocator is installed, counts its
//! allocations; this module keeps a ring of recent ticks and answers
//! `prof.systems`, `prof.worst`, `prof.frame`, and `prof.alloc` from it,
//! and holds a `Capture` while `prof.capture` is collecting ticks for a
//! trace file. Wall-clock durations are recorded here and nowhere else in
//! the sim.

use crate::clock::ClockKind;
use crate::system::Stage;
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// One system's sample within a tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemSample {
    /// System name (or `hierarchy.propagate`).
    pub name: Arc<str>,
    /// Wall time spent in the system.
    pub elapsed: Duration,
    /// Heap allocations made while it ran (0 when not counted).
    pub allocs: u64,
    /// Bytes requested by those allocations.
    pub bytes: u64,
}

/// One tick's timing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickRecord {
    /// Tick number on its clock.
    pub tick: u64,
    /// Which clock ticked.
    pub clock: ClockKind,
    /// Whole tick, including stage overhead and maintenance.
    pub total: Duration,
    /// Per-system samples in run order.
    pub systems: Vec<SystemSample>,
    /// Allocations over the whole tick.
    pub allocs: u64,
    /// Bytes requested over the whole tick.
    pub bytes: u64,
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

/// Allocation aggregate for one system over the recent window (`prof.alloc`).
#[derive(Clone, Debug, PartialEq)]
pub struct AllocProf {
    /// System name.
    pub name: String,
    /// Stage.
    pub stage: Stage,
    /// Mean allocations per tick.
    pub mean_allocs: f64,
    /// Mean bytes per tick.
    pub mean_bytes: f64,
    /// Most allocations in one tick.
    pub max_allocs: u64,
    /// Most bytes in one tick.
    pub max_bytes: u64,
    /// Allocations summed over the window.
    pub total_allocs: u64,
    /// Bytes summed over the window.
    pub total_bytes: u64,
    /// Ticks sampled.
    pub samples: usize,
}

/// One system span inside a captured tick, in microseconds from the start
/// of the capture.
#[derive(Clone, Debug, PartialEq)]
pub struct CapturedSpan {
    /// System name.
    pub name: Arc<str>,
    /// Start, µs from capture start.
    pub start_us: f64,
    /// Duration, µs.
    pub dur_us: f64,
    /// Allocations while it ran.
    pub allocs: u64,
    /// Bytes while it ran.
    pub bytes: u64,
}

/// One tick of a capture.
#[derive(Clone, Debug, PartialEq)]
pub struct CapturedTick {
    /// Tick number.
    pub tick: u64,
    /// Clock.
    pub clock: ClockKind,
    /// Start, µs from capture start.
    pub start_us: f64,
    /// Duration, µs.
    pub dur_us: f64,
    /// Live entities at the end of the tick.
    pub entities: usize,
    /// Allocations over the tick.
    pub allocs: u64,
    /// Bytes over the tick.
    pub bytes: u64,
    /// System spans in run order.
    pub systems: Vec<CapturedSpan>,
}

/// A `prof.capture {ticks}` in progress or complete: the next `wanted`
/// ticks with per-system spans on a common time base, ready to write as a
/// trace.
#[derive(Clone, Debug)]
pub struct Capture {
    origin: Instant,
    /// Ticks asked for.
    pub wanted: usize,
    /// Ticks collected so far.
    pub ticks: Vec<CapturedTick>,
}

impl Capture {
    pub(crate) fn new(wanted: usize) -> Capture {
        Capture {
            origin: Instant::now(),
            wanted: wanted.max(1),
            ticks: Vec::with_capacity(wanted.max(1)),
        }
    }

    /// Microseconds since the capture began.
    pub(crate) fn us(&self, at: Instant) -> f64 {
        at.saturating_duration_since(self.origin).as_secs_f64() * 1e6
    }

    /// True once every wanted tick is in.
    pub fn complete(&self) -> bool {
        self.ticks.len() >= self.wanted
    }

    /// The capture as a Chrome trace event file (the JSON format Tracy's
    /// `import-chrome` tool converts to `.tracy`, and Perfetto and
    /// `chrome://tracing` open directly). One process, one thread: a
    /// complete (`X`) event per tick and, nested inside it, one per system,
    /// each carrying its allocation counts as args.
    pub fn chrome_trace(&self) -> String {
        let mut out = String::with_capacity(256 + self.ticks.len() * 512);
        out.push_str("{\"displayTimeUnit\":\"ms\",\"traceEvents\":[\n");
        out.push_str(
            "{\"ph\":\"M\",\"name\":\"process_name\",\"pid\":1,\"tid\":1,\"args\":{\"name\":\"argus\"}},\n",
        );
        out.push_str(
            "{\"ph\":\"M\",\"name\":\"thread_name\",\"pid\":1,\"tid\":1,\"args\":{\"name\":\"sim\"}}",
        );
        for t in &self.ticks {
            let _ = write!(
                out,
                ",\n{{\"ph\":\"X\",\"cat\":\"tick\",\"name\":\"tick {}\",\"pid\":1,\"tid\":1,\"ts\":{:.3},\"dur\":{:.3},\"args\":{{\"tick\":{},\"clock\":\"{}\",\"entities\":{},\"allocs\":{},\"bytes\":{}}}}}",
                t.tick, t.start_us, t.dur_us, t.tick, t.clock, t.entities, t.allocs, t.bytes
            );
            for s in &t.systems {
                let _ = write!(
                    out,
                    ",\n{{\"ph\":\"X\",\"cat\":\"system\",\"name\":\"{}\",\"pid\":1,\"tid\":1,\"ts\":{:.3},\"dur\":{:.3},\"args\":{{\"tick\":{},\"allocs\":{},\"bytes\":{}}}}}",
                    json_escape(&s.name),
                    s.start_us,
                    s.dur_us,
                    t.tick,
                    s.allocs,
                    s.bytes
                );
            }
        }
        out.push_str("\n]}\n");
        out
    }

    /// Write the trace to `path`, creating parent directories.
    pub fn write(&self, path: &std::path::Path) -> std::io::Result<usize> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = self.chrome_trace();
        std::fs::write(path, &text)?;
        Ok(text.len())
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

#[derive(Clone, Debug)]
pub(crate) struct Profiler {
    ring: VecDeque<TickRecord>,
    cap: usize,
}

fn p95(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    if n == 0 {
        return 0.0;
    }
    let idx = ((n as f64 * 0.95).ceil() as usize).clamp(1, n) - 1;
    sorted[idx]
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

    fn window(&self, last: usize) -> Vec<&TickRecord> {
        self.ring.iter().rev().take(last.max(1)).collect()
    }

    /// Per-system aggregates over the last `last` ticks, in the given order.
    pub fn systems(&self, last: usize, order: &[(Arc<str>, Stage)]) -> Vec<SystemProf> {
        let window = self.window(last);
        order
            .iter()
            .map(|(name, stage)| {
                let mut samples: Vec<f64> = window
                    .iter()
                    .filter_map(|t| {
                        t.systems
                            .iter()
                            .find(|s| &s.name == name)
                            .map(|s| s.elapsed.as_secs_f64() * 1e3)
                    })
                    .collect();
                samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let n = samples.len();
                let mean = if n == 0 {
                    0.0
                } else {
                    samples.iter().sum::<f64>() / n as f64
                };
                SystemProf {
                    name: name.to_string(),
                    stage: *stage,
                    mean_ms: mean,
                    p95_ms: p95(&samples),
                    max_ms: samples.last().copied().unwrap_or(0.0),
                    samples: n,
                }
            })
            .collect()
    }

    /// Per-system allocation aggregates over the last `last` ticks.
    pub fn alloc(&self, last: usize, order: &[(Arc<str>, Stage)]) -> Vec<AllocProf> {
        let window = self.window(last);
        order
            .iter()
            .map(|(name, stage)| {
                let samples: Vec<(u64, u64)> = window
                    .iter()
                    .filter_map(|t| {
                        t.systems
                            .iter()
                            .find(|s| &s.name == name)
                            .map(|s| (s.allocs, s.bytes))
                    })
                    .collect();
                let n = samples.len();
                let total_allocs: u64 = samples.iter().map(|s| s.0).sum();
                let total_bytes: u64 = samples.iter().map(|s| s.1).sum();
                AllocProf {
                    name: name.to_string(),
                    stage: *stage,
                    mean_allocs: if n == 0 {
                        0.0
                    } else {
                        total_allocs as f64 / n as f64
                    },
                    mean_bytes: if n == 0 {
                        0.0
                    } else {
                        total_bytes as f64 / n as f64
                    },
                    max_allocs: samples.iter().map(|s| s.0).max().unwrap_or(0),
                    max_bytes: samples.iter().map(|s| s.1).max().unwrap_or(0),
                    total_allocs,
                    total_bytes,
                    samples: n,
                }
            })
            .collect()
    }

    /// Whole-tick p95 over the last `last` ticks, ms.
    pub fn tick_p95_ms(&self, last: usize) -> f64 {
        let mut v: Vec<f64> = self
            .window(last)
            .iter()
            .map(|t| t.total.as_secs_f64() * 1e3)
            .collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        p95(&v)
    }

    /// Whole-tick mean allocations and bytes over the last `last` ticks.
    pub fn tick_alloc_mean(&self, last: usize) -> (f64, f64) {
        let w = self.window(last);
        if w.is_empty() {
            return (0.0, 0.0);
        }
        let n = w.len() as f64;
        (
            w.iter().map(|t| t.allocs).sum::<u64>() as f64 / n,
            w.iter().map(|t| t.bytes).sum::<u64>() as f64 / n,
        )
    }
}
