//! Parallel systems (§3 "The scheduler uses this for parallelism", phase 5
//! item 5).
//!
//! A parallel system is a `Send` body that sees the world through a
//! [`ParCtx`]: the components it declared, and nothing it could race on.
//! The scheduler runs consecutive parallel systems that do not conflict as
//! one batch, on one thread or several. What a system in a batch reads, no
//! other system of the batch writes, so it reads the world as the batch
//! began plus its own writes. What it changes beyond its columns waits for
//! the end of the batch: its events and its spawns, despawns, inserts, and
//! removes are applied then, system by system in run order. A batch is
//! therefore the same on one thread or many, and the world after it does
//! not depend on which thread ran what (§6).

use crate::clock::ClockKind;
use crate::rng::Pcg32;
use crate::system::Stage;
use argus_ecs::{
    codes, Access, ArgusError, Component, ComponentId, Entity, Event, EventRecord, Interner, Query,
    Registry, Result, Value, World,
};

/// A parallel system body.
pub type ParSystemFn = Box<dyn FnMut(&mut ParCtx<'_>) -> Result<()> + Send>;

/// A change a parallel system asked for, applied after its batch.
#[derive(Clone, Debug)]
pub(crate) enum Command {
    Spawn {
        comps: Vec<ComponentId>,
        fields: Vec<(ComponentId, String, Value)>,
    },
    Despawn(Entity),
    Insert(Entity, ComponentId),
    Remove(Entity, ComponentId),
}

/// What one parallel system left for the end of its batch.
#[derive(Debug, Default)]
pub(crate) struct Deferred {
    pub(crate) events: Vec<(String, Vec<(String, Value)>)>,
    pub(crate) commands: Vec<Command>,
}

impl Deferred {
    /// Apply it to the world, as the system that asked: events first, in
    /// the order emitted, then the commands in the order given. A command
    /// on an entity another system of the batch removed by then does
    /// nothing.
    pub(crate) fn apply(self, world: &mut World, by: std::sync::Arc<str>) {
        world.set_emitter(Some(by));
        for (name, fields) in self.events {
            world.emit(Event::Game { name, fields });
        }
        for c in self.commands {
            match c {
                Command::Spawn { comps, fields } => {
                    if let Ok(e) = world.spawn_with(&comps) {
                        for (c, field, v) in fields {
                            if let Some(f) = world.registry().schema(c).field(&field) {
                                let _ = world.set_field_by(e, c, f, v);
                            }
                        }
                    }
                }
                Command::Despawn(e) => {
                    if world.is_alive(e) {
                        let _ = world.despawn(e);
                    }
                }
                Command::Insert(e, c) => {
                    if world.is_alive(e) && !world.has(e, c) {
                        let _ = world.insert_default(e, c);
                    }
                }
                Command::Remove(e, c) => {
                    if world.is_alive(e) && world.has(e, c) {
                        let _ = world.remove(e, c);
                    }
                }
            }
        }
        world.set_emitter(None);
    }
}

/// The world as one parallel system may see it.
pub struct ParCtx<'a> {
    pub(crate) world: &'a World,
    pub(crate) access: &'a Access,
    pub(crate) spawns: &'a [ComponentId],
    pub(crate) rng: Option<&'a mut Pcg32>,
    pub(crate) out: &'a mut Deferred,
    /// System name.
    pub name: &'a str,
    /// Stage being run.
    pub stage: Stage,
    /// Clock whose tick this is.
    pub clock: ClockKind,
    /// Tick number on that clock, starting at 0.
    pub tick: u64,
    /// Sim time on that clock: tick × timestep. Never wall-clock.
    pub time: f64,
    /// Timestep on that clock.
    pub dt: f64,
    /// The continuous clock's speed as this tick began.
    pub speed: f64,
    /// The tick is frozen (a hit-stop): only `unfrozen` systems run.
    pub frozen: bool,
    /// Events held over for this system from ticks a freeze kept it from
    /// running, oldest first (as [`crate::SystemCtx`] has them).
    pub(crate) held: &'a [EventRecord],
}

impl<'a> ParCtx<'a> {
    /// The declared access set.
    pub fn access(&self) -> &Access {
        self.access
    }

    /// [`crate::SystemCtx::split`]: the world (to be read through queries
    /// bound to the access set), the declaration, and the stream at once.
    pub fn split(&mut self) -> (&World, &Access, Option<&mut Pcg32>) {
        (self.world, self.access, self.rng.as_deref_mut())
    }

    /// The component registry.
    pub fn registry(&self) -> &Registry {
        self.world.registry()
    }

