//! The scheduler: owns the two clocks, the system list, the RNG streams,
//! the profiler ring, and the edit-mode snapshot. Drives ticks over a world.
//!
//! "Editing" is the world with the tick stages disabled; "playing" is the
//! world with them enabled. `play` takes a snapshot, `stop` restores it (§2).

use crate::alloc;
use crate::clock::{Clock, ClockKind};
use crate::prof::{
    AllocProf, Capture, CapturedSpan, CapturedTick, Profiler, SystemProf, SystemSample, TickRecord,
};
use crate::rng::{self, Pcg32};
use crate::system::{ClockBinding, Stage, SystemCtx, SystemDesc, SystemFn};
use argus_ecs::{Access, ArgusError, ComponentId, Event, Registry, Result, Snapshot, World};
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::time::Instant;

/// Scene-level scheduler configuration (`scene.toml` clock section, §4/§6).
#[derive(Clone, Debug, PartialEq)]
pub struct SchedulerConfig {
    /// Which clock drives `sim`.
    pub driving: ClockKind,
    /// Continuous clock rate in ticks per second.
    pub hz: f64,
    /// Run seed; every RNG stream derives from it.
    pub seed: u64,
    /// How many recent ticks the profiler keeps.
    pub prof_ticks: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        SchedulerConfig {
            driving: ClockKind::Continuous,
            hz: 60.0,
            seed: 0,
            prof_ticks: 600,
        }
    }
}

impl SchedulerConfig {
    /// Continuous scene at `hz`.
    pub fn continuous(hz: f64) -> Self {
        SchedulerConfig {
            driving: ClockKind::Continuous,
            hz,
            ..Default::default()
        }
    }
    /// Discrete scene (turns).
    pub fn discrete() -> Self {
        SchedulerConfig {
            driving: ClockKind::Discrete,
            ..Default::default()
        }
    }
    /// Set the run seed.
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }
}

struct SystemSlot {
    desc: SystemDesc,
    name: Arc<str>,
    run: SystemFn,
    enabled: bool,
    disabled_reason: Option<String>,
    rng: Option<Pcg32>,
}

/// Everything about the sim that is not in the world: clocks, seed, RNG
/// stream states, play state. Together with an `argus_ecs::Snapshot` this
/// is a full snapshot (§6).
#[derive(Clone, Debug, PartialEq)]
pub struct SimState {
    /// Continuous clock.
    pub continuous: Clock,
    /// Discrete clock.
    pub discrete: Clock,
    /// Run seed.
    pub seed: u64,
    /// RNG state per system that declared a stream, keyed by system name.
    pub rng: Vec<(String, (u64, u64))>,
    /// Play state.
    pub playing: bool,
    /// Pause state.
    pub paused: bool,
}

/// World snapshot plus sim state.
#[derive(Clone, Debug)]
pub struct SimSnapshot {
    /// The world.
    pub world: Snapshot,
    /// The sim.
    pub sim: SimState,
}

/// What one render frame did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameReport {
    /// Sim ticks run on the continuous clock.
    pub sim_ticks: u32,
    /// Whether `present` ran (always, unless there are no present systems).
    pub presented: bool,
}

/// Result of `run_until`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunUntil {
    /// Ticks run.
    pub ticks: u64,
    /// True if the condition was met; false if `max_ticks` was reached first.
    pub hit: bool,
}

/// One system as `script.systems` / `argus docs` report it.
#[derive(Clone, Debug, PartialEq)]
pub struct SystemInfo {
    /// Name.
    pub name: String,
    /// Stage.
    pub stage: Stage,
    /// Ordering constraints.
    pub after: Vec<String>,
    /// Declared reads, by component name.
    pub reads: Vec<String>,
    /// Declared writes, by component name.
    pub writes: Vec<String>,
    /// Clock binding.
    pub clock: ClockBinding,
    /// RNG stream.
    pub rng: Option<String>,
    /// Enabled?
    pub enabled: bool,
    /// Why it is disabled, if it is.
    pub disabled_reason: Option<String>,
    /// Declaration site.
    pub source: Option<String>,
    /// Doc string.
    pub doc: String,
    /// Runs at the end of its stage.
    pub last: bool,
}

