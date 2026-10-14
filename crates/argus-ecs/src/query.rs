//! Queries: iterate the archetypes that match a component filter, with
//! typed (`&T` / `&mut T`) or dynamic (field-by-name) column access.
//!
//! A query may carry an [`Access`] declaration — the components a system
//! said it reads and writes (§3 "Each system declares the components it
//! reads and writes"). Fetching anything outside the declaration is a
//! structured error, which is what keeps `world.who_writes` trustworthy (§7).

use crate::archetype::Archetype;
use crate::column::{ColMut, ColRef, Column};
use crate::entity::Entity;
use crate::error::{codes, ArgusError, Result};
use crate::intern::Interner;
use crate::intern::StrId;
use crate::schema::{Component, ComponentId, ComponentSchema, FieldSchema, Registry};
use crate::value::{FieldType, Value, Vec2};
use crate::world::World;

/// Declared read and write sets. Writing implies reading.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Access {
    /// Components read.
    pub reads: Vec<ComponentId>,
    /// Components written.
    pub writes: Vec<ComponentId>,
}

impl Access {
    /// Empty declaration: nothing may be fetched.
    pub fn new() -> Access {
        Access::default()
    }
    /// Declare a read.
    pub fn read(mut self, c: ComponentId) -> Self {
        self.reads.push(c);
        self
    }
    /// Declare a write.
    pub fn write(mut self, c: ComponentId) -> Self {
        self.writes.push(c);
        self
    }
    /// Build from component names, as a Luau system declaration would.
    pub fn from_names(registry: &Registry, reads: &[&str], writes: &[&str]) -> Result<Access> {
        let mut a = Access::new();
        for r in reads {
            a.reads.push(registry.require(r)?);
        }
        for w in writes {
            a.writes.push(registry.require(w)?);
        }
        Ok(a)
    }
    /// True if the component may be read.
    pub fn allows_read(&self, c: ComponentId) -> bool {
        self.reads.contains(&c) || self.writes.contains(&c)
    }
    /// True if the component may be written.
    pub fn allows_write(&self, c: ComponentId) -> bool {
        self.writes.contains(&c)
    }
}

fn access_violation(registry: &Registry, c: ComponentId, write: bool) -> ArgusError {
    let name = &registry.schema(c).name;
    ArgusError::new(
        codes::ACCESS_VIOLATION,
        format!(
            "system {} {name} without declaring it",
            if write { "writes" } else { "reads" }
        ),
    )
    .hint(format!(
        "Add {name:?} to the system's {} list. Declared sets are what world.who_writes reports.",
        if write { "writes" } else { "reads" }
    ))
}

/// A component filter over the world's archetypes.
pub struct Query<'w> {
    world: &'w World,
    has: Vec<ComponentId>,
    not: Vec<ComponentId>,
    /// Borrowed from the system that runs the query (no copy per query), or
    /// owned when built for one call.
    access: Option<std::borrow::Cow<'w, Access>>,
    /// Set when a `with::<T>` names an unregistered type: matches nothing.
    impossible: bool,
    /// Which entities by whether they are switched off ([`crate::active`]):
    /// the active ones unless asked.
    activity: Activity,
}

/// A query's entities by whether they are switched off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Activity {
    /// Only entities in the game (the default).
    Active,
    /// Every entity.
    All,
    /// Only entities switched off, or under one that is.
    Inactive,
}

