//! The scheduler: owns the two clocks, the system list, the RNG streams,
//! the profiler ring, and the edit-mode snapshot. Drives ticks over a world.
//!
//! "Editing" is the world with the tick stages disabled; "playing" is the
//! world with them enabled. `play` takes a snapshot, `stop` restores it (§2).

use crate::alloc;
use crate::clock::{Clock, ClockKind};
use crate::par::{Deferred, ParCtx, ParSystemFn};
use crate::prof::{
    AllocProf, Capture, CapturedSpan, CapturedTick, Profiler, SystemProf, SystemSample, TickRecord,
};
use crate::rng::{self, Pcg32};
use crate::system::{ClockBinding, Stage, SystemCtx, SystemDesc, SystemFn};
use argus_ecs::codec::{Reader, Writer};
use argus_ecs::{Access, ArgusError, ComponentId, Event, Registry, Result, Snapshot, World};
use argus_os::time::Instant;
use std::collections::{BTreeSet, HashMap};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::time::Duration;

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
    /// Threads a batch of parallel systems may use: 1 runs every batch on
    /// the sim thread, 0 (the default) one per core. The world after a
    /// tick is the same for any number (§6).
    pub threads: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        SchedulerConfig {
            driving: ClockKind::Continuous,
            hz: 60.0,
            seed: 0,
            prof_ticks: 600,
            threads: 0,
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
    run: Body,
    enabled: bool,
    disabled_reason: Option<String>,
    /// Switched off by a fault, not by a scene's settings: what a reload
    /// switches on again ([`Scheduler::enable_faulted`]).
    faulted: bool,
    rng: Option<Pcg32>,
    /// The batch it runs in ([`NO_BATCH`] for a serial system).
    batch: u32,
    /// What its last run took: whether a batch is worth threads.
    cost: Duration,
}

/// A system's body: run with the whole world on the sim thread, or in a
/// batch through a [`ParCtx`].
enum Body {
    Serial(SystemFn),
    Parallel(ParSystemFn),
}

const NO_BATCH: u32 = u32::MAX;

/// A batch runs on threads only when its systems' last runs promise to save
/// at least this much (the time beyond its longest system), since starting
/// threads costs tens of microseconds. Either way the batch is the same.
const PARALLEL_FLOOR: Duration = Duration::from_micros(200);

/// One parallel system's run in a batch, with what it needs and what it
/// leaves: each is moved to the thread that runs it.
struct Job<'s> {
    slot: usize,
    run: &'s mut ParSystemFn,
    rng: Option<&'s mut Pcg32>,
    access: &'s Access,
    spawns: &'s [ComponentId],
    name: &'s Arc<str>,
    /// What is held over for it from frozen ticks.
    held: Vec<argus_ecs::EventRecord>,
    out: Deferred,
    outcome: Option<std::thread::Result<Result<()>>>,
    start: Option<Instant>,
    elapsed: Duration,
    allocs: (u64, u64),
    lane: u32,
}

/// What every job of a batch shares.
#[derive(Clone, Copy)]
struct Tick {
    stage: Stage,
    kind: ClockKind,
    tick: u64,
    time: f64,
    dt: f64,
    speed: f64,
    frozen: bool,
}

impl Job<'_> {
    fn run(&mut self, world: &World, t: Tick, lane: u32) {
        let start = Instant::now();
        let (a0, b0) = alloc::counters();
        let mut ctx = ParCtx {
            world,
            access: self.access,
            spawns: self.spawns,
            rng: self.rng.as_deref_mut(),
            out: &mut self.out,
            name: self.name,
            stage: t.stage,
            clock: t.kind,
            tick: t.tick,
            time: t.time,
            dt: t.dt,
            speed: t.speed,
            frozen: t.frozen,
            held: &self.held,
        };
        let run = &mut *self.run;
        self.outcome = Some(catch_unwind(AssertUnwindSafe(|| run(&mut ctx))));
        let (a1, b1) = alloc::counters();
        self.start = Some(start);
        self.elapsed = start.elapsed();
        self.allocs = (a1 - a0, b1 - b0);
        self.lane = lane;
    }
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
    /// Systems switched off (by a fault or by request), with the reason.
    pub disabled: Vec<(String, String)>,
    /// Events held over a frozen tick for the systems it kept from running
    /// ([`Scheduler::hold_when_frozen`]): input the host will not send
    /// again, so a replay from a keyframe must still have it.
    pub held: Vec<HeldEvent>,
}

/// An event a freeze kept some systems from seeing, held for them until
/// each next runs (issue 5: a model's answer arrived while a pause froze
/// the tick, and the game waited for it for ever).
#[derive(Clone, Debug, PartialEq)]
pub struct HeldEvent {
    /// The event as it was emitted, its frame the tick it arrived on.
    pub record: argus_ecs::EventRecord,
    /// The systems that have not seen it yet, by name.
    pub owed: Vec<String>,
}

fn encode_value(w: &mut Writer, v: &argus_ecs::Value) {
    use argus_ecs::Value;
    match v {
        Value::Bool(b) => {
            w.u8(0);
            w.bool(*b);
        }
        Value::Int(n) => {
            w.u8(1);
            w.i64(*n);
        }
        Value::UInt(n) => {
            w.u8(2);
            w.u64(*n);
        }
        Value::Float(f) => {
            w.u8(3);
            w.f64(*f);
        }
        Value::Vec2(v) => {
            w.u8(4);
            w.f32(v.x);
            w.f32(v.y);
        }
        Value::Str(s) => {
            w.u8(5);
            w.str(s);
        }
        Value::Entity(e) => {
            w.u8(6);
            w.u64(e.to_bits());
        }
        Value::Ref(s) => {
            w.u8(7);
            w.str(s);
        }
        Value::List(items) => {
            w.u8(8);
            w.count(items.len());
            for x in items {
                encode_value(w, x);
            }
        }
    }
}