/// Runs systems over a world on two clocks.
pub struct Scheduler {
    config: SchedulerConfig,
    continuous: Clock,
    discrete: Clock,
    systems: Vec<SystemSlot>,
    /// Run order per stage, as indices into `systems`. Rebuilt lazily.
    order: [Vec<usize>; 5],
    order_dirty: bool,
    playing: bool,
    paused: bool,
    edit_snapshot: Option<SimSnapshot>,
    prof: Profiler,
    capture: Option<Capture>,
    observers: Vec<Observer>,
}

/// Something that reads the world at the end of every tick and cannot
/// change it: the daemon's history for `obs.trace`. Called with the frame
/// the tick ran as, the frame its events carry.
pub type Observer = Box<dyn FnMut(&World, u64)>;

/// What one tick collects while it runs.
struct TickSamples {
    timings: Vec<SystemSample>,
    spans: Vec<CapturedSpan>,
}

impl Scheduler {
    /// New scheduler with no systems.
    pub fn new(config: SchedulerConfig) -> Scheduler {
        let prof = Profiler::new(config.prof_ticks);
        Scheduler {
            continuous: Clock::continuous(config.hz),
            discrete: Clock::discrete(),
            config,
            systems: Vec::new(),
            order: Default::default(),
            order_dirty: false,
            playing: false,
            paused: false,
            edit_snapshot: None,
            prof,
            capture: None,
            observers: Vec::new(),
        }
    }

    /// Read the world at the end of every tick, after the hierarchy stage
    /// and before the frame counter moves on. Observers are not systems:
    /// they declare nothing, change nothing, and are not listed; their time
    /// is sampled as `obs.observe`.
    pub fn observe(&mut self, f: Observer) {
        self.observers.push(f);
    }

    /// Remove every observer.
    pub fn clear_observers(&mut self) {
        self.observers.clear();
    }

    /// The configuration.
    pub fn config(&self) -> &SchedulerConfig {
        &self.config
    }

    // ----- systems ----------------------------------------------------------

    /// Register a system. Names are unique; `present` systems may not
    /// declare writes; ordering is validated on the next tick.
    pub fn add_system(&mut self, desc: SystemDesc, run: SystemFn) -> Result<()> {
        if self.systems.iter().any(|s| s.desc.name == desc.name) {
            return Err(ArgusError::new(
                crate::codes::DUPLICATE_SYSTEM,
                format!("a system named {:?} is already registered", desc.name),
            )
            .hint("System names are unique; the file path is the conventional name.")
            .at(desc.source.clone()));
        }
        if desc.stage == Stage::Present && !desc.access.writes.is_empty() {
            return Err(ArgusError::new(
                crate::codes::STAGE_READ_ONLY,
                format!(
                    "system {} is in the present stage but declares writes",
                    desc.name
                ),
            )
            .hint("present is read-only; move gameplay mutation to sim or late.")
            .at(desc.source.clone()));
        }
        let rng = desc.rng.as_ref().map(|s| rng::stream(self.config.seed, s));
        self.systems.push(SystemSlot {
            name: Arc::from(desc.name.as_str()),
            desc,
            run,
            enabled: true,
            disabled_reason: None,
            rng,
        });
        self.order_dirty = true;
        Ok(())
    }

    fn slot_index(&self, name: &str) -> Result<usize> {
        self.systems
            .iter()
            .position(|s| s.desc.name == name)
            .ok_or_else(|| {
                ArgusError::new(
                    crate::codes::UNKNOWN_SYSTEM,
                    format!("no system named {name:?}"),
                )
                .hint("script.systems lists every registered system.")
            })
    }

    /// Remove a system.
    pub fn remove_system(&mut self, name: &str) -> Result<()> {
        let i = self.slot_index(name)?;
        self.systems.remove(i);
        self.order_dirty = true;
        Ok(())
    }

