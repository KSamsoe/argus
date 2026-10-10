//! System declarations and the context a system runs with.
//!
//! A system declares its stage, what it runs after, what it reads and
//! writes, which clock it binds to, and an optional RNG stream (§3, §6).
//! The context hands the system a world view that enforces those
//! declarations: queries carry the access set, structural changes check it,
//! randomness comes only from the declared stream, and time is sim time.

use crate::clock::ClockKind;
use crate::rng::Pcg32;
use argus_ecs::{
    codes, Access, ArgusError, Component, ComponentId, Entity, Event, Query, Result, Value, World,
};
use std::fmt;

/// The fixed stage order (§3). `sim` runs on the game clock and is the only
/// stage that may mutate gameplay state; `present` is read-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Stage {
    /// Input actions land here (action maps, injected input).
    Input,
    /// Gameplay. Runs on the driving clock.
    Sim,
    /// Cleanup after sim: despawn queues, event fan-out.
    Late,
    /// Transform propagation. `argus_ecs::propagate` runs first, then systems here.
    Hierarchy,
    /// Read-only: interpolation, camera, UI. Runs every render frame.
    Present,
}

impl Stage {
    /// All stages in execution order.
    pub const ALL: [Stage; 5] = [
        Stage::Input,
        Stage::Sim,
        Stage::Late,
        Stage::Hierarchy,
        Stage::Present,
    ];

    /// Position in the order.
    pub fn index(self) -> usize {
        self as usize
    }

    /// Parse the lowercase name used in script declarations.
    pub fn parse(s: &str) -> Option<Stage> {
        Some(match s {
            "input" => Stage::Input,
            "sim" => Stage::Sim,
            "late" => Stage::Late,
            "hierarchy" => Stage::Hierarchy,
            "present" => Stage::Present,
            _ => return None,
        })
    }

    /// Lowercase name.
    pub fn name(self) -> &'static str {
        match self {
            Stage::Input => "input",
            Stage::Sim => "sim",
            Stage::Late => "late",
            Stage::Hierarchy => "hierarchy",
            Stage::Present => "present",
        }
    }

    /// True for stages that run on a clock tick (everything but `present`).
    pub fn is_tick_stage(self) -> bool {
        self != Stage::Present
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Which clock's ticks run a system (§6 "a system group binds to either clock").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ClockBinding {
    /// Whichever clock the scene declared drives `sim`.
    #[default]
    Driving,
    /// The continuous clock, even in a discrete-driven scene.
    Continuous,
    /// The discrete clock, even in a continuous-driven scene.
    Discrete,
}

impl ClockBinding {
    /// Resolve against the scene's driving clock.
    pub fn resolve(self, driving: ClockKind) -> ClockKind {
        match self {
            ClockBinding::Driving => driving,
            ClockBinding::Continuous => ClockKind::Continuous,
            ClockBinding::Discrete => ClockKind::Discrete,
        }
    }
}