impl<'w> Query<'w> {
    pub(crate) fn new(world: &'w World) -> Query<'w> {
        Query {
            world,
            has: Vec::new(),
            not: Vec::new(),
            access: None,
            impossible: false,
            activity: Activity::Active,
        }
    }

    /// Inactive entities too ([`crate::active`]): what the engine's own
    /// bookkeeping (saves, the hierarchy) needs.
    pub fn including_inactive(mut self) -> Self {
        self.activity = Activity::All;
        self
    }

    /// Only inactive entities: those switched off, and those under one.
    pub fn only_inactive(mut self) -> Self {
        self.activity = Activity::Inactive;
        self
    }

    /// Require a component by id.
    pub fn has(mut self, c: ComponentId) -> Self {
        if !self.has.contains(&c) {
            self.has.push(c);
        }
        self
    }
    /// Require a Rust component. An unregistered type matches nothing.
    pub fn with<T: Component>(self) -> Self {
        match self.world.registry().id_of::<T>() {
            Some(c) => self.has(c),
            None => Query {
                impossible: true,
                ..self
            },
        }
    }
    /// Exclude a component by id.
    pub fn not(mut self, c: ComponentId) -> Self {
        self.not.push(c);
        self
    }
    /// Exclude a Rust component.
    pub fn without<T: Component>(self) -> Self {
        match self.world.registry().id_of::<T>() {
            Some(c) => self.not(c),
            None => self,
        }
    }
    /// Attach a declared access set; fetches outside it fail.
    pub fn access(mut self, access: Access) -> Self {
        self.access = Some(std::borrow::Cow::Owned(access));
        self
    }
    /// Attach a declared access set by reference (a system's, for the length
    /// of its run), without copying it.
    pub fn access_ref(mut self, access: &'w Access) -> Self {
        self.access = Some(std::borrow::Cow::Borrowed(access));
        self
    }

    /// True if the archetype satisfies the filter.
    pub fn matches(&self, arch: &Archetype) -> bool {
        if self.impossible
            || !self.has.iter().all(|&c| arch.has(c))
            || self.not.iter().any(|&c| arch.has(c))
        {
            return false;
        }
        if self.activity == Activity::All {
            return true;
        }
        let (a, b) = self.world.registry().inactive_tags();
        let off = a.is_some_and(|c| arch.has(c)) || b.is_some_and(|c| arch.has(c));
        off == (self.activity == Activity::Inactive)
    }

    /// Matching archetypes (including empty ones).
    pub fn archetypes(&self) -> impl Iterator<Item = &'w Archetype> + '_ {
        self.world
            .archetypes()
            .iter()
            .filter(move |a| self.matches(a))
    }

    /// Number of matching entities.
    pub fn count(&self) -> usize {
        self.archetypes().map(Archetype::len).sum()
    }

    /// Matching entities, in archetype then row order.
    pub fn entities(&self) -> Vec<Entity> {
        self.archetypes()
            .flat_map(|a| a.entities().iter().copied())
            .collect()
    }

    /// Visit each non-empty matching archetype with column access.
    pub fn for_each_view(&self, mut f: impl FnMut(ArchetypeView<'_>) -> Result<()>) -> Result<()> {
        for arch in self.archetypes() {
            if arch.is_empty() {
                continue;
            }
            f(ArchetypeView {
                arch,
                world: self.world,
                access: self.access.as_deref(),
            })?;
        }
        Ok(())
    }

    /// Typed iteration: `q.each::<(&Transform, &mut Velocity)>(|e, (t, v)| ...)`.
    /// The fetched components are added to the filter automatically.
    pub fn each<F: Fetch>(&self, mut f: impl FnMut(Entity, F::Item<'_>)) -> Result<()> {
        let registry = self.world.registry();
        let comps = F::components(registry)?;
        if let Some(access) = &self.access {
            for &(c, write) in &comps {
                let ok = if write {
                    access.allows_write(c)
                } else {
                    access.allows_read(c)
                };
                if !ok {
                    return Err(access_violation(registry, c, write));
                }
            }
        }
        for arch in self.world.archetypes() {
            if arch.is_empty() || !self.matches(arch) || !comps.iter().all(|(c, _)| arch.has(*c)) {
                continue;
            }
            let view = ArchetypeView {
                arch,
                world: self.world,
                access: self.access.as_deref(),
            };
            let mut cols = F::cols(&view)?;
            let entities = arch.entities();
            for (row, &e) in entities.iter().enumerate() {
                f(e, F::item(&mut cols, row));
            }
        }
        Ok(())
    }
}

/// Column access into one archetype during a query.
pub struct ArchetypeView<'a> {
    arch: &'a Archetype,
    world: &'a World,
    access: Option<&'a Access>,
}

impl<'a> ArchetypeView<'a> {
    /// Row count.
    pub fn len(&self) -> usize {
        self.arch.len()
    }
    /// True if no rows.
    pub fn is_empty(&self) -> bool {
        self.arch.is_empty()
    }
    /// Entities in row order.
    pub fn entities(&self) -> &'a [Entity] {
        self.arch.entities()
    }
    /// The archetype.
    pub fn archetype(&self) -> &'a Archetype {
        self.arch
    }
    /// The world's registry.
    pub fn registry(&self) -> &'a Registry {
        self.world.registry()
    }

    fn check(&self, c: ComponentId, write: bool) -> Result<&'a Column> {
        if let Some(a) = self.access {
            let ok = if write {
                a.allows_write(c)
            } else {
                a.allows_read(c)
            };
            if !ok {
                return Err(access_violation(self.world.registry(), c, write));
            }
        }
        self.arch.column(c).ok_or_else(|| {
            ArgusError::new(
                codes::MISSING_COMPONENT,
                format!(
                    "archetype has no {} column",
                    self.world.registry().schema(c).name
                ),
            )
            .hint("Add the component to the query's has list.")
        })
    }

    fn id_of<T: Component>(&self) -> Result<ComponentId> {
        self.world.registry().id_of::<T>().ok_or_else(|| {
            ArgusError::new(
                codes::UNKNOWN_COMPONENT,
                format!("{} is not registered", T::NAME),
            )
        })
    }

    /// Shared typed column.
    pub fn column<T: Component>(&self) -> Result<ColRef<'a, T>> {
        let c = self.id_of::<T>()?;
        self.check(c, false)?.borrow::<T>()
    }

    /// Exclusive typed column.
    pub fn column_mut<T: Component>(&self) -> Result<ColMut<'a, T>> {
        let c = self.id_of::<T>()?;
        self.check(c, true)?.borrow_mut::<T>()
    }

    /// Read-only dynamic column (field access by name).
    pub fn dyn_column(&self, c: ComponentId) -> Result<DynColumn<'a>> {
        let col = self.check(c, false)?;
        col.acquire_shared()?;
        Ok(DynColumn {
            col,
            schema: self.world.registry().schema(c),
            strings: self.world.strings(),
            writable: false,
        })
    }

    /// Writable dynamic column.
    pub fn dyn_column_mut(&self, c: ComponentId) -> Result<DynColumn<'a>> {
        let col = self.check(c, true)?;
        col.acquire_exclusive()?;
        Ok(DynColumn {
            col,
            schema: self.world.registry().schema(c),
            strings: self.world.strings(),
            writable: true,
        })
    }
}