    /// Replace a system's declaration and body in place, keeping its slot so
    /// registration-order tie-breaks do not shift. Re-enables it and keeps
    /// its RNG stream state when the stream name is unchanged (§7 hot reload:
    /// "continues from the current frame"). Registers it if absent.
    pub fn replace_system(&mut self, desc: SystemDesc, run: SystemFn) -> Result<()> {
        let Some(i) = self.systems.iter().position(|s| s.desc.name == desc.name) else {
            return self.add_system(desc, run);
        };
        if desc.stage == Stage::Present && !desc.access.writes.is_empty() {
            return Err(ArgusError::new(
                crate::codes::STAGE_READ_ONLY,
                format!(
                    "system {} is in the present stage but declares writes",
                    desc.name
                ),
            )
            .hint("present is read-only; move gameplay mutation to sim or late.")
            .at(desc.source.clone()));
        }
        let slot = &mut self.systems[i];
        let rng = match (&slot.desc.rng, &desc.rng) {
            (Some(old), Some(new)) if old == new => slot.rng.take(),
            (_, Some(new)) => Some(rng::stream(self.config.seed, new)),
            (_, None) => None,
        };
        slot.desc = desc;
        slot.run = run;
        slot.rng = rng;
        slot.enabled = true;
        slot.disabled_reason = None;
        self.order_dirty = true;
        Ok(())
    }

    /// Re-enable a system (after a fault, or a `disable`).
    pub fn enable(&mut self, name: &str) -> Result<()> {
        let i = self.slot_index(name)?;
        self.systems[i].enabled = true;
        self.systems[i].disabled_reason = None;
        Ok(())
    }

    /// Disable a system.
    pub fn disable(&mut self, name: &str, reason: &str) -> Result<()> {
        let i = self.slot_index(name)?;
        self.systems[i].enabled = false;
        self.systems[i].disabled_reason = Some(reason.to_string());
        Ok(())
    }

    /// Enabled?
    pub fn is_enabled(&self, name: &str) -> Result<bool> {
        Ok(self.systems[self.slot_index(name)?].enabled)
    }

    /// Every system in stage and run order.
    pub fn systems(&mut self, registry: &Registry) -> Result<Vec<SystemInfo>> {
        self.ensure_order()?;
        let names = |ids: &[ComponentId]| {
            ids.iter()
                .map(|&c| registry.schema(c).name.clone())
                .collect()
        };
        let mut out = Vec::new();
        for stage in &self.order {
            for &i in stage {
                let s = &self.systems[i];
                out.push(SystemInfo {
                    name: s.desc.name.clone(),
                    stage: s.desc.stage,
                    after: s.desc.after.clone(),
                    reads: names(&s.desc.access.reads),
                    writes: names(&s.desc.access.writes),
                    clock: s.desc.clock,
                    rng: s.desc.rng.clone(),
                    enabled: s.enabled,
                    disabled_reason: s.disabled_reason.clone(),
                    source: s.desc.source.clone(),
                    doc: s.desc.doc.clone(),
                    last: s.desc.last,
                });
            }
        }
        Ok(out)
    }

    /// Systems that declare a write to the component, in stage and run
    /// order: the answer to "what could have changed Health?" (§3, `world.who_writes`).
    pub fn who_writes(&mut self, c: ComponentId) -> Result<Vec<String>> {
        self.ensure_order()?;
        Ok(self
            .order
            .iter()
            .flatten()
            .filter(|&&i| self.systems[i].desc.access.allows_write(c))
            .map(|&i| self.systems[i].desc.name.clone())
            .collect())
    }

    /// Systems that declare a read of the component, in stage and run order.
    pub fn who_reads(&mut self, c: ComponentId) -> Result<Vec<String>> {
        self.ensure_order()?;
        Ok(self
            .order
            .iter()
            .flatten()
            .filter(|&&i| self.systems[i].desc.access.allows_read(c))
            .map(|&i| self.systems[i].desc.name.clone())
            .collect())
    }

    /// Validate ordering now instead of on the first tick.
    pub fn validate(&mut self) -> Result<()> {
        self.ensure_order()
    }