/// Everything a system declares about itself.
#[derive(Clone, Debug, PartialEq)]
pub struct SystemDesc {
    /// Unique name, dotted like `sim.hitstop`.
    pub name: String,
    /// Stage.
    pub stage: Stage,
    /// Systems in the same stage that must run before this one.
    pub after: Vec<String>,
    /// Declared reads and writes.
    pub access: Access,
    /// Clock binding.
    pub clock: ClockBinding,
    /// Named RNG stream, if the system uses randomness.
    pub rng: Option<String>,
    /// `file:line` of the declaration.
    pub source: Option<String>,
    /// What the system does.
    pub doc: String,
    /// Run after every other system of the stage that does not order itself
    /// after this one (the physics step: gameplay sets velocities, then
    /// bodies move). `after` constraints still hold.
    pub last: bool,
    /// Also run once when a scene loads, before the first tick
    /// (`Scheduler::run_load`): a game that builds its world from the seed
    /// shows it before the first command.
    pub on_load: bool,
    /// Run before every other system of the stage that does not order
    /// itself before this one (a hit-stop decides, first, whether the tick
    /// runs at all).
    pub first: bool,
    /// Run while the sim is frozen (a hit-stop, phase 2 item 6): the post
    /// effects that play during the stop. Every other system skips a
    /// frozen tick.
    pub unfrozen: bool,
    /// May change the world in any way through [`SystemCtx::world_mut`]:
    /// a loader that spawns and despawns whole rooms (phase 3 item 6). It
    /// declares what it wrote as it goes ([`SystemCtx::declare`]).
    pub exclusive: bool,
    /// Registered with a parallel body (`Scheduler::add_parallel_system`);
    /// the scheduler sets it.
    pub parallel: bool,
    /// Components a parallel system spawns, inserts, or removes. Those
    /// changes wait for the end of its batch, so they do not keep it apart
    /// from systems that use the same columns; they are writes for
    /// `who_writes`.
    pub spawns: Vec<ComponentId>,
}

impl SystemDesc {
    /// New declaration with no ordering, no access, driving clock.
    pub fn new(name: impl Into<String>, stage: Stage) -> SystemDesc {
        SystemDesc {
            name: name.into(),
            stage,
            after: Vec::new(),
            access: Access::new(),
            clock: ClockBinding::Driving,
            rng: None,
            source: None,
            doc: String::new(),
            last: false,
            on_load: false,
            first: false,
            unfrozen: false,
            exclusive: false,
            parallel: false,
            spawns: Vec::new(),
        }
    }

    /// Declare a component a parallel system spawns with, inserts, or
    /// removes (applied after its batch).
    pub fn spawns(mut self, c: ComponentId) -> Self {
        self.spawns.push(c);
        self
    }

    /// May change the world in any way ([`SystemCtx::world_mut`]).
    pub fn exclusive(mut self) -> Self {
        self.exclusive = true;
        self
    }

    /// Run first in the stage.
    pub fn first(mut self) -> Self {
        self.first = true;
        self
    }

    /// Keep running while the sim is frozen.
    pub fn unfrozen(mut self) -> Self {
        self.unfrozen = true;
        self
    }

    /// Also run once when a scene loads, before the first tick.
    pub fn on_load(mut self) -> Self {
        self.on_load = true;
        self
    }
    /// Run at the end of the stage: after every system that is not itself
    /// last and does not order itself after this one.
    pub fn last(mut self) -> Self {
        self.last = true;
        self
    }
    /// Run after another system in the same stage.
    pub fn after(mut self, name: impl Into<String>) -> Self {
        self.after.push(name.into());
        self
    }
    /// Declare a read.
    pub fn reads(mut self, c: ComponentId) -> Self {
        self.access.reads.push(c);
        self
    }
    /// Declare a write.
    pub fn writes(mut self, c: ComponentId) -> Self {
        self.access.writes.push(c);
        self
    }
    /// Replace the access set.
    pub fn access(mut self, access: Access) -> Self {
        self.access = access;
        self
    }
    /// Bind to a clock.
    pub fn clock(mut self, clock: ClockBinding) -> Self {
        self.clock = clock;
        self
    }
    /// Declare an RNG stream.
    pub fn rng(mut self, stream: impl Into<String>) -> Self {
        self.rng = Some(stream.into());
        self
    }
    /// Declaration site.
    pub fn source(mut self, src: impl Into<String>) -> Self {
        self.source = Some(src.into());
        self
    }
    /// Doc string.
    pub fn doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = doc.into();
        self
    }
}

