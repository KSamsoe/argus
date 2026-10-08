//! The world: one registry, one entity table, one set of archetypes, one
//! string table, one event queue (§2 "The daemon holds exactly one world").
//! A daemon holds several `World`s only as forks of a snapshot (§6).

use crate::archetype::{Archetype, ArchetypeId, ArchetypeStat};
use crate::entity::{Entities, Entity};
use crate::error::{codes, ArgusError, Result};
use crate::event::{Event, EventRecord};
use crate::hierarchy::{GlobalTransform, Transform};
use crate::intern::Interner;
use crate::query::Query;
use crate::schema::{Component, ComponentId, ComponentSchema, FieldSchema, Registry};
use crate::spatial::SpatialHash;
use crate::value::{FieldType, Value};
use std::collections::HashMap;

/// The ECS world.
#[derive(Clone, Debug)]
pub struct World {
    pub(crate) registry: Registry,
    pub(crate) entities: Entities,
    pub(crate) archetypes: Vec<Archetype>,
    pub(crate) archetype_map: HashMap<Vec<ComponentId>, ArchetypeId>,
    /// Authored string id -> entity.
    pub(crate) names: HashMap<String, Entity>,
    /// Entity index -> authored string id.
    pub(crate) ids: HashMap<u32, String>,
    /// A room-scoped id's last part (`player` of `a/player`) -> the entities
    /// it could mean, so `@player` finds `@a/player` when there is only one.
    pub(crate) short: HashMap<String, Vec<Entity>>,
    pub(crate) strings: Interner,
    pub(crate) events: Vec<EventRecord>,
    pub(crate) frame: u64,
    /// The system running now, which every event it emits is tagged with.
    pub(crate) emitter: Option<std::sync::Arc<str>>,
    /// The shared broadphase over `Transform.pos` (or `GlobalTransform`
    /// when present), rebuilt by [`World::rebuild_broadphase`].
    broadphase: SpatialHash,
    /// See [`World::structure`].
    pub(crate) structure: u64,
}