    fn ensure_order(&mut self) -> Result<()> {
        if !self.order_dirty {
            return Ok(());
        }
        let by_name: HashMap<&str, usize> = self
            .systems
            .iter()
            .enumerate()
            .map(|(i, s)| (s.desc.name.as_str(), i))
            .collect();
        let mut order: [Vec<usize>; 5] = Default::default();
        for stage in Stage::ALL {
            let members: Vec<usize> = (0..self.systems.len())
                .filter(|&i| self.systems[i].desc.stage == stage)
                .collect();
            // Kahn's algorithm, always taking the lowest registration index
            // among ready systems so ties are deterministic; a `last` system
            // waits while anything else is ready.
            let mut indeg: HashMap<usize, usize> = members.iter().map(|&i| (i, 0)).collect();
            let mut edges: HashMap<usize, Vec<usize>> = HashMap::new();
            for &i in &members {
                for dep in &self.systems[i].desc.after {
                    let &j = by_name.get(dep.as_str()).ok_or_else(|| {
                        ArgusError::new(
                            crate::codes::UNKNOWN_SYSTEM,
                            format!(
                                "system {} runs after {dep:?}, which is not registered",
                                self.systems[i].desc.name
                            ),
                        )
                        .hint("Check the name, or register the other system first.")
                        .at(self.systems[i].desc.source.clone())
                    })?;
                    if self.systems[j].desc.stage != stage {
                        return Err(ArgusError::new(
                            crate::codes::ORDER_CYCLE,
                            format!(
                                "system {} runs after {dep:?}, which is in stage {} not {}",
                                self.systems[i].desc.name,
                                self.systems[j].desc.stage,
                                stage
                            ),
                        )
                        .hint("after only orders systems within one stage; stages already order each other.")
                        .at(self.systems[i].desc.source.clone()));
                    }
                    edges.entry(j).or_default().push(i);
                    *indeg.get_mut(&i).unwrap() += 1;
                }
            }
            let mut ready: Vec<usize> = members.iter().copied().filter(|i| indeg[i] == 0).collect();
            let mut out = Vec::with_capacity(members.len());
            let systems = &self.systems;
            let key = |i: &usize| (systems[*i].desc.last, *i);
            while !ready.is_empty() {
                ready.sort_unstable_by_key(|i| std::cmp::Reverse(key(i)));
                let i = ready.pop().unwrap();
                out.push(i);
                if let Some(next) = edges.get(&i) {
                    for &k in next {
                        let d = indeg.get_mut(&k).unwrap();
                        *d -= 1;
                        if *d == 0 {
                            ready.push(k);
                        }
                    }
                }
            }
            if out.len() != members.len() {
                let stuck: Vec<&str> = members
                    .iter()
                    .filter(|i| !out.contains(i))
                    .map(|&i| self.systems[i].desc.name.as_str())
                    .collect();
                return Err(ArgusError::new(
                    crate::codes::ORDER_CYCLE,
                    format!("after constraints form a cycle among: {}", stuck.join(", ")),
                )
                .hint("Remove one after entry so the stage has a linear order."));
            }
            order[stage.index()] = out;
        }
        self.order = order;
        self.order_dirty = false;
        Ok(())
    }

    // ----- clocks and play state --------------------------------------------

    /// Interpolation fraction for the renderer: how far into the next
    /// tick real time is, 0 to 1 (section 6). 1 on the discrete clock.
    pub fn alpha(&self) -> f32 {
        self.driving().alpha()
    }

    /// The clock that drives `sim`.
    pub fn driving(&self) -> &Clock {
        self.clock(self.config.driving)
    }

    /// A clock by kind.
    pub fn clock(&self, kind: ClockKind) -> &Clock {
        match kind {
            ClockKind::Continuous => &self.continuous,
            ClockKind::Discrete => &self.discrete,
        }
    }

    fn clock_mut(&mut self, kind: ClockKind) -> &mut Clock {
        match kind {
            ClockKind::Continuous => &mut self.continuous,
            ClockKind::Discrete => &mut self.discrete,
        }
    }

    /// Driving-clock tick count.
    pub fn tick(&self) -> u64 {
        self.driving().tick
    }

    /// Driving-clock sim time (`sim.time`).
    pub fn time(&self) -> f64 {
        self.driving().time()
    }

    /// Run seed.
    pub fn seed(&self) -> u64 {
        self.config.seed
    }

