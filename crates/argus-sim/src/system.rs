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
        }
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
}

impl<'a> SystemCtx<'a> {
    /// Read-only world access (no declared-access check; use `query` for iteration).
    pub fn world(&self) -> &World {
        self.world
    }

    /// The declared access set.
    pub fn access(&self) -> &Access {
        self.access
    }

    /// A query bound to the declared access set.
    pub fn query(&self) -> Query<'_> {
        self.world.query().access(self.access.clone())
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

    /// Live entities within `r` of a point, nearest first, from the
    /// broadphase rebuilt at the start of this tick (§12).
    pub fn within(&self, x: f32, y: f32, r: f32) -> Vec<(Entity, f32)> {
        self.world.within(x, y, r)
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