fn decode_value(r: &mut Reader) -> Result<argus_ecs::Value> {
    use argus_ecs::Value;
    Ok(match r.u8()? {
        0 => Value::Bool(r.bool()?),
        1 => Value::Int(r.i64()?),
        2 => Value::UInt(r.u64()?),
        3 => Value::Float(r.f64()?),
        4 => {
            let x = r.f32()?;
            Value::Vec2(argus_ecs::Vec2::new(x, r.f32()?))
        }
        5 => Value::Str(r.str()?),
        6 => Value::Entity(argus_ecs::Entity::from_bits(r.u64()?)),
        7 => Value::Ref(r.str()?),
        8 => {
            let n = r.count()?;
            let mut items = Vec::with_capacity(n.min(argus_ecs::value::MAX_LIST));
            for _ in 0..n {
                items.push(decode_value(r)?);
            }
            Value::List(items)
        }
        t => return Err(r.bad(format!("a held event's field of kind {t}"))),
    })
}

impl SimState {
    /// Write the sim state for a keyframe on disk (phase 2 item 2).
    pub fn encode(&self, w: &mut Writer) {
        self.continuous.encode(w);
        self.discrete.encode(w);
        w.u64(self.seed);
        w.count(self.rng.len());
        for (name, (state, inc)) in &self.rng {
            w.str(name);
            w.u64(*state);
            w.u64(*inc);
        }
        w.bool(self.playing);
        w.bool(self.paused);
        w.count(self.disabled.len());
        for (name, why) in &self.disabled {
            w.str(name);
            w.str(why);
        }
        w.count(self.held.len());
        for h in &self.held {
            let Event::Game { name, fields } = &h.record.event else {
                unreachable!("only game events are held");
            };
            w.u64(h.record.frame);
            w.str(name);
            w.count(fields.len());
            for (k, v) in fields {
                w.str(k);
                encode_value(w, v);
            }
            w.opt(h.record.by.as_deref(), |w, by| w.str(by));
            w.count(h.owed.len());
            for o in &h.owed {
                w.str(o);
            }
        }
    }

    /// Read what [`SimState::encode`] wrote.
    pub fn decode(r: &mut Reader) -> Result<SimState> {
        SimState::decode_as(r, true)
    }

    /// Read a sim state from a file that may be older: `held`, whether it
    /// has the events held over a freeze (replay files from version 8,
    /// saves from version 4).
    pub fn decode_as(r: &mut Reader, held: bool) -> Result<SimState> {
        let continuous = Clock::decode(r)?;
        let discrete = Clock::decode(r)?;
        let seed = r.u64()?;
        let mut rng = Vec::new();
        for _ in 0..r.count()? {
            let name = r.str()?;
            rng.push((name, (r.u64()?, r.u64()?)));
        }
        let playing = r.bool()?;
        let paused = r.bool()?;
        let mut disabled = Vec::new();
        for _ in 0..r.count()? {
            let name = r.str()?;
            disabled.push((name, r.str()?));
        }
        let mut kept = Vec::new();
        if held {
            for _ in 0..r.count()? {
                let frame = r.u64()?;
                let name = r.str()?;
                let mut fields = Vec::new();
                for _ in 0..r.count()? {
                    let k = r.str()?;
                    fields.push((k, decode_value(r)?));
                }
                let by = r.opt(|r| r.str())?.map(Arc::from);
                let mut owed = Vec::new();
                for _ in 0..r.count()? {
                    owed.push(r.str()?);
                }
                kept.push(HeldEvent {
                    record: argus_ecs::EventRecord {
                        frame,
                        event: Event::Game { name, fields },
                        by,
                    },
                    owed,
                });
            }
        }
        Ok(SimState {
            continuous,
            discrete,
            seed,
            rng,
            playing,
            paused,
            disabled,
            held: kept,
        })
    }
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
    /// Systems it runs after.
    pub after: Vec<String>,
    /// Systems it runs before.
    pub before: Vec<String>,
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
    /// A parallel system ([`Scheduler::add_parallel_system`]).
    pub parallel: bool,
    /// The batch it runs in, numbered in run order across stages; the
    /// systems of one batch may run at once.
    pub batch: Option<u32>,
    /// Runs at the end of its stage.
    pub last: bool,
    /// Also runs once when a scene loads (`run_load`).
    pub on_load: bool,
    /// Runs first in its stage.
    pub first: bool,
    /// Runs while the sim is frozen.
    pub unfrozen: bool,
    /// Moves between turns (2D staples item 5).
    pub between_turns: bool,
}

/// How many turns a scheduler remembers ([`Scheduler::turns_back`]): far
/// more than a recorder's window of thirty or a mark's ten, so a seek back
/// still finds the turns before where it lands.
pub const RECENT_TURNS: usize = 1024;

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
    tick_end: Option<TickEnd>,
    quiet: bool,
    /// A system froze the rest of the tick being run (`SystemCtx::freeze`).
    frozen: bool,
    /// The ticks that were turns on a driving discrete clock, the latest
    /// [`RECENT_TURNS`] ([`Scheduler::turns_back`]).
    recent_turns: BTreeSet<u64>,
    /// An idle tick is running: only the systems that move between turns.
    idling: bool,
    /// How many recorded idle ticks are owed before a turn, and not yet run
    /// (a seek's or a replay's), from the host's journal.
    idle_due: Option<IdleDue>,
    /// Where idle ticks run live are recorded: the turn they come before,
    /// and how many.
    idle_record: Option<IdleRecord>,
    /// Threads a batch may use, resolved from the config.
    threads: usize,
    /// How much a batch must promise to save to run on threads.
    parallel_floor: Duration,
    /// Whether each batch ran on threads last time, by batch number.
    threaded: Vec<bool>,
    /// An `after` naming no registered system is a warning, not an error
    /// (`tolerate_missing`): a plugin that did not build is missing.
    tolerate_missing: bool,
    /// The `after` names no system had at the last ordering: (system, name).
    dangling: Vec<(String, String)>,
    /// Event names held over a frozen tick ([`Scheduler::hold_when_frozen`]).
    hold: Vec<String>,
    /// Events held over now, each with the systems still to see it.
    held: Vec<HeldEvent>,
    /// The systems a freeze kept from running this tick.
    skipped: Vec<usize>,
}

/// Something that reads the world at the end of every tick and cannot
/// change it: the daemon's history for `obs.trace`. Called with the frame
/// the tick ran as, the frame its events carry.
pub type Observer = Box<dyn FnMut(&World, u64)>;