    /// Change the run seed and reseed every stream from it.
    pub fn set_seed(&mut self, seed: u64) {
        self.config.seed = seed;
        for s in &mut self.systems {
            s.rng = s.desc.rng.as_ref().map(|n| rng::stream(seed, n));
        }
    }

    /// Enter play (or resume from pause). The first `play` from edit mode
    /// snapshots the world so `stop` can return to it.
    pub fn play(&mut self, world: &World) {
        if !self.playing {
            self.edit_snapshot = Some(SimSnapshot {
                world: world.snapshot(),
                sim: self.state(),
            });
            self.playing = true;
        }
        self.paused = false;
    }

    /// Pause: frames stop ticking; `step` still works.
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// Leave play: restore the world and sim state from when `play` was called.
    pub fn stop(&mut self, world: &mut World) {
        if let Some(snap) = self.edit_snapshot.take() {
            world.restore(&snap.world);
            self.restore_state(&snap.sim);
        }
        self.playing = false;
        self.paused = false;
    }

    /// In play mode?
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Paused?
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// `sim.speed {x}`: ticks per render frame on the continuous clock.
    pub fn set_speed(&mut self, x: f64) -> Result<()> {
        if self.config.driving != ClockKind::Continuous {
            return Err(ArgusError::new(
                crate::codes::WRONG_CLOCK,
                "speed applies to the continuous clock; this scene's sim is discrete",
            )
            .hint("Use sim.advance to move a discrete clock."));
        }
        self.continuous.set_speed(x)
    }

    /// Current speed.
    pub fn speed(&self) -> f64 {
        self.continuous.speed
    }

    // ----- running ----------------------------------------------------------

    /// One render frame. A continuous scene in play runs `speed` ticks
    /// (fractional speeds accumulate); a discrete scene never ticks on a
    /// frame. `present` runs regardless of play state.
    pub fn frame(&mut self, world: &mut World) -> Result<FrameReport> {
        let mut report = FrameReport::default();
        if self.playing && !self.paused && self.config.driving == ClockKind::Continuous {
            let n = self.continuous.ticks_for_frame();
            for _ in 0..n {
                self.run_tick(world, ClockKind::Continuous)?;
            }
            report.sim_ticks = n;
        }
        report.presented = self.present(world)?;
        Ok(report)
    }

    /// `sim.step {n}`: run `n` ticks of the driving clock, ignoring pause.
    pub fn step(&mut self, world: &mut World, n: u64) -> Result<()> {
        for _ in 0..n {
            self.run_tick(world, self.config.driving)?;
        }
        Ok(())
    }

    /// `sim.advance {by: "turn", n}`: run `n` discrete ticks. Works in any
    /// scene, so a continuous game can host a discrete subsystem.
    pub fn advance(&mut self, world: &mut World, n: u64) -> Result<()> {
        for _ in 0..n {
            self.run_tick(world, ClockKind::Discrete)?;
        }
        Ok(())
    }

    /// Run driving-clock ticks until `done(world)` is true or `max_ticks`
    /// pass. This is the headless fast-forward: as fast as the CPU allows.
    pub fn run_until(
        &mut self,
        world: &mut World,
        max_ticks: u64,
        mut done: impl FnMut(&World) -> bool,
    ) -> Result<RunUntil> {
        let mut ticks = 0;
        while ticks < max_ticks {
            self.run_tick(world, self.config.driving)?;
            ticks += 1;
            if done(world) {
                return Ok(RunUntil { ticks, hit: true });
            }
        }
        Ok(RunUntil { ticks, hit: false })
    }

    /// Run until a gameplay event with this name is emitted, or `max_ticks`.
    pub fn run_until_event(
        &mut self,
        world: &mut World,
        name: &str,
        max_ticks: u64,
    ) -> Result<RunUntil> {
        let start = world.events().len();
        let mut seen = start;
        self.run_until(world, max_ticks, |w| {
            let evs = w.events();
            let hit = evs[seen.min(evs.len())..]
                .iter()
                .any(|r| matches!(&r.event, Event::Game { name: n, .. } if n == name));
            seen = evs.len();
            hit
        })
    }