/// Field-level access to one column, for the Luau binding and the protocol.
pub struct DynColumn<'a> {
    col: &'a Column,
    schema: &'a ComponentSchema,
    strings: &'a Interner,
    writable: bool,
}

impl<'a> DynColumn<'a> {
    /// Row count.
    pub fn len(&self) -> usize {
        self.col.len()
    }
    /// The first row's bytes, for code that reads the column as rows of a
    /// type laid out as the schema (a plugin, phase 5 item 6). Valid while
    /// this borrow lives; written through only when it is writable.
    pub fn as_ptr(&self) -> *mut u8 {
        if self.col.is_empty() || self.col.stride() == 0 {
            core::ptr::NonNull::<u64>::dangling().as_ptr().cast()
        } else {
            self.col.row_ptr_mut(0)
        }
    }
    /// Whether the borrow is exclusive.
    pub fn is_writable(&self) -> bool {
        self.writable
    }
    /// True if no rows.
    pub fn is_empty(&self) -> bool {
        self.col.is_empty()
    }
    /// The schema.
    pub fn schema(&self) -> &'a ComponentSchema {
        self.schema
    }
    /// Field schema by name.
    pub fn field(&self, name: &str) -> Result<&'a FieldSchema> {
        self.schema.field(name).ok_or_else(|| {
            ArgusError::new(
                codes::UNKNOWN_FIELD,
                format!("{} has no field {name:?}", self.schema.name),
            )
            .at(self.schema.source.clone())
        })
    }
    /// Read a field.
    pub fn get(&self, row: usize, field: &str) -> Result<Value> {
        let f = self.field(field)?;
        Ok(self.get_by(row, f))
    }
    /// Read a field by schema (skips the name lookup).
    pub fn get_by(&self, row: usize, f: &FieldSchema) -> Value {
        assert!(row < self.col.len(), "row out of range");
        // SAFETY: offset + size within the row.
        unsafe { f.ty.read(self.col.row_ptr(row).add(f.offset), self.strings) }
    }
    /// A numeric or bool field as f64 (a vec2 as `axis` 0 or 1), without
    /// building a `Value`: the per-row read of systems that compare fields
    /// every tick (the animator's transitions). `None` for other types.
    pub fn num_by(&self, row: usize, f: &FieldSchema, axis: Option<u8>) -> Option<f64> {
        assert!(row < self.col.len(), "row out of range");
        // SAFETY: offset + size within the row; the type decides the width.
        unsafe {
            let p = self.col.row_ptr(row).add(f.offset);
            macro_rules! rd {
                ($t:ty) => {
                    core::ptr::read_unaligned(p as *const $t)
                };
            }
            Some(match (f.ty, axis) {
                (FieldType::Bool, _) => (rd!(u8) != 0) as u8 as f64,
                (FieldType::U8, _) => rd!(u8) as f64,
                (FieldType::U16, _) => rd!(u16) as f64,
                (FieldType::U32, _) => rd!(u32) as f64,
                (FieldType::U64, _) => rd!(u64) as f64,
                (FieldType::I8, _) => rd!(i8) as f64,
                (FieldType::I16, _) => rd!(i16) as f64,
                (FieldType::I32, _) => rd!(i32) as f64,
                (FieldType::I64, _) => rd!(i64) as f64,
                (FieldType::F32, _) => rd!(f32) as f64,
                (FieldType::F64, _) => rd!(f64),
                (FieldType::Vec2, Some(0)) => rd!(Vec2).x as f64,
                (FieldType::Vec2, Some(1)) => rd!(Vec2).y as f64,
                _ => return None,
            })
        }
    }

    /// A string or reference field's interned id, without resolving it.
    pub fn str_id_by(&self, row: usize, f: &FieldSchema) -> Option<StrId> {
        assert!(row < self.col.len(), "row out of range");
        match f.ty {
            // SAFETY: a Str or Ref field is a StrId at its offset.
            FieldType::Str | FieldType::Ref => Some(unsafe {
                core::ptr::read_unaligned(self.col.row_ptr(row).add(f.offset) as *const StrId)
            }),
            _ => None,
        }
    }

    /// Write a string or reference field's interned id, without resolving
    /// it (a system writing the same few strings every tick).
    pub fn set_str_id_by(&self, row: usize, f: &FieldSchema, id: StrId) -> Result<()> {
        if !self.writable {
            return Err(ArgusError::new(
                codes::ACCESS_VIOLATION,
                format!("{}.{} fetched read-only", self.schema.name, f.name),
            )
            .hint("Fetch the column with dyn_column_mut, and declare the write."));
        }
        assert!(row < self.col.len(), "row out of range");
        match f.ty {
            // SAFETY: a Str or Ref field is a StrId at its offset; exclusive
            // borrow held.
            FieldType::Str | FieldType::Ref => unsafe {
                core::ptr::write_unaligned(
                    self.col.row_ptr_mut(row).add(f.offset) as *mut StrId,
                    id,
                );
                Ok(())
            },
            _ => Err(ArgusError::new(
                codes::TYPE_MISMATCH,
                format!("{}.{} is not a string", self.schema.name, f.name),
            )),
        }
    }

    /// Write a field with coercion and range check.
    pub fn set(&self, row: usize, field: &str, value: impl Into<Value>) -> Result<()> {
        let f = self.field(field)?;
        self.set_by(row, f, value.into())
    }
    /// Write a field by schema.
    pub fn set_by(&self, row: usize, f: &FieldSchema, value: Value) -> Result<()> {
        if !self.writable {
            return Err(ArgusError::new(
                codes::ACCESS_VIOLATION,
                format!("{}.{} fetched read-only", self.schema.name, f.name),
            )
            .hint("Fetch the column with dyn_column_mut, and declare the write."));
        }
        assert!(row < self.col.len(), "row out of range");
        let value = self.schema.check(f, value)?;
        // SAFETY: offset + size within the row; exclusive borrow held.
        unsafe {
            f.ty.write(
                self.col.row_ptr_mut(row).add(f.offset),
                &value,
                self.strings,
            )
        }
    }
}