/// One batch of parallel systems (`sim.systems`' batches).
#[derive(Clone, Debug, PartialEq)]
pub struct BatchInfo {
    /// Its number, in run order across stages.
    pub batch: u32,
    /// Its stage.
    pub stage: Stage,
    /// Its systems, in run order.
    pub systems: Vec<String>,
    /// Whether it ran on threads the last time it ran.
    pub threaded: bool,
}

/// Called when a tick has fully ended (the clock already counts it), with
/// the scheduler, so it can take the sim state: the daemon's keyframes.
pub type TickEnd = Box<dyn FnMut(&World, &Scheduler)>;

/// How many recorded idle ticks come before turn `t` and have not run yet
/// (2D staples item 5); the host's journal answers, and counts them run.
pub type IdleDue = Box<dyn FnMut(u64) -> u64>;

/// Idle ticks run live: the turn they come before, and how many.
pub type IdleRecord = Box<dyn FnMut(u64, u64)>;

/// What one tick collects while it runs.
struct TickSamples {
    timings: Vec<SystemSample>,
    spans: Vec<CapturedSpan>,
}

impl Scheduler {
    /// New scheduler with no systems.
    pub fn new(config: SchedulerConfig) -> Scheduler {
        let prof = Profiler::new(config.prof_ticks);
        let threads = resolve_threads(config.threads);
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
            tick_end: None,
            quiet: false,
            frozen: false,
            recent_turns: BTreeSet::new(),
            idling: false,
            idle_due: None,
            idle_record: None,
            threads,
            parallel_floor: PARALLEL_FLOOR,
            threaded: Vec::new(),
            tolerate_missing: false,
            dangling: Vec::new(),
            hold: Vec::new(),
            held: Vec::new(),
            skipped: Vec::new(),
        }
    }

    /// Hold events of this name over a frozen tick for the systems the
    /// freeze kept from running: each sees the event the next tick it
    /// runs, before that tick's own, and the systems that ran see it once.
    /// For input the host will not send again, a `host.message` above all
    /// (issue 5: a pause froze the tick a model's answer arrived on, and
    /// the turn system never saw it). Other events are a moment of the
    /// sim, and a frozen tick drops them for the frozen systems.
    pub fn hold_when_frozen(&mut self, name: &str) {
        if !self.hold.iter().any(|h| h == name) {
            self.hold.push(name.to_string());
        }
    }

    /// The events held over now, each with the systems still to see it.
    pub fn held(&self) -> &[HeldEvent] {
        &self.held
    }

    /// What is held over for one system, as its context shows it.
    fn owed_to(&self, i: usize) -> Vec<argus_ecs::EventRecord> {
        if self.held.is_empty() {
            return Vec::new();
        }
        let name = &*self.systems[i].name;
        self.held
            .iter()
            .filter(|h| h.owed.iter().any(|o| o == name))
            .map(|h| h.record.clone())
            .collect()
    }

    /// A system has run, and seen what was held over for it.
    fn delivered(&mut self, i: usize) {
        if self.held.is_empty() {
            return;
        }
        let name = self.systems[i].name.clone();
        for h in &mut self.held {
            h.owed.retain(|o| *o != *name);
        }
        self.held.retain(|h| !h.owed.is_empty());
    }

    /// At the end of a frozen tick: this tick's events of a held name, for
    /// the systems the freeze kept from running.
    fn hold_over(&mut self, world: &World, events_before: usize) {
        let mut owed: Vec<String> = self
            .skipped
            .iter()
            .map(|&i| self.systems[i].desc.name.clone())
            .collect();
        owed.sort();
        owed.dedup();
        if owed.is_empty() {
            return;
        }
        for r in &world.events()[events_before.min(world.events().len())..] {
            if matches!(&r.event, Event::Game { name, .. } if self.hold.iter().any(|h| h == name)) {
                self.held.push(HeldEvent {
                    record: r.clone(),
                    owed: owed.clone(),
                });
            }
        }
    }

    /// Let an `after` name a system that is not registered, as a warning
    /// ([`Scheduler::dangling_after`]) rather than an error: the systems
    /// of a plugin that did not build are missing, and the game runs
    /// without them, ordering what it can (phase 5 item 6).
    pub fn tolerate_missing(&mut self, yes: bool) {
        self.tolerate_missing = yes;
        self.order_dirty = true;
    }

    /// `(system, name)` for each `after` the last ordering found no system
    /// for, when they are tolerated.
    pub fn dangling_after(&self) -> &[(String, String)] {
        &self.dangling
    }

    /// Threads a batch of parallel systems may use: 1 keeps every batch on
    /// the sim thread, 0 is one per core. The world after a tick is the
    /// same for any number (§6); only the time it takes changes.
    pub fn set_threads(&mut self, n: usize) {
        self.config.threads = n;
        self.threads = resolve_threads(n);
    }

    /// Threads a batch may use now (0 resolved to the core count).
    pub fn threads(&self) -> usize {
        self.threads
    }

    /// How much time a batch's systems must promise to save, beyond its
    /// longest one, before it runs on threads (200 µs unless set). Zero
    /// sends every batch of two or more to threads: what a test of the
    /// threaded path wants.
    pub fn set_parallel_floor(&mut self, floor: Duration) {
        self.parallel_floor = floor;
    }

    /// The batches of parallel systems in run order, each with its members
    /// and whether it ran on threads last time.
    pub fn batches(&mut self) -> Result<Vec<BatchInfo>> {
        self.ensure_order()?;
        let mut out: Vec<BatchInfo> = Vec::new();
        for stage in &self.order {
            for &i in stage {
                let s = &self.systems[i];
                if s.batch == NO_BATCH {
                    continue;
                }
                match out.last_mut() {
                    Some(b) if b.batch == s.batch => b.systems.push(s.desc.name.clone()),
                    _ => out.push(BatchInfo {
                        batch: s.batch,
                        stage: s.desc.stage,
                        systems: vec![s.desc.name.clone()],
                        threaded: self
                            .threaded
                            .get(s.batch as usize)
                            .copied()
                            .unwrap_or(false),
                    }),
                }
            }
        }
        Ok(out)
    }

    /// Call `f` at the end of every tick, after the clock counts it.
    pub fn on_tick_end(&mut self, f: TickEnd) {
        self.tick_end = Some(f);
    }

    /// The host's journal of idle ticks (2D staples item 5): `due` says how
    /// many recorded idle ticks come before a turn and have not run (a
    /// seek or a replay runs them first), `record` keeps the ones run live.
    pub fn on_idle(&mut self, due: IdleDue, record: IdleRecord) {
        self.idle_due = Some(due);
        self.idle_record = Some(record);
    }

    /// Idle ticks (2D staples item 5): in a turn-based scene, `n` ticks of
    /// the continuous clock between turns in which only the systems that
    /// move between turns run (animators, particles, tweens, the camera),
    /// so the game moves while it waits. No turn passes and the world's
    /// frame stays. They are recorded for the coming turn, so a seek or a
    /// replay runs the same before it. A continuous scene has none.
    pub fn idle(&mut self, world: &mut World, n: u64) -> Result<()> {
        if self.config.driving != ClockKind::Discrete || n == 0 {
            return Ok(());
        }
        for _ in 0..n {
            self.run_idle_tick(world)?;
        }
        let turn = self.discrete.tick;
        if let Some(f) = self.idle_record.as_mut() {
            f(turn, n);
        }
        Ok(())
    }

    /// Ticks of the continuous clock run between turns so far.
    pub fn idle_ticks(&self) -> u64 {
        self.continuous.tick
    }

    fn run_idle_tick(&mut self, world: &mut World) -> Result<()> {
        self.ensure_order()?;
        let kind = ClockKind::Continuous;
        let tick = self.continuous.tick;
        let dt = self.continuous.timestep;
        let time = self.continuous.time();
        let mut samples = TickSamples {
            timings: Vec::new(),
            spans: Vec::new(),
        };
        self.frozen = false;
        self.skipped.clear();
        self.idling = true;
        for stage in [Stage::Input, Stage::Sim, Stage::Late] {
            self.run_stage(world, stage, kind, tick, time, dt, &mut samples);
        }
        let propagated = argus_ecs::hierarchy::propagate_all(world);
        if propagated.is_ok() {
            self.run_stage(world, Stage::Hierarchy, kind, tick, time, dt, &mut samples);
        }
        self.idling = false;
        propagated?;
        // Dead references nulled, as at a frame's end, without ending the
        // frame: a turn is a frame.
        world.maintain();
        world.rebuild_broadphase();
        self.continuous.tick += 1;
        Ok(())
    }

    /// Whether the last tick was frozen by a system (a hit-stop).
    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// On a driving discrete clock, the tick the `n`th latest turn ran on:
    /// where a window of the last `n` turns starts (issue 36). A turn is a
    /// tick whose systems ran. A menu's press, a character typed, or the
    /// wheel freezes its tick, which takes no turn though the tick count
    /// moves. `None` before `n` turns, or past the [`RECENT_TURNS`] kept,
    /// or on a continuous clock. Each tick that runs says whether it was a
    /// turn, a seek's re-run included, so the ticks before now are the run
    /// as it stands: a seek back and forward leaves the turns it skipped as
    /// they were, and a branch's turns replace the old ones as they run.
    pub fn turns_back(&self, n: usize) -> Option<u64> {
        let now = self.discrete.tick;
        n.checked_sub(1)
            .and_then(|k| self.recent_turns.range(..now).rev().nth(k).copied())
    }

    /// How many turns ran from tick `from` on (at most [`RECENT_TURNS`]).
    pub fn turns_since(&self, from: u64) -> usize {
        let now = self.discrete.tick;
        if from >= now {
            return 0;
        }
        self.recent_turns.range(from..now).count()
    }

    /// A discrete tick ran, and was a turn if its systems ran.
    fn note_turn(&mut self, tick: u64, turn: bool) {
        if turn {
            self.recent_turns.insert(tick);
            while self.recent_turns.len() > RECENT_TURNS {
                self.recent_turns.pop_first();
            }
        } else {
            self.recent_turns.remove(&tick);
        }
    }

    /// Quiet ticks run the systems but not the observers, the tick-end
    /// hook, or the profiler: a seek replaying ticks that already happened
    /// (their history, keyframes, and timings are the first run's).
    pub fn set_quiet(&mut self, quiet: bool) {
        self.quiet = quiet;
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
        self.add_body(desc, Body::Serial(run))
    }

    /// Register a parallel system (phase 5 item 5): a `Send` body that sees
    /// the world through a [`ParCtx`], so the scheduler may run it beside
    /// the parallel systems next to it in run order that it does not
    /// conflict with. It may not be exclusive; its declaration is final
    /// (it cannot declare more as it runs).
    pub fn add_parallel_system(&mut self, desc: SystemDesc, run: ParSystemFn) -> Result<()> {
        self.add_body(desc, Body::Parallel(run))
    }

    fn check_parallel(desc: &mut SystemDesc, body: &Body) -> Result<()> {
        desc.parallel = matches!(body, Body::Parallel(_));
        if desc.parallel && desc.exclusive {
            return Err(ArgusError::new(
                crate::codes::NOT_PARALLEL,
                format!(
                    "system {} is parallel and exclusive; an exclusive system changes the whole world",
                    desc.name
                ),
            )
            .hint("Register a loader with add_system; a parallel system writes its declared components.")
            .at(desc.source.clone()));
        }
        Ok(())
    }

    fn add_body(&mut self, mut desc: SystemDesc, run: Body) -> Result<()> {
        Self::check_parallel(&mut desc, &run)?;
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
            faulted: false,
            rng,
            batch: NO_BATCH,
            cost: Duration::ZERO,
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
        self.replace_body(desc, Body::Serial(run))
    }

    /// [`Scheduler::replace_system`] for a parallel system.
    pub fn replace_parallel_system(&mut self, desc: SystemDesc, run: ParSystemFn) -> Result<()> {
        self.replace_body(desc, Body::Parallel(run))
    }

    fn replace_body(&mut self, mut desc: SystemDesc, run: Body) -> Result<()> {
        let Some(i) = self.systems.iter().position(|s| s.desc.name == desc.name) else {
            return self.add_body(desc, run);
        };
        Self::check_parallel(&mut desc, &run)?;
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
        slot.faulted = false;
        self.order_dirty = true;
        Ok(())
    }

    /// Order an installed system after another of its stage, as if its
    /// declaration said so: a module that slots its system between two
    /// of the engine's (the rigid tier between the kinematic move and the
    /// overlap pass, phase 5 item 7).
    pub fn order_after(&mut self, system: &str, after: &str) -> Result<()> {
        let i = self.slot_index(system)?;
        let a = &mut self.systems[i].desc.after;
        if !a.iter().any(|x| x == after) {
            a.push(after.to_string());
            self.order_dirty = true;
        }
        Ok(())
    }

    /// A registered system's declaration site: `Some(None)` for one
    /// declared nowhere in particular (the engine's own), `None` when no
    /// system has the name.
    pub fn source_of(&self, name: &str) -> Option<Option<&str>> {
        self.systems
            .iter()
            .find(|s| s.desc.name == name)
            .map(|s| s.desc.source.as_deref())
    }

    /// Re-enable a system (after a fault, or a `disable`).
    pub fn enable(&mut self, name: &str) -> Result<()> {
        let i = self.slot_index(name)?;
        self.systems[i].enabled = true;
        self.systems[i].disabled_reason = None;
        self.systems[i].faulted = false;
        Ok(())
    }

    /// Switch on again every system a fault switched off, not one a scene's
    /// settings did: what a reload does, so a system faulted by the run's
    /// data (a model's answer) runs again with no file changed (issue 13).
    /// Returns their names.
    pub fn enable_faulted(&mut self) -> Vec<String> {
        let mut on = Vec::new();
        for s in self.systems.iter_mut().filter(|s| s.faulted) {
            s.enabled = true;
            s.disabled_reason = None;
            s.faulted = false;
            on.push(s.desc.name.clone());
        }
        on
    }

    /// Disable a system.
    pub fn disable(&mut self, name: &str, reason: &str) -> Result<()> {
        let i = self.slot_index(name)?;
        self.systems[i].enabled = false;
        self.systems[i].disabled_reason = Some(reason.to_string());
        self.systems[i].faulted = false;
        Ok(())
    }

    /// Enabled?
    pub fn is_enabled(&self, name: &str) -> Result<bool> {
        Ok(self.systems[self.slot_index(name)?].enabled)
    }

    /// Is any system off, by a fault or by request?
    pub fn any_disabled(&self) -> bool {
        self.systems.iter().any(|s| !s.enabled)
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
                    before: s.desc.before.clone(),
                    reads: names(&s.desc.access.reads),
                    writes: names(&writes_of(&s.desc)),
                    clock: s.desc.clock,
                    rng: s.desc.rng.clone(),
                    enabled: s.enabled,
                    disabled_reason: s.disabled_reason.clone(),
                    source: s.desc.source.clone(),
                    doc: s.desc.doc.clone(),
                    last: s.desc.last,
                    on_load: s.desc.on_load,
                    first: s.desc.first,
                    unfrozen: s.desc.unfrozen,
                    between_turns: s.desc.between_turns,
                    parallel: s.desc.parallel,
                    batch: (s.batch != NO_BATCH).then_some(s.batch),
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
            .filter(|&&i| {
                let d = &self.systems[i].desc;
                d.access.allows_write(c) || d.spawns.contains(&c)
            })
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
        let mut dangling = Vec::new();
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
                let d = &self.systems[i].desc;
                // `after` is an edge from the other system to this one,
                // `before` from this one to the other.
                let deps = d
                    .after
                    .iter()
                    .map(|n| (n, "after"))
                    .chain(d.before.iter().map(|n| (n, "before")));
                for (dep, word) in deps {
                    if self.tolerate_missing && !by_name.contains_key(dep.as_str()) {
                        dangling.push((d.name.clone(), dep.clone()));
                        continue;
                    }
                    let &j = by_name.get(dep.as_str()).ok_or_else(|| {
                        ArgusError::new(
                            crate::codes::UNKNOWN_SYSTEM,
                            format!(
                                "system {} runs {word} {dep:?}, which is not registered",
                                d.name
                            ),
                        )
                        .hint("Check the name, or register the other system first.")
                        .at(d.source.clone())
                    })?;
                    if self.systems[j].desc.stage != stage {
                        return Err(ArgusError::new(
                            crate::codes::ORDER_CYCLE,
                            format!(
                                "system {} runs {word} {dep:?}, which is in stage {} not {}",
                                d.name, self.systems[j].desc.stage, stage
                            ),
                        )
                        .hint("after and before only order systems within one stage; stages already order each other.")
                        .at(d.source.clone()));
                    }
                    let (from, to) = if word == "after" { (j, i) } else { (i, j) };
                    edges.entry(from).or_default().push(to);
                    *indeg.get_mut(&to).unwrap() += 1;
                }
            }
            let mut ready: Vec<usize> = members.iter().copied().filter(|i| indeg[i] == 0).collect();
            let mut out = Vec::with_capacity(members.len());
            let systems = &self.systems;
            let key = |i: &usize| (systems[*i].desc.last, !systems[*i].desc.first, *i);
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
                    format!(
                        "after and before constraints form a cycle among: {}",
                        stuck.join(", ")
                    ),
                )
                .hint("Remove one after or before entry so the stage has a linear order."));
            }
            order[stage.index()] = out;
        }
        self.order = order;
        self.dangling = dangling;
        self.plan_batches();
        self.order_dirty = false;
        Ok(())
    }

    /// Group the parallel systems into batches (§3 "The scheduler uses this
    /// for parallelism"): walking each stage in run order, a parallel
    /// system joins the batch before it when the batch is the system just
    /// before it in the order, nothing in the batch writes what it reads or
    /// writes or reads what it writes, and no `after` names one from the
    /// other. Otherwise it starts a batch. The checks are static, from the
    /// declarations; the order is the stage's, unchanged.
    fn plan_batches(&mut self) {
        for s in &mut self.systems {
            s.batch = NO_BATCH;
        }
        let mut next = 0u32;
        for stage in &self.order {
            let mut members: Vec<usize> = Vec::new();
            for &i in stage {
                if !matches!(self.systems[i].run, Body::Parallel(_)) {
                    members.clear();
                    continue;
                }
                let me = &self.systems[i].desc;
                let fits = !members.is_empty()
                    && members.iter().all(|&j| {
                        let other = &self.systems[j].desc;
                        !conflicts(&me.access, &other.access)
                            && !me.after.contains(&other.name)
                            && !other.after.contains(&me.name)
                            && !me.before.contains(&other.name)
                            && !other.before.contains(&me.name)
                    });
                if !fits {
                    members.clear();
                    next += 1;
                }
                members.push(i);
                self.systems[i].batch = next - 1;
            }
        }
        self.threaded = vec![false; next as usize];
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
        // A turn-based scene in play moves between turns (2D staples item
        // 5): an idle tick a frame.
        if self.playing && !self.paused && self.config.driving == ClockKind::Discrete {
            self.idle(world, 1)?;
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
        // A turn re-run by a seek or a replay: the idle ticks recorded
        // before it first (2D staples item 5).
        if kind == ClockKind::Discrete && self.config.driving == ClockKind::Discrete {
            let turn = self.discrete.tick;
            let owed = self.idle_due.as_mut().map_or(0, |f| f(turn));
            for _ in 0..owed {
                self.run_idle_tick(world)?;
            }
        }
        let started = Instant::now();
        let (a0, b0) = alloc::counters();
        let tick = self.clock(kind).tick;
        let dt = self.clock(kind).timestep;
        let time = self.clock(kind).time();
        let mut samples = TickSamples {
            timings: Vec::new(),
            spans: Vec::new(),
        };
        self.frozen = false;
        self.skipped.clear();
        let events_before = world.events().len();
        for stage in [Stage::Input, Stage::Sim, Stage::Late] {
            self.run_stage(world, stage, kind, tick, time, dt, &mut samples);
        }
        if self.frozen && !self.hold.is_empty() {
            self.hold_over(world, events_before);
        }
        self.apply_speed_requests(world, events_before);
        if kind == ClockKind::Discrete && self.config.driving == ClockKind::Discrete {
            self.note_turn(tick, !self.frozen);
        }
        {
            let t = Instant::now();
            let (pa, pb) = alloc::counters();
            argus_ecs::hierarchy::propagate_all(world)?;
            let (qa, qb) = alloc::counters();
            self.sample(
                &mut samples,
                Arc::from("hierarchy.propagate"),
                t,
                qa - pa,
                qb - pb,
                0,
            );
        }
        self.run_stage(world, Stage::Hierarchy, kind, tick, time, dt, &mut samples);
        if !self.observers.is_empty() && !self.quiet {
            let t = Instant::now();
            let (pa, pb) = alloc::counters();
            let frame = world.frame();
            for o in &mut self.observers {
                o(world, frame);
            }
            let (qa, qb) = alloc::counters();
            self.sample(
                &mut samples,
                Arc::from("obs.observe"),
                t,
                qa - pa,
                qb - pb,
                0,
            );
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
                0,
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
        if !self.quiet {
            self.prof.record(TickRecord {
                tick,
                clock: kind,
                total,
                systems: samples.timings,
                allocs: a1 - a0,
                bytes: b1 - b0,
                archetypes,
            });
        }
        self.clock_mut(kind).tick += 1;
        if !self.quiet {
            if let Some(mut f) = self.tick_end.take() {
                f(world, self);
                self.tick_end = Some(f);
            }
        }
        Ok(())
    }

    /// A game's `speed.requested` (Luau's `q:speed(x)`) takes effect at the
    /// end of the tick that asked, here, inside the run: every host (the
    /// daemon, a release build, the page) and every replay of the tick
    /// sees the same clock after it, so a game may read its speed. The
    /// frame that is running keeps the count of ticks it began with.
    fn apply_speed_requests(&mut self, world: &World, from: usize) {
        let evs = world.events();
        let asked = evs[from.min(evs.len())..]
            .iter()
            .rev()
            .find_map(|r| match &r.event {
                Event::Game { name, fields } if name == "speed.requested" => fields
                    .iter()
                    .find(|(k, _)| k == "x")
                    .and_then(|(_, v)| match v {
                        argus_ecs::Value::Float(f) => Some(*f),
                        argus_ecs::Value::Int(i) => Some(*i as f64),
                        argus_ecs::Value::UInt(u) => Some(*u as f64),
                        _ => None,
                    }),
                _ => None,
            });
        if let Some(x) = asked.filter(|x| *x > 0.0) {
            // A discrete scene has no speed to set.
            let _ = self.continuous.set_speed(x);
        }
    }

    /// Record one system's sample for the profiler ring and, while a
    /// capture is collecting, its span on the capture's time base.
    fn sample(
        &self,
        s: &mut TickSamples,
        name: Arc<str>,
        t: Instant,
        allocs: u64,
        bytes: u64,
        lane: u32,
    ) {
        self.sample_span(s, name, t, t.elapsed(), allocs, bytes, lane);
    }

    /// [`Scheduler::sample`] for a span already timed (on another thread:
    /// `lane` is the batch's thread, 0 the sim thread's).
    #[allow(clippy::too_many_arguments)]
    fn sample_span(
        &self,
        s: &mut TickSamples,
        name: Arc<str>,
        t: Instant,
        elapsed: Duration,
        allocs: u64,
        bytes: u64,
        lane: u32,
    ) {
        if let Some(c) = &self.capture {
            if !c.complete() {
                s.spans.push(CapturedSpan {
                    name: name.clone(),
                    start_us: c.us(t),
                    dur_us: elapsed.as_secs_f64() * 1e6,
                    allocs,
                    bytes,
                    lane,
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
        let keep = |s: &Scheduler, i: usize| {
            if s.idling {
                return s.systems[i].enabled && s.systems[i].desc.between_turns;
            }
            s.systems[i].enabled && s.binds(i, kind) && (!s.frozen || s.systems[i].desc.unfrozen)
        };
        // Would have run but for a freeze: what is held over is for these.
        let frozen_out = |s: &Scheduler, i: usize| {
            !s.idling
                && s.frozen
                && s.systems[i].enabled
                && s.binds(i, kind)
                && !s.systems[i].desc.unfrozen
        };
        // On a turn, what moves between turns takes no time (2D staples
        // item 5): its time passes on the idle ticks.
        let turn = kind == ClockKind::Discrete && self.config.driving == ClockKind::Discrete;
        let dt_of = |s: &Scheduler, i: usize| {
            if turn && s.systems[i].desc.between_turns {
                0.0
            } else {
                dt
            }
        };
        let mut at = 0;
        while at < order.len() {
            let i = order[at];
            let batch = self.systems[i].batch;
            if batch != NO_BATCH {
                let end = batch_end(&self.systems, &order, at);
                let out: Vec<usize> = order[at..end]
                    .iter()
                    .copied()
                    .filter(|&j| frozen_out(self, j))
                    .collect();
                self.skipped.extend(out);
                let members: Vec<usize> = order[at..end]
                    .iter()
                    .copied()
                    .filter(|&j| keep(self, j))
                    .collect();
                let samples = Some(&mut *samples);
                self.run_batch(&members, batch, world, stage, kind, tick, time, dt, samples);
                at = end;
                continue;
            }
            at += 1;
            if !keep(self, i) {
                if frozen_out(self, i) {
                    self.skipped.push(i);
                }
                continue;
            }
            let t = Instant::now();
            let (a0, b0) = alloc::counters();
            let dt = dt_of(self, i);
            self.run_system(i, world, stage, kind, tick, time, dt);
            let (a1, b1) = alloc::counters();
            let name = self.systems[i].name.clone();
            self.systems[i].cost = t.elapsed();
            self.sample(samples, name, t, a1 - a0, b1 - b0, 0);
        }
    }

    /// Run the parallel systems `members` of one batch (already filtered to
    /// those that run this tick): on threads when they promise to save
    /// time, else one after another on this thread. Either way they share
    /// the world as the batch began, with the string table sealed, and
    /// what each deferred is applied after the last of them, in run order.
    #[allow(clippy::too_many_arguments)]
    fn run_batch(
        &mut self,
        members: &[usize],
        batch: u32,
        world: &mut World,
        stage: Stage,
        kind: ClockKind,
        tick: u64,
        time: f64,
        dt: f64,
        mut samples: Option<&mut TickSamples>,
    ) {
        if members.is_empty() {
            return;
        }
        let t = Tick {
            stage,
            kind,
            tick,
            time,
            dt,
            speed: self.continuous.speed,
            frozen: self.frozen,
        };
        let costs: Vec<Duration> = members.iter().map(|&i| self.systems[i].cost).collect();
        let longest = costs.iter().copied().max().unwrap_or_default();
        let total: Duration = costs.iter().sum();
        let lanes = self.threads.min(members.len());
        let threaded = lanes > 1 && total.saturating_sub(longest) >= self.parallel_floor;
        if let Some(slot) = self.threaded.get_mut(batch as usize) {
            *slot = threaded;
        }
        // What is held over for each, before the slots are borrowed.
        let mut owed: Vec<(usize, Vec<argus_ecs::EventRecord>)> =
            members.iter().map(|&i| (i, self.owed_to(i))).collect();
        // Each member's body, stream, and declaration, borrowed apart.
        let mut jobs: Vec<Job<'_>> = Vec::with_capacity(members.len());
        for (i, slot) in self.systems.iter_mut().enumerate() {
            if !members.contains(&i) {
                continue;
            }
            let SystemSlot {
                desc,
                name,
                run,
                rng,
                ..
            } = slot;
            let Body::Parallel(run) = run else {
                unreachable!("a batch holds parallel systems");
            };
            let held = owed
                .iter_mut()
                .find(|(m, _)| *m == i)
                .map(|(_, h)| std::mem::take(h))
                .unwrap_or_default();
            jobs.push(Job {
                slot: i,
                run,
                rng: rng.as_mut(),
                access: &desc.access,
                spawns: &desc.spawns,
                name,
                held,
                out: Deferred::default(),
                outcome: None,
                start: None,
                elapsed: Duration::ZERO,
                allocs: (0, 0),
                lane: 0,
            });
        }
        jobs.sort_by_key(|j| members.iter().position(|&m| m == j.slot));
        let shared: &World = world;
        shared.strings().seal();
        if threaded {
            // Longest first, each to the least loaded lane; lane 0 is this
            // thread. Which lane runs what changes nothing but the time.
            let mut order: Vec<usize> = (0..jobs.len()).collect();
            order.sort_by_key(|&k| std::cmp::Reverse(costs[k]));
            let mut load = vec![Duration::ZERO; lanes];
            let mut lane_of = vec![0usize; jobs.len()];
            for k in order {
                let l = (0..lanes).min_by_key(|&l| load[l]).unwrap_or(0);
                load[l] += costs[k].max(Duration::from_nanos(1));
                lane_of[k] = l;
            }
            let mut groups: Vec<Vec<&mut Job<'_>>> = (0..lanes).map(|_| Vec::new()).collect();
            for (k, job) in jobs.iter_mut().enumerate() {
                groups[lane_of[k]].push(job);
            }
            let mut groups = groups.into_iter();
            let mine = groups.next().unwrap_or_default();
            std::thread::scope(|scope| {
                for (l, group) in groups.enumerate() {
                    scope.spawn(move || {
                        for job in group {
                            job.run(shared, t, l as u32 + 1);
                        }
                    });
                }
                for job in mine {
                    job.run(shared, t, 0);
                }
            });
        } else {
            for job in &mut jobs {
                job.run(shared, t, 0);
            }
        }
        shared.strings().unseal();
        // Back on the sim thread, in run order: samples, then each one's
        // deferred changes, then its fault if it had one.
        let done: Vec<_> = jobs
            .into_iter()
            .map(|j| {
                (
                    j.slot, j.out, j.outcome, j.start, j.elapsed, j.allocs, j.lane,
                )
            })
            .collect();
        for (i, out, outcome, start, elapsed, (allocs, bytes), lane) in done {
            self.systems[i].cost = elapsed;
            let name = self.systems[i].name.clone();
            if let (Some(s), Some(start)) = (samples.as_deref_mut(), start) {
                self.sample_span(s, name.clone(), start, elapsed, allocs, bytes, lane);
            }
            out.apply(world, name);
            self.delivered(i);
            if let Some(outcome) = outcome {
                self.settle(i, world, outcome);
            }
        }
    }

    /// Run, once, every enabled system declared `on_load`, in stage order,
    /// on the world as a scene load left it: before the first tick, at its
    /// tick and frame, from the same RNG streams. The design's stages run on
    /// ticks only, so a discrete game that generates its floor in `sim`
    /// showed an empty screen until the first key (found when Kevin played
    /// Delve). Returns how many ran.
    pub fn run_load(&mut self, world: &mut World) -> Result<usize> {
        self.ensure_order()?;
        let kind = self.config.driving;
        let (tick, time, dt) = (
            self.driving().tick,
            self.driving().time(),
            self.driving().timestep,
        );
        let mut ran = 0;
        for stage in [Stage::Input, Stage::Sim, Stage::Late, Stage::Hierarchy] {
            let order = self.order[stage.index()].clone();
            let keep = |s: &Scheduler, i: usize| {
                let x = &s.systems[i];
                x.enabled && x.desc.on_load && s.binds(i, kind)
            };
            let mut at = 0;
            while at < order.len() {
                let i = order[at];
                let batch = self.systems[i].batch;
                if batch != NO_BATCH {
                    let end = batch_end(&self.systems, &order, at);
                    let members: Vec<usize> = order[at..end]
                        .iter()
                        .copied()
                        .filter(|&j| keep(self, j))
                        .collect();
                    ran += members.len();
                    self.run_batch(&members, batch, world, stage, kind, tick, time, dt, None);
                    at = end;
                    continue;
                }
                at += 1;
                if keep(self, i) {
                    self.run_system(i, world, stage, kind, tick, time, dt);
                    ran += 1;
                }
            }
        }
        if ran > 0 {
            argus_ecs::hierarchy::propagate_all(world)?;
            world.rebuild_broadphase();
        }
        Ok(ran)
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
        let mut at = 0;
        while at < order.len() {
            let i = order[at];
            let batch = self.systems[i].batch;
            if batch != NO_BATCH {
                let end = batch_end(&self.systems, &order, at);
                let members: Vec<usize> = order[at..end]
                    .iter()
                    .copied()
                    .filter(|&j| self.systems[j].enabled)
                    .collect();
                ran |= !members.is_empty();
                let stage = Stage::Present;
                self.run_batch(&members, batch, world, stage, kind, tick, time, dt, None);
                at = end;
                continue;
            }
            at += 1;
            if self.systems[i].enabled {
                self.run_system(i, world, Stage::Present, kind, tick, time, dt);
                ran = true;
            }
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
        let froze;
        let frozen = self.frozen;
        let speed = self.continuous.speed;
        let held = self.owed_to(i);
        let slot = &mut self.systems[i];
        let Body::Serial(run) = &mut slot.run else {
            // A parallel system outside a batch runs as a batch of one.
            let batch = slot.batch;
            self.run_batch(&[i], batch, world, stage, kind, tick, time, dt, None);
            return;
        };
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
                speed,
                learned: Vec::new(),
                frozen,
                freeze: false,
                exclusive: slot.desc.exclusive,
                held: &held,
            };
            // What the system emits says it did (obs.why).
            ctx.world.set_emitter(Some(slot.name.clone()));
            let r = catch_unwind(AssertUnwindSafe(|| run(&mut ctx)));
            ctx.world.set_emitter(None);
            let learned = std::mem::take(&mut ctx.learned);
            froze = ctx.freeze;
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
        if froze {
            self.frozen = true;
        }
        self.delivered(i);
        self.settle(i, world, outcome);
    }

    /// A system's run is over: if it failed, switch it off and say so.
    fn settle(&mut self, i: usize, world: &mut World, outcome: std::thread::Result<Result<()>>) {
        let slot = &mut self.systems[i];
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
        slot.faulted = true;
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
            disabled: self
                .systems
                .iter()
                .filter(|s| !s.enabled)
                .map(|s| {
                    (
                        s.desc.name.clone(),
                        s.disabled_reason.clone().unwrap_or_default(),
                    )
                })
                .collect(),
            held: self.held.clone(),
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
        self.held = state.held.clone();
        // A system a fault switched off at frame 300 was on at frame 200.
        for s in &mut self.systems {
            match state.disabled.iter().find(|(n, _)| *n == s.desc.name) {
                Some((_, why)) => {
                    // Off by the scene's settings now, it was then too.
                    s.faulted = s.faulted || s.enabled;
                    s.enabled = false;
                    s.disabled_reason = Some(why.clone());
                }
                None => {
                    s.enabled = true;
                    s.disabled_reason = None;
                    s.faulted = false;
                }
            }
        }
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

    /// The last `last` ticks' numbers and whole times in ms, oldest first.
    pub fn prof_totals(&self, last: usize) -> Vec<(u64, f64)> {
        self.prof.totals(last)
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

/// What a system changes: its declared writes, then what it spawns.
fn writes_of(desc: &SystemDesc) -> Vec<ComponentId> {
    let mut w = desc.access.writes.clone();
    for &c in &desc.spawns {
        if !w.contains(&c) {
            w.push(c);
        }
    }
    w
}

/// 0 is one thread per core. The page has one thread: a browser starts no
/// threads for wasm without shared memory, so every batch runs on it.
fn resolve_threads(n: usize) -> usize {
    if cfg!(target_arch = "wasm32") {
        return 1;
    }
    match n {
        0 => std::thread::available_parallelism().map_or(1, |n| n.get()),
        n => n,
    }
}

/// Whether two declarations may run at once: neither writes what the other
/// reads or writes.
fn conflicts(a: &Access, b: &Access) -> bool {
    a.writes.iter().any(|&c| b.allows_read(c)) || b.writes.iter().any(|&c| a.allows_read(c))
}

/// One past the last position of the batch that starts at `at` in `order`.
fn batch_end(systems: &[SystemSlot], order: &[usize], at: usize) -> usize {
    let batch = systems[order[at]].batch;
    let mut end = at + 1;
    while end < order.len() && systems[order[end]].batch == batch {
        end += 1;
    }
    end
}