    /// Run driving-clock ticks until sim time reaches `seconds`.
    pub fn run_until_time(
        &mut self,
        world: &mut World,
        seconds: f64,
        max_ticks: u64,
    ) -> Result<RunUntil> {
        let target_tick = (seconds / self.driving().timestep).ceil() as u64;
        let mut ticks = 0;
        while self.driving().tick < target_tick && ticks < max_ticks {
            self.run_tick(world, self.config.driving)?;
            ticks += 1;
        }
        Ok(RunUntil {
            ticks,
            hit: self.driving().tick >= target_tick,
        })
    }

    fn binds(&self, i: usize, kind: ClockKind) -> bool {
        self.systems[i].desc.clock.resolve(self.config.driving) == kind
    }

    fn run_tick(&mut self, world: &mut World, kind: ClockKind) -> Result<()> {
        self.ensure_order()?;
        let started = Instant::now();
        let (a0, b0) = alloc::counters();
        let tick = self.clock(kind).tick;
        let dt = self.clock(kind).timestep;
        let time = self.clock(kind).time();
        let mut samples = TickSamples {
            timings: Vec::new(),
            spans: Vec::new(),
        };
        for stage in [Stage::Input, Stage::Sim, Stage::Late] {
            self.run_stage(world, stage, kind, tick, time, dt, &mut samples);
        }
        {
            let t = Instant::now();
            let (pa, pb) = alloc::counters();
            argus_ecs::propagate(world)?;
            let (qa, qb) = alloc::counters();
            self.sample(
                &mut samples,
                Arc::from("hierarchy.propagate"),
                t,
                qa - pa,
                qb - pb,
            );
        }
        self.run_stage(world, Stage::Hierarchy, kind, tick, time, dt, &mut samples);
        if !self.observers.is_empty() {
            let t = Instant::now();
            let (pa, pb) = alloc::counters();
            let frame = world.frame();
            for o in &mut self.observers {
                o(world, frame);
            }
            let (qa, qb) = alloc::counters();
            self.sample(&mut samples, Arc::from("obs.observe"), t, qa - pa, qb - pb);
        }
        world.end_frame();
        {
            // The shared broadphase (§12), rebuilt once per tick from the
            // tick's final positions: what the next tick's systems and any
            // query between ticks see.
            let t = Instant::now();
            let (pa, pb) = alloc::counters();
            world.rebuild_broadphase();
            let (qa, qb) = alloc::counters();
            self.sample(
                &mut samples,
                Arc::from("broadphase.rebuild"),
                t,
                qa - pa,
                qb - pb,
            );
        }

        let archetypes: Vec<(u32, usize)> = world
            .archetypes()
            .iter()
            .filter(|a| !a.is_empty())
            .map(|a| (a.id.0, a.len()))
            .collect();
        let (a1, b1) = alloc::counters();
        let total = started.elapsed();
        if let Some(c) = &mut self.capture {
            if !c.complete() {
                c.ticks.push(CapturedTick {
                    tick,
                    clock: kind,
                    start_us: c.us(started),
                    dur_us: total.as_secs_f64() * 1e6,
                    entities: archetypes.iter().map(|a| a.1).sum(),
                    allocs: a1 - a0,
                    bytes: b1 - b0,
                    systems: std::mem::take(&mut samples.spans),
                });
            }
        }
        self.prof.record(TickRecord {
            tick,
            clock: kind,
            total,
            systems: samples.timings,
            allocs: a1 - a0,
            bytes: b1 - b0,
            archetypes,
        });
        self.clock_mut(kind).tick += 1;
        Ok(())
    }