/// A system body.
pub type SystemFn = Box<dyn FnMut(&mut SystemCtx<'_>) -> Result<()>>;

/// The world as one system is allowed to see it.
pub struct SystemCtx<'a> {
    pub(crate) world: &'a mut World,
    pub(crate) access: &'a Access,
    pub(crate) rng: Option<&'a mut Pcg32>,
    /// System name.
    pub name: &'a str,
    /// Stage being run.
    pub stage: Stage,
    /// Clock whose tick this is (for `present`, the driving clock).
    pub clock: ClockKind,
    /// Tick number on that clock, starting at 0.
    pub tick: u64,
    /// Sim time on that clock: tick × timestep. Never wall-clock.
    pub time: f64,
    /// Timestep on that clock.
    pub dt: f64,
    /// The continuous clock's speed as this tick began: ticks a frame
    /// (Luau's `q:speed()`). Part of the run: a game's request for another
    /// speed takes effect at the end of the tick that asked, and a change
    /// from outside is a change between ticks, so a replay sees the same.
    pub speed: f64,
    /// Components this run declared it touches (see [`SystemCtx::declare`]).
    pub(crate) learned: Vec<(ComponentId, bool)>,
    /// The tick is frozen (a hit-stop, phase 2 item 6): only `unfrozen`
    /// systems run, and they see this true. A camera holds its view and
    /// still shakes.
    pub frozen: bool,
    /// Set by [`SystemCtx::freeze`].
    pub(crate) freeze: bool,
    /// The system is exclusive ([`SystemDesc::exclusive`]).
    pub(crate) exclusive: bool,
}

impl<'a> SystemCtx<'a> {
    /// Declare that this system reads (or writes) a component its data
    /// turned out to name: a tween's target field, an animator's condition.
    /// The declaration joins the system's access after this run, so
    /// `who_writes` and `who_reads` stay true for data-driven systems (§3).
    /// Cheap to repeat; a system usually declares each component once.
    pub fn declare(&mut self, c: ComponentId, write: bool) {
        let known = if write {
            self.access.allows_write(c)
        } else {
            self.access.allows_read(c)
        };
        if !known && !self.learned.contains(&(c, write)) {
            self.learned.push((c, write));
        }
    }

    /// Freeze the rest of this tick: every system after this one that is
    /// not `unfrozen` skips it (a hit-stop, phase 2 item 6). The tick still
    /// counts, so input and replays stay on their ticks.
    pub fn freeze(&mut self) {
        self.freeze = true;
    }

    /// Read-only world access (no declared-access check; use `query` for iteration).
    pub fn world(&self) -> &World {
        self.world
    }

    /// The whole world, to change as it must: only for a system declared
    /// [`SystemDesc::exclusive`] (a loader). It declares what it writes.
    pub fn world_mut(&mut self) -> Result<&mut World> {
        if !self.exclusive {
            return Err(ArgusError::new(
                crate::codes::NOT_EXCLUSIVE,
                format!("system {} is not exclusive; world_mut is for loaders", self.name),
            )
            .hint("Declare the system with SystemDesc::exclusive(), or write through the declared components."));
        }
        Ok(self.world)
    }

    /// The declared access set.
    pub fn access(&self) -> &Access {
        self.access
    }

    /// The world, the declaration, and the stream at once, for a host
    /// that hands them to code of its own (a plugin, phase 5 item 6). The
    /// world's columns are reached through queries bound to the access set.
    pub fn split(&mut self) -> (&World, &Access, Option<&mut Pcg32>) {
        (self.world, self.access, self.rng.as_deref_mut())
    }