impl Drop for DynColumn<'_> {
    fn drop(&mut self) {
        if self.writable {
            self.col.release_exclusive();
        } else {
            self.col.release_shared();
        }
    }
}

/// What a typed query fetches per row. Implemented for `&T`, `&mut T`, and
/// tuples of those up to four elements.
pub trait Fetch {
    /// Per-archetype column guards.
    type Cols<'a>;
    /// Per-row item handed to the closure.
    type Item<'b>;
    /// Components fetched, with `true` for writes.
    fn components(registry: &Registry) -> Result<Vec<(ComponentId, bool)>>;
    /// Acquire columns for one archetype.
    fn cols<'a>(view: &ArchetypeView<'a>) -> Result<Self::Cols<'a>>;
    /// Build the row item.
    fn item<'b>(cols: &'b mut Self::Cols<'_>, row: usize) -> Self::Item<'b>;
}

impl<T: Component> Fetch for &T {
    type Cols<'a> = ColRef<'a, T>;
    type Item<'b> = &'b T;
    fn components(registry: &Registry) -> Result<Vec<(ComponentId, bool)>> {
        let c = registry.id_of::<T>().ok_or_else(|| {
            ArgusError::new(
                codes::UNKNOWN_COMPONENT,
                format!("{} is not registered", T::NAME),
            )
        })?;
        Ok(vec![(c, false)])
    }
    fn cols<'a>(view: &ArchetypeView<'a>) -> Result<Self::Cols<'a>> {
        view.column::<T>()
    }
    fn item<'b>(cols: &'b mut Self::Cols<'_>, row: usize) -> Self::Item<'b> {
        &cols[row]
    }
}