    /// The world's strings: look one up, or resolve an id. A string the
    /// world has never held cannot be added from a parallel system
    /// (`try_intern` says so): its id would depend on which thread asked.
    pub fn strings(&self) -> &Interner {
        self.world.strings()
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

    fn check(&self, c: ComponentId, write: bool) -> Result<()> {
        let ok = if write {
            self.access.allows_write(c)
        } else {
            self.access.allows_read(c)
        };
        if ok {
            return Ok(());
        }
        let name = &self.world.registry().schema(c).name;
        let verb = if write { "writes" } else { "reads" };
        Err(ArgusError::new(
            codes::ACCESS_VIOLATION,
            format!("system {} {verb} {name} without declaring it", self.name),
        )
        .hint(format!(
            "Add {name:?} to the system's {verb} list; a parallel system's declaration is what keeps its batch apart."
        )))
    }

    /// A structural change to `c` needs it declared written or spawned.
    fn check_spawn(&self, c: ComponentId) -> Result<()> {
        if self.spawns.contains(&c) {
            return Ok(());
        }
        self.check(c, true).map_err(|e| {
            let name = &self.world.registry().schema(c).name;
            e.hint(format!(
                "Declare spawns({name}) (or a write of it); a spawn, insert, or remove lands after the batch."
            ))
        })
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

    /// Copy out a component; requires a declared read.
    pub fn get<T: Component>(&self, e: Entity) -> Result<T> {
        let c = self.id_of::<T>()?;
        self.check(c, false)?;
        self.world.get::<T>(e)
    }

    /// Mutate a component in place; requires a declared write.
    pub fn update<T: Component, R>(&self, e: Entity, f: impl FnOnce(&mut T) -> R) -> Result<R> {
        let c = self.id_of::<T>()?;
        self.check(c, true)?;
        self.world.update::<T, R>(e, f)
    }

    /// Read a field by path; requires a declared read.
    pub fn get_field(&self, e: Entity, path: &str) -> Result<Value> {
        let (c, f) = self.world.registry().resolve_path(path)?;
        self.check(c, false)?;
        self.world.get_field_by(e, c, f)
    }

    /// Write a field by path; requires a declared write.
    pub fn set_field(&self, e: Entity, path: &str, value: impl Into<Value>) -> Result<()> {
        let (c, f) = self.world.registry().resolve_path(path)?;
        self.check(c, true)?;
        self.world.set_field_by(e, c, f, value.into())
    }

    /// True if the entity has the component (no declaration needed).
    pub fn has(&self, e: Entity, c: ComponentId) -> bool {
        self.world.has(e, c)
    }

    /// Liveness.
    pub fn is_alive(&self, e: Entity) -> bool {
        self.world.is_alive(e)
    }

    /// Resolve `@id` / `e:n`.
    pub fn entity(&self, r: &str) -> Result<Entity> {
        self.world.entity(r)
    }

    /// Live entities within `r` of a point, nearest first (§12).
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
        let w = self.world;
        w.broadphase()
            .nearest(x, y, max_r, |e| w.is_alive(e) && keep(e))
    }

    /// Events emitted this tick before the batch began, with this name
    /// (every event when `name` is empty), oldest first. Events of the
    /// other systems in the batch are not here yet: they land after it.
    pub fn events_now<'b>(&'b self, name: &'b str) -> impl Iterator<Item = &'b EventRecord> + 'b {
        let frame = self.world.frame();
        let all = self.world.events();
        let start = all.len() - all.iter().rev().take_while(|r| r.frame >= frame).count();
        let named = move |r: &&EventRecord| {
            name.is_empty() || matches!(&r.event, Event::Game { name: n, .. } if n == name)
        };
        self.held.iter().filter(named).chain(
            all[start..]
                .iter()
                .filter(move |r| r.frame == frame)
                .filter(named),
        )
    }

    /// Emit a gameplay event (§8 L2), after the batch.
    pub fn emit(&mut self, name: impl Into<String>, fields: Vec<(String, Value)>) {
        self.out.events.push((name.into(), fields));
    }

    /// Spawn an entity with these components and field values (`path`,
    /// value), after the batch; every component must be declared spawned
    /// (or written). Its strings may be new: they are written after the
    /// batch.
    pub fn spawn(&mut self, comps: &[ComponentId], fields: Vec<(&str, Value)>) -> Result<()> {
        for &c in comps {
            self.check_spawn(c)?;
        }
        let mut resolved = Vec::with_capacity(fields.len());
        for (path, v) in fields {
            let (c, f) = self.world.registry().resolve_path(path)?;
            if !comps.contains(&c) {
                return Err(ArgusError::new(
                    codes::MISSING_COMPONENT,
                    format!("{path} is not a field of the components spawned"),
                )
                .hint("Name the field's component in the spawn's component list."));
            }
            let v = self.world.registry().schema(c).check(f, v)?;
            resolved.push((c, f.name.clone(), v));
        }
        self.out.commands.push(Command::Spawn {
            comps: comps.to_vec(),
            fields: resolved,
        });
        Ok(())
    }

    /// Despawn an entity, after the batch.
    pub fn despawn(&mut self, e: Entity) -> Result<()> {
        if !self.world.is_alive(e) {
            return Err(ArgusError::new(
                codes::ENTITY_NOT_ALIVE,
                format!("{e} is not alive"),
            ));
        }
        self.out.commands.push(Command::Despawn(e));
        Ok(())
    }

    /// Insert a component at its defaults, after the batch; requires it
    /// declared spawned (or written).
    pub fn insert_default(&mut self, e: Entity, c: ComponentId) -> Result<()> {
        self.check_spawn(c)?;
        self.out.commands.push(Command::Insert(e, c));
        Ok(())
    }

    /// Remove a component, after the batch; requires it declared spawned
    /// (or written).
    pub fn remove(&mut self, e: Entity, c: ComponentId) -> Result<()> {
        self.check_spawn(c)?;
        self.out.commands.push(Command::Remove(e, c));
        Ok(())
    }
}