    /// Record one system's sample for the profiler ring and, while a
    /// capture is collecting, its span on the capture's time base.
    fn sample(&self, s: &mut TickSamples, name: Arc<str>, t: Instant, allocs: u64, bytes: u64) {
        let elapsed = t.elapsed();
        if let Some(c) = &self.capture {
            if !c.complete() {
                s.spans.push(CapturedSpan {
                    name: name.clone(),
                    start_us: c.us(t),
                    dur_us: elapsed.as_secs_f64() * 1e6,
                    allocs,
                    bytes,
                });
            }
        }
        s.timings.push(SystemSample {
            name,
            elapsed,
            allocs,
            bytes,
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn run_stage(
        &mut self,
        world: &mut World,
        stage: Stage,
        kind: ClockKind,
        tick: u64,
        time: f64,
        dt: f64,
        samples: &mut TickSamples,
    ) {
        let order = self.order[stage.index()].clone();
        for i in order {
            if !self.systems[i].enabled || !self.binds(i, kind) {
                continue;
            }
            let t = Instant::now();
            let (a0, b0) = alloc::counters();
            self.run_system(i, world, stage, kind, tick, time, dt);
            let (a1, b1) = alloc::counters();
            let name = self.systems[i].name.clone();
            self.sample(samples, name, t, a1 - a0, b1 - b0);
        }
    }

    /// Run the `present` stage once. Returns true if any system ran.
    pub fn present(&mut self, world: &mut World) -> Result<bool> {
        self.ensure_order()?;
        let kind = self.config.driving;
        let (tick, time, dt) = (
            self.driving().tick,
            self.driving().time(),
            self.driving().timestep,
        );
        let order = self.order[Stage::Present.index()].clone();
        let mut ran = false;
        for i in order {
            if !self.systems[i].enabled {
                continue;
            }
            self.run_system(i, world, Stage::Present, kind, tick, time, dt);
            ran = true;
        }
        Ok(ran)
    }

    #[allow(clippy::too_many_arguments)]
    fn run_system(
        &mut self,
        i: usize,
        world: &mut World,
        stage: Stage,
        kind: ClockKind,
        tick: u64,
        time: f64,
        dt: f64,
    ) {
        let slot = &mut self.systems[i];
        let outcome = {
            let mut ctx = SystemCtx {
                world,
                access: &slot.desc.access,
                rng: slot.rng.as_mut(),
                name: &slot.desc.name,
                stage,
                clock: kind,
                tick,
                time,
                dt,
                learned: Vec::new(),
            };
            // What the system emits says it did (obs.why).
            ctx.world.set_emitter(Some(slot.name.clone()));
            let r = catch_unwind(AssertUnwindSafe(|| (slot.run)(&mut ctx)));
            ctx.world.set_emitter(None);
            let learned = std::mem::take(&mut ctx.learned);
            drop(ctx);
            // What the system's data named joins its declared access.
            for (c, write) in learned {
                let a = &mut slot.desc.access;
                if write {
                    if !a.writes.contains(&c) {
                        a.writes.push(c);
                    }
                } else if !a.allows_read(c) {
                    a.reads.push(c);
                }
            }
            r
        };
        let (message, r#ref) = match outcome {
            Ok(Ok(())) => return,
            Ok(Err(e)) => (
                e.to_string(),
                e.r#ref.clone().or_else(|| slot.desc.source.clone()),
            ),
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "panic with non-string payload".to_string());
                (format!("panic: {msg}"), slot.desc.source.clone())
            }
        };
        slot.enabled = false;
        slot.disabled_reason = Some(message.clone());
        let name = slot.desc.name.clone();
        world.emit(Event::SystemFault {
            system: name.clone(),
            message: message.clone(),
            r#ref,
        });
        world.emit(Event::SystemDisabled {
            system: name,
            reason: message,
        });
    }

    // ----- snapshots --------------------------------------------------------

    /// Sim state for a snapshot.
    pub fn state(&self) -> SimState {
        SimState {
            continuous: self.continuous.clone(),
            discrete: self.discrete.clone(),
            seed: self.config.seed,
            rng: self
                .systems
                .iter()
                .filter_map(|s| s.rng.as_ref().map(|r| (s.desc.name.clone(), r.state())))
                .collect(),
            playing: self.playing,
            paused: self.paused,
        }
    }

    /// Restore sim state. Systems present now but absent from the state
    /// keep their current RNG; the reverse is ignored.
    pub fn restore_state(&mut self, state: &SimState) {
        self.continuous = state.continuous.clone();
        self.discrete = state.discrete.clone();
        self.config.seed = state.seed;
        for s in &mut self.systems {
            if let Some((_, (st, inc))) = state.rng.iter().find(|(n, _)| *n == s.desc.name) {
                s.rng = Some(Pcg32::from_state(*st, *inc));
            }
        }
        self.playing = state.playing;
        self.paused = state.paused;
    }

    /// Full snapshot: world plus sim state.
    pub fn snapshot(&self, world: &World) -> SimSnapshot {
        SimSnapshot {
            world: world.snapshot(),
            sim: self.state(),
        }
    }

    /// Restore a full snapshot.
    pub fn restore(&mut self, world: &mut World, snap: &SimSnapshot) {
        world.restore(&snap.world);
        self.restore_state(&snap.sim);
    }

    // ----- profiling --------------------------------------------------------

    /// Row order for `prof.systems` and `prof.alloc`: stage order, with
    /// `hierarchy.propagate` at the head of its stage.
    fn prof_order(&mut self) -> Result<Vec<(Arc<str>, Stage)>> {
        self.ensure_order()?;
        let mut order: Vec<(Arc<str>, Stage)> = Vec::new();
        for stage in Stage::ALL {
            if stage == Stage::Hierarchy {
                order.push((Arc::from("hierarchy.propagate"), Stage::Hierarchy));
            }
            for &i in &self.order[stage.index()] {
                order.push((self.systems[i].name.clone(), stage));
            }
            if stage == Stage::Hierarchy {
                order.push((Arc::from("broadphase.rebuild"), Stage::Hierarchy));
            }
        }
        Ok(order)
    }

    /// `prof.systems {last}`: mean and p95 per system in stage order.
    pub fn prof_systems(&mut self, last: usize) -> Result<Vec<SystemProf>> {
        let order = self.prof_order()?;
        Ok(self.prof.systems(last, &order))
    }

    /// `prof.alloc {last}`: allocations and bytes per system per tick in
    /// stage order. Real numbers only when `alloc::installed()`; see
    /// `crate::alloc`.
    pub fn prof_alloc(&mut self, last: usize) -> Result<Vec<AllocProf>> {
        let order = self.prof_order()?;
        Ok(self.prof.alloc(last, &order))
    }

    /// Mean whole-tick `(allocations, bytes)` over the last `last` ticks.
    pub fn prof_tick_alloc_mean(&self, last: usize) -> (f64, f64) {
        self.prof.tick_alloc_mean(last)
    }

    /// `prof.capture {ticks}`: collect the next `ticks` ticks with every
    /// system span on one time base. Replaces a capture in progress.
    pub fn capture_start(&mut self, ticks: usize) {
        self.capture = Some(Capture::new(ticks));
    }

    /// Progress of the capture in progress: `(collected, wanted)`.
    pub fn capture_status(&self) -> Option<(usize, usize)> {
        self.capture.as_ref().map(|c| (c.ticks.len(), c.wanted))
    }

    /// The capture, once every wanted tick is in; `None` while collecting
    /// or when none was started.
    pub fn capture_take(&mut self) -> Option<Capture> {
        if self.capture.as_ref().is_some_and(|c| c.complete()) {
            self.capture.take()
        } else {
            None
        }
    }

    /// Abandon the capture in progress, returning what was collected.
    pub fn capture_cancel(&mut self) -> Option<Capture> {
        self.capture.take()
    }

    /// `prof.worst {n}`: the slowest recent ticks with per-system breakdown
    /// and entity counts by archetype.
    pub fn prof_worst(&self, n: usize) -> Vec<TickRecord> {
        self.prof.worst(n)
    }

    /// `prof.frame`: the most recent tick.
    pub fn prof_last(&self) -> Option<&TickRecord> {
        self.prof.last()
    }

    /// Whole-tick p95 over the last `last` ticks, ms. What a `perf("sim")`
    /// assertion in the test harness reads.
    pub fn prof_tick_p95_ms(&self, last: usize) -> f64 {
        self.prof.tick_p95_ms(last)
    }

    /// Ticks currently held by the profiler.
    pub fn prof_len(&self) -> usize {
        self.prof.len()
    }

    /// A read-only access set (used for `present` contexts and tooling).
    pub fn read_only(access: &Access) -> Access {
        Access {
            reads: access
                .reads
                .iter()
                .chain(access.writes.iter())
                .copied()
                .collect(),
            writes: Vec::new(),
        }
    }
}