impl<T: Component> Fetch for &mut T {
    type Cols<'a> = ColMut<'a, T>;
    type Item<'b> = &'b mut T;
    fn components(registry: &Registry) -> Result<Vec<(ComponentId, bool)>> {
        let c = registry.id_of::<T>().ok_or_else(|| {
            ArgusError::new(
                codes::UNKNOWN_COMPONENT,
                format!("{} is not registered", T::NAME),
            )
        })?;
        Ok(vec![(c, true)])
    }
    fn cols<'a>(view: &ArchetypeView<'a>) -> Result<Self::Cols<'a>> {
        view.column_mut::<T>()
    }
    fn item<'b>(cols: &'b mut Self::Cols<'_>, row: usize) -> Self::Item<'b> {
        &mut cols[row]
    }
}

macro_rules! tuple_fetch {
    ($($name:ident $idx:tt),+) => {
        impl<$($name: Fetch),+> Fetch for ($($name,)+) {
            type Cols<'a> = ($($name::Cols<'a>,)+);
            type Item<'b> = ($($name::Item<'b>,)+);
            fn components(registry: &Registry) -> Result<Vec<(ComponentId, bool)>> {
                let mut v = Vec::new();
                $( v.extend($name::components(registry)?); )+
                Ok(v)
            }
            fn cols<'a>(view: &ArchetypeView<'a>) -> Result<Self::Cols<'a>> {
                Ok(($($name::cols(view)?,)+))
            }
            fn item<'b>(cols: &'b mut Self::Cols<'_>, row: usize) -> Self::Item<'b> {
                ($($name::item(&mut cols.$idx, row),)+)
            }
        }
    };
}
tuple_fetch!(A 0);
tuple_fetch!(A 0, B 1);
tuple_fetch!(A 0, B 1, C 2);
tuple_fetch!(A 0, B 1, C 2, D 3);