/// The next [`World::structure`] value: never repeats in a process.
static STRUCTURE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub(crate) fn next_structure() -> u64 {
    STRUCTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl Default for World {
    fn default() -> Self {
        World::new()
    }
}

impl World {
    /// Empty world with no components registered.
    pub fn new() -> World {
        let registry = Registry::default();
        let empty = Archetype::new(ArchetypeId(0), Vec::new(), &registry);
        let mut archetype_map = HashMap::new();
        archetype_map.insert(Vec::new(), ArchetypeId(0));
        World {
            registry,
            entities: Entities::default(),
            archetypes: vec![empty],
            archetype_map,
            names: HashMap::new(),
            ids: HashMap::new(),
            short: HashMap::new(),
            strings: Interner::new(),
            events: Vec::new(),
            frame: 0,
            emitter: None,
            broadphase: SpatialHash::default(),
            structure: next_structure(),
        }
    }

    // ----- registry ---------------------------------------------------------

    /// Register a Rust component type. Idempotent.
    pub fn register<T: Component>(&mut self) -> Result<ComponentId> {
        let before = self.registry.len();
        let id = self.registry.register_type::<T>()?;
        if self.registry.len() > before {
            self.emit(Event::ComponentRegistered {
                component: id,
                name: T::NAME.to_string(),
            });
        }
        Ok(id)
    }

    /// Register a runtime-built schema (the Luau path). Idempotent for an
    /// identical schema; a conflicting schema under the same name is an error.
    pub fn register_schema(&mut self, schema: ComponentSchema) -> Result<ComponentId> {
        let before = self.registry.len();
        let name = schema.name.clone();
        let id = self.registry.register(schema)?;
        if self.registry.len() > before {
            self.emit(Event::ComponentRegistered {
                component: id,
                name,
            });
        }
        Ok(id)
    }

    /// The registry.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Component id by name.
    pub fn component_id(&self, name: &str) -> Option<ComponentId> {
        self.registry.id(name)
    }

    /// Component id for a Rust type, if registered.
    pub fn component_id_of<T: Component>(&self) -> Option<ComponentId> {
        self.registry.id_of::<T>()
    }

    /// The schema for a component, by name (`world.describe`, §5).
    pub fn describe(&self, name: &str) -> Result<&ComponentSchema> {
        Ok(self.registry.schema(self.registry.require(name)?))
    }

    /// The string table.
    pub fn strings(&self) -> &Interner {
        &self.strings
    }

    /// Frame counter used to tag events. Advanced by [`World::end_frame`].
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// Changes whenever an entity is spawned or despawned or gains or loses
    /// a component, and when the world is restored, forked, or read back,
    /// and never repeats in a process. With [`World::frame`], what a cache
    /// of data derived from the world (a navigation grid, phase 3 item 2)
    /// checks before trusting itself: a field changed within a frame does
    /// not move it.
    pub fn structure(&self) -> u64 {
        self.structure
    }

    // ----- entities ---------------------------------------------------------

    /// Spawn a runtime entity: no string id, never written to a scene file (§3).
    pub fn spawn(&mut self) -> Entity {
        let e = self.entities.alloc(false);
        self.place_in_empty(e);
        self.emit(Event::Spawned {
            entity: e,
            id: None,
        });
        e
    }

    /// Spawn a runtime entity straight into the archetype of `comps`, every
    /// component at its schema defaults: one archetype placement instead
    /// of one move per component (§19 batch spawn). Duplicates are fine.
    pub fn spawn_with(&mut self, comps: &[ComponentId]) -> Result<Entity> {
        let ids = self.component_set(comps)?;
        let e = self.entities.alloc(false);
        self.place_with(e, &ids);
        self.emit(Event::Spawned {
            entity: e,
            id: None,
        });
        self.emit_added(e, &ids);
        Ok(e)
    }

    /// Spawn an authored entity straight into the archetype of `comps`, as
    /// [`World::spawn_with`] does for a runtime one: what a room load does
    /// for each of its entities (phase 3 item 6, stream-in under 4 ms).
    pub fn spawn_authored_with(&mut self, id: &str, comps: &[ComponentId]) -> Result<Entity> {
        let id = self.check_new_id(id)?.to_string();
        let ids = self.component_set(comps)?;
        let e = self.entities.alloc(true);
        self.place_with(e, &ids);
        self.name(e, &id);
        self.emit(Event::Spawned {
            entity: e,
            id: Some(id),
        });
        self.emit_added(e, &ids);
        Ok(e)
    }

    /// `comps` sorted and deduplicated, each registered.
    fn component_set(&self, comps: &[ComponentId]) -> Result<Vec<ComponentId>> {
        let mut ids: Vec<ComponentId> = comps.to_vec();
        ids.sort_unstable();
        ids.dedup();
        for &c in &ids {
            if c.0 as usize >= self.registry.schemas().len() {
                return Err(ArgusError::new(
                    codes::UNKNOWN_COMPONENT,
                    format!("component id {} is not registered", c.0),
                ));
            }
        }
        Ok(ids)
    }

    /// Put a new entity in the archetype of `ids` (sorted), every component
    /// at its schema defaults.
    fn place_with(&mut self, e: Entity, ids: &[ComponentId]) {
        let rows: Vec<Vec<u8>> = ids
            .iter()
            .map(|&c| self.registry.schema(c).default_row(&self.strings))
            .collect();
        self.structure = next_structure();
        let dst = self.archetype_for(ids.to_vec());
        let ptrs: Vec<*const u8> = rows.iter().map(|r| r.as_ptr()).collect();
        // SAFETY: each row is exactly its schema's size, in `ids` order,
        // which is the archetype's component order.
        let row = unsafe { self.archetypes[dst.0 as usize].push_row(e, &ptrs) };
        let slot = self.entities.slot_mut(e).expect("just allocated");
        slot.archetype = dst.0;
        slot.row = row as u32;
    }

    fn emit_added(&mut self, e: Entity, ids: &[ComponentId]) {
        for &c in ids {
            self.emit(Event::ComponentAdded {
                entity: e,
                component: c,
            });
        }
    }

    /// A string id a new authored entity may take.
    fn check_new_id<'i>(&self, id: &'i str) -> Result<&'i str> {
        let id = id.strip_prefix('@').unwrap_or(id);
        if id.is_empty()
            || id.contains(char::is_whitespace)
            || id.starts_with('@')
            || id.starts_with("e:")
        {
            return Err(ArgusError::new(codes::BAD_ID, format!("{id:?} is not a valid string id"))
                .hint("Ids look like player or door.east: no whitespace, no leading @, not an e: ref."));
        }
        if let Some(&existing) = self.names.get(id) {
            return Err(ArgusError::new(
                codes::DUPLICATE_ID,
                format!("string id {id:?} is already used by {existing}"),
            )
            .hint("Authored ids are unique within a scene; pick another id or despawn the existing entity."));
        }
        Ok(id)
    }

    /// Give an entity its string id.
    fn name(&mut self, e: Entity, id: &str) {
        self.names.insert(id.to_string(), e);
        self.ids.insert(e.index(), id.to_string());
        if let Some((_, last)) = id.rsplit_once('/') {
            self.short.entry(last.to_string()).or_default().push(e);
        }
    }

    /// Spawn an authored entity with a stable string id such as `player` or
    /// `door.east`. Ids are unique within a world.
    pub fn spawn_authored(&mut self, id: &str) -> Result<Entity> {
        let id = self.check_new_id(id)?.to_string();
        let e = self.entities.alloc(true);
        self.place_in_empty(e);
        self.name(e, &id);
        let id = id.as_str();
        self.emit(Event::Spawned {
            entity: e,
            id: Some(id.to_string()),
        });
        Ok(e)
    }

    fn place_in_empty(&mut self, e: Entity) {
        self.structure = next_structure();
        // SAFETY: the empty archetype has no columns, so no row pointers are read.
        let row = unsafe { self.archetypes[0].push_row(e, &[]) };
        let slot = self.entities.slot_mut(e).expect("just allocated");
        slot.archetype = 0;
        slot.row = row as u32;
    }

    /// Resolve an entity reference in any protocol form: `@player`, `player`,
    /// or `e:4011`. In a room scene an id is `room/id`; a bare `@player`
    /// means the one `…/player` there is, as it does in the room's own files,
    /// and is an error naming them all when there are several.
    pub fn entity(&self, r: &str) -> Result<Entity> {
        if r.starts_with("e:") {
            let e: Entity = r
                .parse()
                .map_err(|m: String| ArgusError::new(codes::BAD_ID, m))?;
            return if self.is_alive(e) {
                Ok(e)
            } else {
                Err(ArgusError::new(
                    codes::ENTITY_NOT_ALIVE,
                    format!("{e} is not alive"),
                ))
            };
        }
        let id = r.strip_prefix('@').unwrap_or(r);
        if let Some(&e) = self.names.get(id) {
            return Ok(e);
        }
        match self.short.get(id).map(Vec::as_slice) {
            Some([e]) => Ok(*e),
            Some(many) if !id.contains('/') => {
                let mut ids: Vec<String> = many
                    .iter()
                    .filter_map(|e| self.string_id(*e))
                    .map(|s| format!("@{s}"))
                    .collect();
                ids.sort();
                Err(ArgusError::new(
                    codes::UNKNOWN_ID,
                    format!("@{id} could be any of {}", ids.join(", ")),
                )
                .hint("In a room scene an id is room/id; name the room."))
            }
            _ => Err(ArgusError::new(
                codes::UNKNOWN_ID,
                format!("no entity with string id {id:?}"),
            )
            .hint("world.summary lists named entities; runtime entities have no string id.")),
        }
    }

    /// The authored string id of an entity, if it has one.
    pub fn string_id(&self, e: Entity) -> Option<&str> {
        if !self.is_alive(e) {
            return None;
        }
        self.ids.get(&e.index()).map(String::as_str)
    }

    /// True if the entity was spawned with a string id.
    pub fn is_authored(&self, e: Entity) -> bool {
        self.entities.slot(e).map(|s| s.authored).unwrap_or(false)
    }

    /// True if the handle refers to a live entity.
    pub fn is_alive(&self, e: Entity) -> bool {
        self.entities.is_alive(e)
    }

    /// Destroy an entity. Components holding references to it are nulled at
    /// the next [`World::maintain`], with an event per reference.
    pub fn despawn(&mut self, e: Entity) -> Result<()> {
        let slot = *self.require_slot(e)?;
        self.structure = next_structure();
        if let Some(moved) = self.archetypes[slot.archetype as usize].swap_remove(slot.row as usize)
        {
            self.entities
                .slot_mut(moved)
                .expect("moved entity alive")
                .row = slot.row;
        }
        self.entities.free(e);
        let id = self.ids.remove(&e.index());
        if let Some(id) = &id {
            self.names.remove(id);
            if let Some((_, last)) = id.rsplit_once('/') {
                if let Some(v) = self.short.get_mut(last) {
                    v.retain(|x| *x != e);
                    if v.is_empty() {
                        self.short.remove(last);
                    }
                }
            }
        }
        self.emit(Event::Despawned { entity: e, id });
        Ok(())
    }

    fn require_slot(&self, e: Entity) -> Result<&crate::entity::Slot> {
        self.entities.slot(e).ok_or_else(|| {
            ArgusError::new(codes::ENTITY_NOT_ALIVE, format!("{e} is not alive"))
                .hint("The entity was despawned or never existed; re-query for a live handle.")
        })
    }

    /// Live entity count.
    pub fn entity_count(&self) -> usize {
        self.entities.alive_count()
    }

    /// Number of authored (string-id) entities.
    pub fn authored_count(&self) -> usize {
        self.names.len()
    }

    /// All authored ids and their entities, sorted by id.
    pub fn named_entities(&self) -> Vec<(&str, Entity)> {
        let mut v: Vec<_> = self.names.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        v
    }

    /// Every named entity, in no order, without collecting them.
    pub fn named(&self) -> impl Iterator<Item = (&str, Entity)> + '_ {
        self.names.iter().map(|(k, v)| (k.as_str(), *v))
    }

    // ----- broadphase -------------------------------------------------------

    /// Bring the spatial hash up to date with every entity with a position:
    /// `GlobalTransform.pos` when the entity has one, else `Transform.pos`.
    /// The scheduler calls this at the end of every tick; a client that
    /// edits positions outside a tick calls it before a spatial query. Only
    /// the entities that changed cell move in the hash, and the ones gone
    /// (or without a position now) leave it (§12 "rebuilt incrementally").
    /// Returns the number of entities indexed.
    pub fn rebuild_broadphase(&mut self) -> usize {
        let frame = self.frame;
        let t = self.registry.id_of::<Transform>();
        let g = self.registry.id_of::<GlobalTransform>();
        let Some(t) = t else {
            self.broadphase.clear(frame);
            return 0;
        };
        let t_off = self
            .registry
            .schema(t)
            .field("pos")
            .map(|f| f.offset)
            .unwrap_or(0);
        let g_off = g
            .and_then(|g| self.registry.schema(g).field("pos").map(|f| f.offset))
            .unwrap_or(0);
        let mut hash = std::mem::take(&mut self.broadphase);
        hash.begin(frame);
        if hash.is_empty() {
            hash.reserve(self.entities.alive);
        }
        for arch in &self.archetypes {
            if arch.is_empty() {
                continue;
            }
            let (col, off) = match (g.and_then(|g| arch.column(g)), arch.column(t)) {
                (Some(gc), _) => (gc, g_off),
                (None, Some(tc)) => (tc, t_off),
                (None, None) => continue,
            };
            for (row, &e) in arch.entities().iter().enumerate() {
                // SAFETY: `pos` is a Vec2 (two f32) at `off` inside a row of
                // this column, and `row` is within the column's length.
                let (x, y) = unsafe {
                    let p = col.row_ptr(row).add(off) as *const f32;
                    (p.read_unaligned(), p.add(1).read_unaligned())
                };
                hash.place(e, x, y);
            }
        }
        hash.finish();
        self.broadphase = hash;
        self.broadphase.len()
    }

    /// The broadphase as last rebuilt.
    pub fn broadphase(&self) -> &SpatialHash {
        &self.broadphase
    }

    /// Set the broadphase cell size (world units); takes effect on the next rebuild.
    pub fn set_broadphase_cell(&mut self, cell: f32) {
        self.broadphase = SpatialHash::new(cell);
    }

    /// Live entities within `r` of a point, nearest first, from the
    /// broadphase as last rebuilt.
    pub fn within(&self, x: f32, y: f32, r: f32) -> Vec<(Entity, f32)> {
        let mut v = self.broadphase.within(x, y, r);
        v.retain(|(e, _)| self.is_alive(*e));
        v
    }

    // ----- components -------------------------------------------------------

    pub(crate) fn archetype_for(&mut self, comps: Vec<ComponentId>) -> ArchetypeId {
        if let Some(&id) = self.archetype_map.get(&comps) {
            return id;
        }
        let id = ArchetypeId(self.archetypes.len() as u32);
        self.archetypes
            .push(Archetype::new(id, comps.clone(), &self.registry));
        self.archetype_map.insert(comps, id);
        id
    }

    /// Move `e` from its archetype to `dst`. `extra` supplies the bytes for a
    /// component that `dst` has and the source lacks.
    ///
    /// # Safety
    /// `extra.1` must be valid for reads of that component's size.
    unsafe fn move_entity(
        &mut self,
        e: Entity,
        dst: ArchetypeId,
        extra: Option<(ComponentId, *const u8)>,
    ) {
        let slot = *self.entities.slot(e).expect("alive");
        let src_id = slot.archetype as usize;
        let row = slot.row as usize;
        debug_assert_ne!(src_id, dst.0 as usize);
        self.structure = next_structure();

        let ptrs: Vec<*const u8> = {
            let src = &self.archetypes[src_id];
            self.archetypes[dst.0 as usize]
                .components
                .iter()
                .map(|&c| match extra {
                    Some((ec, p)) if ec == c => p,
                    _ => src.column(c).expect("source has component").row_ptr(row),
                })
                .collect()
        };
        // Pushing into dst cannot invalidate src's memory (different archetype).
        let new_row = self.archetypes[dst.0 as usize].push_row(e, &ptrs);
        if let Some(moved) = self.archetypes[src_id].swap_remove(row) {
            self.entities.slot_mut(moved).expect("moved alive").row = row as u32;
        }
        let s = self.entities.slot_mut(e).expect("alive");
        s.archetype = dst.0;
        s.row = new_row as u32;
    }

    /// Insert or overwrite a component from raw row bytes.
    ///
    /// # Safety
    /// `bytes` must be valid for reads of the component's schema size.
    unsafe fn insert_raw(&mut self, e: Entity, c: ComponentId, bytes: *const u8) -> Result<()> {
        let slot = *self.require_slot(e)?;
        let arch = &self.archetypes[slot.archetype as usize];
        if let Some(col) = arch.column(c) {
            let size = col.stride();
            core::ptr::copy_nonoverlapping(bytes, col.row_ptr_mut(slot.row as usize), size);
            return Ok(());
        }
        let mut comps = arch.components.clone();
        let pos = comps.binary_search(&c).unwrap_err();
        comps.insert(pos, c);
        let dst = self.archetype_for(comps);
        self.move_entity(e, dst, Some((c, bytes)));
        self.emit(Event::ComponentAdded {
            entity: e,
            component: c,
        });
        Ok(())
    }

    /// Insert (or overwrite) a Rust component. Registers the type if needed.
    pub fn insert<T: Component>(&mut self, e: Entity, value: T) -> Result<()> {
        let c = self.register::<T>()?;
        // SAFETY: T is the registered type; its size equals the column stride.
        unsafe { self.insert_raw(e, c, &value as *const T as *const u8) }
    }

    /// Insert a component with its schema defaults, by id. Overwrites an
    /// existing instance with defaults.
    pub fn insert_default(&mut self, e: Entity, c: ComponentId) -> Result<()> {
        let row = self.registry.schema(c).default_row(&self.strings);
        // SAFETY: default_row is exactly schema.size bytes.
        unsafe { self.insert_raw(e, c, row.as_ptr()) }
    }

    /// Insert a component by name with its defaults.
    pub fn insert_by_name(&mut self, e: Entity, name: &str) -> Result<ComponentId> {
        let c = self.registry.require(name)?;
        self.insert_default(e, c)?;
        Ok(c)
    }

    /// Remove a component by id. Not having it is not an error.
    pub fn remove(&mut self, e: Entity, c: ComponentId) -> Result<()> {
        let slot = *self.require_slot(e)?;
        let arch = &self.archetypes[slot.archetype as usize];
        if !arch.has(c) {
            return Ok(());
        }
        let comps: Vec<ComponentId> = arch
            .components
            .iter()
            .copied()
            .filter(|&x| x != c)
            .collect();
        let dst = self.archetype_for(comps);
        // SAFETY: no extra bytes are read.
        unsafe { self.move_entity(e, dst, None) };
        self.emit(Event::ComponentRemoved {
            entity: e,
            component: c,
        });
        Ok(())
    }

    /// Remove a Rust component.
    pub fn remove_type<T: Component>(&mut self, e: Entity) -> Result<()> {
        match self.registry.id_of::<T>() {
            Some(c) => self.remove(e, c),
            None => self.require_slot(e).map(|_| ()),
        }
    }

    /// True if the live entity has the component.
    pub fn has(&self, e: Entity, c: ComponentId) -> bool {
        self.entities
            .slot(e)
            .map(|s| self.archetypes[s.archetype as usize].has(c))
            .unwrap_or(false)
    }

    /// True if the live entity has the Rust component.
    pub fn has_type<T: Component>(&self, e: Entity) -> bool {
        self.registry
            .id_of::<T>()
            .map(|c| self.has(e, c))
            .unwrap_or(false)
    }

    /// Components on an entity, sorted by id.
    pub fn components_of(&self, e: Entity) -> Result<&[ComponentId]> {
        let slot = self.require_slot(e)?;
        Ok(&self.archetypes[slot.archetype as usize].components)
    }

    fn locate(&self, e: Entity, c: ComponentId) -> Result<(&crate::column::Column, usize)> {
        let slot = self.require_slot(e)?;
        let arch = &self.archetypes[slot.archetype as usize];
        let col = arch.column(c).ok_or_else(|| {
            ArgusError::new(
                codes::MISSING_COMPONENT,
                format!("{e} has no {} component", self.registry.schema(c).name),
            )
            .hint("world.get with resolved: true lists the entity's components.")
        })?;
        Ok((col, slot.row as usize))
    }

    /// Copy out a Rust component.
    pub fn get<T: Component>(&self, e: Entity) -> Result<T> {
        let c = self.registry.id_of::<T>().ok_or_else(|| {
            ArgusError::new(
                codes::UNKNOWN_COMPONENT,
                format!("{} is not registered", T::NAME),
            )
        })?;
        let (col, row) = self.locate(e, c)?;
        col.acquire_shared()?;
        // SAFETY: T's size matches the stride; the row is initialized.
        let v = unsafe { core::ptr::read(col.row_ptr(row) as *const T) };
        col.release_shared();
        Ok(v)
    }

    /// Mutate a Rust component in place.
    pub fn update<T: Component, R>(&self, e: Entity, f: impl FnOnce(&mut T) -> R) -> Result<R> {
        let c = self.registry.id_of::<T>().ok_or_else(|| {
            ArgusError::new(
                codes::UNKNOWN_COMPONENT,
                format!("{} is not registered", T::NAME),
            )
        })?;
        let (col, row) = self.locate(e, c)?;
        col.acquire_exclusive()?;
        // SAFETY: exclusive borrow held; size matches.
        let r = unsafe { f(&mut *(col.row_ptr_mut(row) as *mut T)) };
        col.release_exclusive();
        Ok(r)
    }

    /// Read one field by `Component.field` path (§5).
    pub fn get_field(&self, e: Entity, path: &str) -> Result<Value> {
        let (c, field) = self.registry.resolve_path(path)?;
        self.get_field_by(e, c, field)
    }

    /// Write one field by path, with type coercion and range checking. The
    /// error for a bad value is the §5 shape: code 4102, sentence, hint, ref.
    pub fn set_field(&self, e: Entity, path: &str, value: impl Into<Value>) -> Result<()> {
        let (c, field) = self.registry.resolve_path(path)?;
        self.set_field_by(e, c, field, value.into())
    }

    /// Read a field with the component id and field schema already resolved
    /// (the hot path for script bindings, which resolve names once).
    pub fn get_field_by(&self, e: Entity, c: ComponentId, field: &FieldSchema) -> Result<Value> {
        let (col, row) = self.locate(e, c)?;
        col.acquire_shared()?;
        // SAFETY: offset + size within the row.
        let v = unsafe {
            field
                .ty
                .read(col.row_ptr(row).add(field.offset), &self.strings)
        };
        col.release_shared();
        Ok(v)
    }

    /// Write a field with the component id and field schema already resolved.
    pub fn set_field_by(
        &self,
        e: Entity,
        c: ComponentId,
        field: &FieldSchema,
        value: Value,
    ) -> Result<()> {
        let schema = self.registry.schema(c);
        let value = schema.check(field, value)?;
        let (col, row) = self.locate(e, c)?;
        col.acquire_exclusive()?;
        // SAFETY: offset + size within the row.
        let r = unsafe {
            field.ty.write(
                col.row_ptr_mut(row).add(field.offset),
                &value,
                &self.strings,
            )
        };
        col.release_exclusive();
        r
    }

    /// Every field of a component on an entity, as `(name, value)` pairs.
    pub fn get_component(&self, e: Entity, c: ComponentId) -> Result<Vec<(String, Value)>> {
        let schema = self.registry.schema(c);
        let (col, row) = self.locate(e, c)?;
        col.acquire_shared()?;
        let out = schema
            .fields
            .iter()
            .map(|f| {
                // SAFETY: offset + size within the row.
                let v = unsafe { f.ty.read(col.row_ptr(row).add(f.offset), &self.strings) };
                (f.name.clone(), v)
            })
            .collect();
        col.release_shared();
        Ok(out)
    }

    // ----- queries ----------------------------------------------------------

    /// Start a query.
    pub fn query(&self) -> Query<'_> {
        Query::new(self)
    }

    /// All archetypes, including empty ones.
    pub fn archetypes(&self) -> &[Archetype] {
        &self.archetypes
    }

    /// Entity counts by archetype, non-empty only, largest first.
    pub fn archetype_stats(&self) -> Vec<ArchetypeStat> {
        let mut v: Vec<ArchetypeStat> = self
            .archetypes
            .iter()
            .filter(|a| !a.is_empty())
            .map(|a| ArchetypeStat {
                id: a.id,
                components: a
                    .components
                    .iter()
                    .map(|&c| self.registry.schema(c).name.clone())
                    .collect(),
                entities: a.len(),
                bytes: a.bytes(),
            })
            .collect();
        v.sort_by(|a, b| b.entities.cmp(&a.entities).then(a.id.0.cmp(&b.id.0)));
        v
    }

    // ----- events and maintenance -------------------------------------------

    /// Append an event tagged with the current frame.
    pub fn emit(&mut self, event: Event) {
        self.events.push(EventRecord {
            frame: self.frame,
            event,
            by: self.emitter.clone(),
        });
    }

    /// The system running now (the scheduler sets it around each system),
    /// so the events it emits say who emitted them (§8 `obs.why`).
    pub fn set_emitter(&mut self, system: Option<std::sync::Arc<str>>) {
        self.emitter = system;
    }

    /// Every live entity, archetype by archetype.
    pub fn live(&self) -> impl Iterator<Item = Entity> + '_ {
        self.archetypes
            .iter()
            .flat_map(|a| a.entities().iter().copied())
    }

    /// Look at one component's row as bytes, fields at their schema
    /// offsets (a tag's row is empty): how history and diffs compare and
    /// keep rows without building values.
    pub fn with_row<R>(&self, e: Entity, c: ComponentId, f: impl FnOnce(&[u8]) -> R) -> Result<R> {
        let size = self.registry.schema(c).size;
        let (col, row) = self.locate(e, c)?;
        if size == 0 {
            return Ok(f(&[]));
        }
        col.acquire_shared()?;
        // SAFETY: the row is `size` initialised bytes inside the column,
        // and the shared borrow is held while `f` looks at them.
        let r = f(unsafe { std::slice::from_raw_parts(col.row_ptr(row), size) });
        col.release_shared();
        Ok(r)
    }

    /// One field's value out of a row's bytes (as [`World::with_row`] gave
    /// them, now or kept from before); `None` if the row is too short.
    pub fn decode_field(&self, field: &FieldSchema, row: &[u8]) -> Option<Value> {
        if field.offset + field.ty.size() > row.len() {
            return None;
        }
        // SAFETY: the field's bytes are inside `row`; reads are unaligned.
        Some(unsafe { field.ty.read(row.as_ptr().add(field.offset), &self.strings) })
    }

    /// Pending events.
    pub fn events(&self) -> &[EventRecord] {
        &self.events
    }

    /// Take all pending events.
    pub fn drain_events(&mut self) -> Vec<EventRecord> {
        std::mem::take(&mut self.events)
    }

    /// End-of-frame maintenance (§3 "Relations"): every entity-reference
    /// field that points at a dead entity is set to `Entity::NULL` and a
    /// `DanglingRefNulled` event is emitted. Returns the number nulled.
    pub fn maintain(&mut self) -> usize {
        let ref_comps: Vec<(ComponentId, Vec<(usize, String)>)> = self
            .registry
            .schemas()
            .iter()
            .enumerate()
            .filter(|(_, s)| s.has_entity_refs())
            .map(|(i, s)| {
                (
                    ComponentId(i as u32),
                    s.fields
                        .iter()
                        .filter(|f| f.ty == FieldType::Entity)
                        .map(|f| (f.offset, f.name.clone()))
                        .collect(),
                )
            })
            .collect();
        if ref_comps.is_empty() {
            return 0;
        }
        let mut nulled = Vec::new();
        for arch in &self.archetypes {
            for (c, fields) in &ref_comps {
                let Some(col) = arch.column(*c) else { continue };
                for row in 0..col.len() {
                    for (offset, name) in fields {
                        // SAFETY: field lies within the row; Entity is 8 bytes, read unaligned.
                        let p = unsafe { col.row_ptr_mut(row).add(*offset) };
                        let target: Entity =
                            unsafe { core::ptr::read_unaligned(p as *const Entity) };
                        if !target.is_null() && !self.entities.is_alive(target) {
                            unsafe { core::ptr::write_unaligned(p as *mut Entity, Entity::NULL) };
                            nulled.push(Event::DanglingRefNulled {
                                entity: arch.entities()[row],
                                component: *c,
                                field: name.clone(),
                                target,
                            });
                        }
                    }
                }
            }
        }
        let n = nulled.len();
        for ev in nulled {
            self.emit(ev);
        }
        n
    }

    /// Run maintenance and advance the frame counter.
    pub fn end_frame(&mut self) -> usize {
        let n = self.maintain();
        self.frame += 1;
        n
    }

    /// One-screen orientation (§8 L0).
    pub fn summary(&self) -> WorldSummary {
        WorldSummary {
            frame: self.frame,
            entities: self.entity_count(),
            authored: self.authored_count(),
            components: self.registry.len(),
            archetypes: self.archetype_stats(),
            named: self
                .named_entities()
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
            pending_events: self.events.len(),
        }
    }

    /// Approximate bytes held by columns, entity table, and strings.
    pub fn bytes(&self) -> usize {
        self.archetypes.iter().map(Archetype::bytes).sum::<usize>()
            + self.strings.bytes()
            // The broadphase: an entry per placed entity and one in a cell.
            + self.broadphase.len() * 20
            + self.events.len() * std::mem::size_of::<EventRecord>()
    }

    /// A copy of the world's state with its event queue (§6): this frame's
    /// events, the ones a system can still read. The older ones are only
    /// waiting for the log, and are the log's: a snapshot taken deep into
    /// a long step carried every event since the step began, and a keyframe
    /// ring of them grew past its budget unseen (eight hours of Outpost in
    /// one `sim.step` held 26 GB; found at the phase 3 gate).
    pub(crate) fn clone_state(&self) -> World {
        World {
            registry: self.registry.clone(),
            entities: self.entities.clone(),
            archetypes: self.archetypes.clone(),
            archetype_map: self.archetype_map.clone(),
            names: self.names.clone(),
            ids: self.ids.clone(),
            short: self.short.clone(),
            strings: self.strings.clone(),
            events: self
                .events
                .iter()
                .filter(|r| r.frame >= self.frame)
                .cloned()
                .collect(),
            frame: self.frame,
            emitter: self.emitter.clone(),
            broadphase: self.broadphase.clone(),
            structure: self.structure,
        }
    }
}

/// What `world.summary` returns: enough to orient in ~300 bytes of JSON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldSummary {
    /// Frame counter.
    pub frame: u64,
    /// Live entities.
    pub entities: usize,
    /// Authored (string-id) entities.
    pub authored: usize,
    /// Registered components.
    pub components: usize,
    /// Non-empty archetypes, largest first.
    pub archetypes: Vec<ArchetypeStat>,
    /// Named entities, sorted by id.
    pub named: Vec<(String, Entity)>,
    /// Events not yet drained.
    pub pending_events: usize,
}