    /// A query bound to the declared access set.
    pub fn query(&self) -> Query<'_> {
        self.world.query().access_ref(self.access)
    }

    /// The declared RNG stream.
    pub fn rng(&mut self) -> Result<&mut Pcg32> {
        match self.rng.as_deref_mut() {
            Some(r) => Ok(r),
            None => Err(ArgusError::new(
                crate::codes::NO_RNG_STREAM,
                format!(
                    "system {} uses randomness without declaring a stream",
                    self.name
                ),
            )
            .hint("Add rng = \"stream-name\" to the system declaration.")),
        }
    }

    fn check_write(&self, c: ComponentId) -> Result<()> {
        if self.access.allows_write(c) {
            return Ok(());
        }
        let name = &self.world.registry().schema(c).name;
        Err(ArgusError::new(
            codes::ACCESS_VIOLATION,
            format!("system {} writes {name} without declaring it", self.name),
        )
        .hint(format!("Add {name:?} to the system's writes list.")))
    }

    fn check_read(&self, c: ComponentId) -> Result<()> {
        if self.access.allows_read(c) {
            return Ok(());
        }
        let name = &self.world.registry().schema(c).name;
        Err(ArgusError::new(
            codes::ACCESS_VIOLATION,
            format!("system {} reads {name} without declaring it", self.name),
        )
        .hint(format!("Add {name:?} to the system's reads list.")))
    }

    fn id_of<T: Component>(&self) -> Result<ComponentId> {
        self.world.component_id_of::<T>().ok_or_else(|| {
            ArgusError::new(
                codes::UNKNOWN_COMPONENT,
                format!("{} is not registered", T::NAME),
            )
            .hint("Register components before the scheduler runs.")
        })
    }

    /// Spawn a runtime entity.
    pub fn spawn(&mut self) -> Entity {
        self.world.spawn()
    }

    /// Spawn an authored entity (rare from a system; scene loading does this).
    pub fn spawn_authored(&mut self, id: &str) -> Result<Entity> {
        self.world.spawn_authored(id)
    }

    /// Spawn straight into the archetype of `comps` at schema defaults;
    /// every component must be in the declared write set.
    pub fn spawn_with(&mut self, comps: &[ComponentId]) -> Result<Entity> {
        for &c in comps {
            self.check_write(c)?;
        }
        self.world.spawn_with(comps)
    }

    /// Spawn straight into the archetype of `comps` for a prefab the system
    /// declares it spawns (a Luau system's `spawns`): the declaration grants
    /// the prefab's components as a whole, so a component the prefab gains
    /// later needs no new declaration. The host checks the grant first.
    pub fn spawn_granted(&mut self, comps: &[ComponentId]) -> Result<Entity> {
        self.world.spawn_with(comps)
    }

    /// Set a field of an entity just made with [`SystemCtx::spawn_granted`],
    /// to the value its prefab gives it.
    pub fn set_field_granted(&mut self, e: Entity, path: &str, value: Value) -> Result<()> {
        self.world.set_field(e, path, value)
    }

    /// A tag (a component with no fields) by name, registered the first time
    /// a spawned prefab names it, as a scene registers the tags its files use.
    pub fn tag(&mut self, name: &str, source: &str) -> Result<ComponentId> {
        if let Some(id) = self.world.component_id(name) {
            return Ok(id);
        }
        let schema = argus_ecs::SchemaBuilder::new(name)
            .doc("Tag declared by use in a prefab.")
            .source(source.to_string())
            .build()?;
        self.world.register_schema(schema)
    }

    /// Live entities within `r` of a point, nearest first, from the
    /// broadphase rebuilt at the start of this tick (§12), or since then by
    /// [`SystemCtx::rebuild_broadphase`].
    pub fn within(&self, x: f32, y: f32, r: f32) -> Vec<(Entity, f32)> {
        self.world.within(x, y, r)
    }

    /// [`within`](SystemCtx::within) in no order, for a caller that orders
    /// the nearest few itself (see `World::within_unordered`).
    pub fn within_unordered(&self, x: f32, y: f32, r: f32) -> Vec<(Entity, f32)> {
        self.world.within_unordered(x, y, r)
    }

    /// Rebuild the shared broadphase from positions as they are now. The
    /// physics step calls this after moving bodies, so its contact pass and
    /// every later system this tick see where things are, not where they were.
    pub fn rebuild_broadphase(&mut self) -> usize {
        self.world.rebuild_broadphase()
    }

    /// Events emitted so far this tick with this name (every event when
    /// `name` is empty), oldest first: how gameplay reacts to a contact or a
    /// landing the physics step reported earlier in the tick.
    pub fn events_now<'b>(
        &'b self,
        name: &'b str,
    ) -> impl Iterator<Item = &'b argus_ecs::EventRecord> + 'b {
        let frame = self.world.frame();
        // This frame's events are the last ones: the queue is appended in
        // frame order, and a long step keeps every event since it began
        // until the daemon logs them. Scanning all of it, each call grew
        // with the step (found at the phase 3 gate: Outpost's inspector
        // asks a dozen times a tick).
        let all = self.world.events();
        let start = all.len() - all.iter().rev().take_while(|r| r.frame >= frame).count();
        all[start..].iter().filter(move |r| {
            r.frame == frame
                && (name.is_empty()
                    || matches!(&r.event, argus_ecs::Event::Game { name: n, .. } if n == name))
        })
    }

    /// The nearest live entity to a point that `keep` accepts, within `max_r`.
    pub fn nearest(
        &self,
        x: f32,
        y: f32,
        max_r: f32,
        mut keep: impl FnMut(Entity) -> bool,
    ) -> Option<(Entity, f32)> {
        let w: &World = self.world;
        w.broadphase()
            .nearest(x, y, max_r, |e| w.is_alive(e) && keep(e))
    }

    /// Despawn an entity.
    pub fn despawn(&mut self, e: Entity) -> Result<()> {
        self.world.despawn(e)
    }

    /// Insert a component; requires a declared write.
    pub fn insert<T: Component>(&mut self, e: Entity, value: T) -> Result<()> {
        let c = self.id_of::<T>()?;
        self.check_write(c)?;
        self.world.insert(e, value)
    }

    /// Insert defaults by id; requires a declared write.
    pub fn insert_default(&mut self, e: Entity, c: ComponentId) -> Result<()> {
        self.check_write(c)?;
        self.world.insert_default(e, c)
    }

    /// Remove a component; requires a declared write.
    pub fn remove(&mut self, e: Entity, c: ComponentId) -> Result<()> {
        self.check_write(c)?;
        self.world.remove(e, c)
    }

    /// Copy out a component; requires a declared read.
    pub fn get<T: Component>(&self, e: Entity) -> Result<T> {
        let c = self.id_of::<T>()?;
        self.check_read(c)?;
        self.world.get::<T>(e)
    }

    /// Mutate a component in place; requires a declared write.
    pub fn update<T: Component, R>(&self, e: Entity, f: impl FnOnce(&mut T) -> R) -> Result<R> {
        let c = self.id_of::<T>()?;
        self.check_write(c)?;
        self.world.update::<T, R>(e, f)
    }

    /// Read a field by path; requires a declared read.
    pub fn get_field(&self, e: Entity, path: &str) -> Result<Value> {
        let (c, _) = self.world.registry().resolve_path(path)?;
        self.check_read(c)?;
        self.world.get_field(e, path)
    }

    /// Write a field by path; requires a declared write.
    pub fn set_field(&self, e: Entity, path: &str, value: impl Into<Value>) -> Result<()> {
        let (c, _) = self.world.registry().resolve_path(path)?;
        self.check_write(c)?;
        self.world.set_field(e, path, value)
    }

    /// True if the entity has the component (no declaration needed).
    pub fn has(&self, e: Entity, c: ComponentId) -> bool {
        self.world.has(e, c)
    }

    /// Resolve `@id` / `e:n`.
    pub fn entity(&self, r: &str) -> Result<Entity> {
        self.world.entity(r)
    }

    /// Liveness.
    pub fn is_alive(&self, e: Entity) -> bool {
        self.world.is_alive(e)
    }

    /// Emit a gameplay event (§8 L2).
    pub fn emit(&mut self, name: impl Into<String>, fields: Vec<(String, Value)>) {
        self.world.emit(Event::Game {
            name: name.into(),
            fields,
        });
    }
}
