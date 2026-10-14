//! Component schemas and the registry (§3 "Components"). A component is a
//! schema plus a column. The schema is declared once, from Rust with
//! `#[derive(Component)]` or at runtime with [`SchemaBuilder`] (the Luau
//! path, §7), and is what `world.describe` returns: every field's type,
//! default, optional range, doc string, and `tune` flag. The protocol never
//! distinguishes Rust-declared from script-declared components.

use crate::entity::Entity;
use crate::error::{codes, ArgusError, Result};
use crate::intern::{Interner, StrId};
use crate::value::{FieldType, Value, Vec2};
use std::any::TypeId;
use std::collections::HashMap;

/// Index of a registered component within a [`Registry`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ComponentId(pub u32);

/// Data components have fields; tags are zero-size (§3 "Tags are zero-size
/// components").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComponentKind {
    /// Has at least one field.
    Data,
    /// Zero-size marker.
    Tag,
}

/// One field of a component.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldSchema {
    /// Field name, unique within the component.
    pub name: String,
    /// Storage type.
    pub ty: FieldType,
    /// Byte offset within the row.
    pub offset: usize,
    /// Value a fresh row gets.
    pub default: Value,
    /// Inclusive numeric range, checked on every dynamic write.
    pub range: Option<(f64, f64)>,
    /// What the field means; this is what the agent reads.
    pub doc: String,
    /// Feel parameter: exposed in the tuning panel and logged by the recorder (§10).
    pub tune: bool,
}

/// A component's full description and row layout.
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentSchema {
    /// Component name, e.g. `HitStop`. Unique within a registry.
    pub name: String,
    /// What the component means.
    pub doc: String,
    /// Schema version, for save migrations (§6 "Save games").
    pub version: u32,
    /// Fields in declaration order.
    pub fields: Vec<FieldSchema>,
    /// Row size in bytes (0 for tags).
    pub size: usize,
    /// Row alignment in bytes (1 for tags).
    pub align: usize,
    /// `file:line` of the declaration, carried into error `ref`s.
    pub source: Option<String>,
    /// Rust type backing this schema, if declared from Rust.
    pub rust_type: Option<TypeId>,
}

impl ComponentSchema {
    /// Data or tag.
    pub fn kind(&self) -> ComponentKind {
        if self.fields.is_empty() {
            ComponentKind::Tag
        } else {
            ComponentKind::Data
        }
    }

    /// True if any field is an entity reference (so maintenance must scan it).
    pub fn has_entity_refs(&self) -> bool {
        self.fields.iter().any(|f| f.ty == FieldType::Entity)
    }

    /// Look up a field by name.
    pub fn field(&self, name: &str) -> Option<&FieldSchema> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// Validate `value` against a field: coerce to the storage type, then
    /// check the declared range. Errors carry the §5 shape.
    pub fn check(&self, field: &FieldSchema, value: Value) -> Result<Value> {
        let value = value.coerce(field.ty).map_err(|e| {
            ArgusError::new(
                e.code,
                format!("{}.{}: {}", self.name, field.name, e.message),
            )
            .hint(format!(
                "{}.{} is declared as {}.",
                self.name, field.name, field.ty
            ))
            .at(self.source.clone())
        })?;
        if let (Some((lo, hi)), Some(v)) = (field.range, value.as_f64()) {
            if v < lo || v > hi {
                return Err(ArgusError::new(
                    codes::OUT_OF_RANGE,
                    format!(
                        "{}.{} must be in range [{lo}, {hi}]; got {v}",
                        self.name, field.name
                    ),
                )
                .hint(format!(
                    "Use {lo} for the lowest legal value or {hi} for the highest."
                ))
                .at(self.source.clone()));
            }
        }
        Ok(value)
    }

    /// Build the bytes of a default row. Padding is zeroed so rows are
    /// deterministic and snapshots of equal worlds compare equal.
    pub fn default_row(&self, strings: &Interner) -> Vec<u8> {
        let mut row = vec![0u8; self.size];
        for f in &self.fields {
            // SAFETY: offset + size <= self.size by construction.
            unsafe {
                f.ty.write(row.as_mut_ptr().add(f.offset), &f.default, strings)
                    .expect("schema default must match its field type");
            }
        }
        row
    }

    fn validate(&self) -> Result<()> {
        let src = self.source.clone();
        if self.name.is_empty() || self.name.contains(['.', ' ', '@']) {
            return Err(ArgusError::new(
                codes::BAD_SCHEMA,
                format!("component name {:?} is invalid", self.name),
            )
            .hint("Names are identifiers like HitStop or WaveSpawner; no dots, spaces, or @.")
            .at(src));
        }
        for (i, f) in self.fields.iter().enumerate() {
            if f.name.is_empty() || f.name.contains(['.', ' ']) {
                return Err(ArgusError::new(
                    codes::BAD_SCHEMA,
                    format!("{}: field name {:?} is invalid", self.name, f.name),
                )
                .at(src));
            }
            if self.fields[..i].iter().any(|g| g.name == f.name) {
                return Err(ArgusError::new(
                    codes::BAD_SCHEMA,
                    format!("{}: field {:?} is declared twice", self.name, f.name),
                )
                .at(src));
            }
            if f.offset % f.ty.align() != 0 || f.offset + f.ty.size() > self.size {
                return Err(ArgusError::new(
                    codes::BAD_SCHEMA,
                    format!(
                        "{}.{}: offset {} does not fit the row layout",
                        self.name, f.name, f.offset
                    ),
                )
                .at(src));
            }
            if f.range.is_some() && !f.ty.is_numeric() {
                return Err(ArgusError::new(
                    codes::BAD_SCHEMA,
                    format!(
                        "{}.{}: range on non-numeric type {}",
                        self.name, f.name, f.ty
                    ),
                )
                .hint("Ranges apply to integer and float fields only.")
                .at(src));
            }
            // Defaults must satisfy their own range and type.
            self.check(f, f.default.clone()).map_err(|e| {
                ArgusError::new(
                    codes::BAD_SCHEMA,
                    format!("default value invalid: {}", e.message),
                )
                .at(src.clone())
            })?;
        }
        if self.size != 0 && !self.size.is_multiple_of(self.align) {
            return Err(ArgusError::new(
                codes::BAD_SCHEMA,
                format!(
                    "{}: size {} is not a multiple of align {}",
                    self.name, self.size, self.align
                ),
            )
            .at(src));
        }
        Ok(())
    }
}

/// Builds a [`ComponentSchema`] at runtime with C struct layout rules, which
/// is how a Luau `component "Name" { ... }` declaration becomes a column.
/// The layout algorithm matches `#[repr(C)]`, so a script schema and a Rust
/// struct with the same fields in the same order share a row layout.
#[derive(Debug)]
pub struct SchemaBuilder {
    schema: ComponentSchema,
    cursor: usize,
}

impl SchemaBuilder {
    /// Start a schema.
    pub fn new(name: impl Into<String>) -> SchemaBuilder {
        SchemaBuilder {
            schema: ComponentSchema {
                name: name.into(),
                doc: String::new(),
                version: 1,
                fields: Vec::new(),
                size: 0,
                align: 1,
                source: None,
                rust_type: None,
            },
            cursor: 0,
        }
    }
    /// Component doc string.
    pub fn doc(mut self, doc: impl Into<String>) -> Self {
        self.schema.doc = doc.into();
        self
    }
    /// Schema version (default 1).
    pub fn version(mut self, v: u32) -> Self {
        self.schema.version = v;
        self
    }
    /// `file:line` of the declaration.
    pub fn source(mut self, src: impl Into<String>) -> Self {
        self.schema.source = Some(src.into());
        self
    }
    /// Add a field with the type's zero default and no range or doc.
    pub fn field(self, name: impl Into<String>, ty: FieldType) -> Self {
        let default = ty.zero();
        self.field_full(name, ty, default, None, "", false)
    }
    /// Add a field with every attribute.
    pub fn field_full(
        mut self,
        name: impl Into<String>,
        ty: FieldType,
        default: Value,
        range: Option<(f64, f64)>,
        doc: impl Into<String>,
        tune: bool,
    ) -> Self {
        let align = ty.align();
        let offset = self.cursor.div_ceil(align) * align;
        self.cursor = offset + ty.size();
        self.schema.align = self.schema.align.max(align);
        self.schema.fields.push(FieldSchema {
            name: name.into(),
            ty,
            offset,
            default,
            range,
            doc: doc.into(),
            tune,
        });
        self
    }
    /// Finish: rounds the size up to the alignment and validates.
    pub fn build(mut self) -> Result<ComponentSchema> {
        let a = self.schema.align;
        self.schema.size = self.cursor.div_ceil(a) * a;
        self.schema.validate()?;
        Ok(self.schema)
    }
}

/// A Rust type that can be a component field. Implemented for every
/// [`FieldType`]; the derive macro uses it to type fields.
pub trait FieldValue: Copy + 'static {
    /// The storage type.
    const TYPE: FieldType;
    /// Convert to a dynamic value (used for schema defaults).
    fn to_value(self) -> Value;
}

macro_rules! field_value {
    ($($t:ty => $ft:ident, $conv:expr;)*) => {$(
        impl FieldValue for $t {
            const TYPE: FieldType = FieldType::$ft;
            fn to_value(self) -> Value { ($conv)(self) }
        }
    )*};
}
field_value! {
    bool => Bool, Value::Bool;
    u8   => U8,  |v: u8|  Value::UInt(v as u64);
    u16  => U16, |v: u16| Value::UInt(v as u64);
    u32  => U32, |v: u32| Value::UInt(v as u64);
    u64  => U64, Value::UInt;
    i8   => I8,  |v: i8|  Value::Int(v as i64);
    i16  => I16, |v: i16| Value::Int(v as i64);
    i32  => I32, |v: i32| Value::Int(v as i64);
    i64  => I64, Value::Int;
    f32  => F32, |v: f32| Value::Float(shortest_f64(v));
    f64  => F64, Value::Float;
    Vec2 => Vec2, Value::Vec2;
    Entity => Entity, Value::Entity;
}

/// The f64 nearest the shortest decimal that reads back as `v`: a Rust
/// default of `0.12f32` is described as 0.12, not 0.11999999731779099, and
/// it survives any store that keeps 15 significant digits (the index), and
/// a scene file says `21.2`, not `21.200000762939453`.
pub fn shortest_f64(v: f32) -> f64 {
    if v.is_finite() {
        v.to_string().parse().unwrap_or(v as f64)
    } else {
        v as f64
    }
}

/// Interned string field. Its default schema value is the empty string; the
/// actual text is resolved through the world's [`Interner`].
impl FieldValue for StrId {
    const TYPE: FieldType = FieldType::Str;
    fn to_value(self) -> Value {
        // Defaults for Rust-declared string fields are always empty: a StrId
        // is meaningless outside the world that interned it.
        Value::Str(String::new())
    }
}

/// A reference to a def, asset, or prefab by path (`item:key.brass`).
/// Same storage as [`StrId`]; typed so schemas can say "this is a reference".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct DefRef(pub StrId);

impl FieldValue for DefRef {
    const TYPE: FieldType = FieldType::Ref;
    fn to_value(self) -> Value {
        Value::Ref(String::new())
    }
}

/// A list of points (`list of vec2`) in a Rust component: a polygon
/// collider's corners (2D staples item 2). Same storage as [`StrId`], the
/// list's encoding interned, so equal lists have equal ids in one world;
/// its default is the empty list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Points(pub StrId);

impl Points {
    /// No points.
    pub const EMPTY: Points = Points(StrId::EMPTY);

    /// Whether there are none.
    pub fn is_empty(self) -> bool {
        self.0 == StrId::EMPTY
    }

    /// The points in an interned list's text (`world.strings().get(p.0)`).
    pub fn read(text: &str) -> Vec<crate::Vec2> {
        crate::value::decode_list(crate::ListOf::Vec2, text)
            .into_iter()
            .filter_map(|v| match v {
                Value::Vec2(p) => Some(p),
                _ => None,
            })
            .collect()
    }
}

impl FieldValue for Points {
    const TYPE: FieldType = FieldType::List(crate::ListOf::Vec2);
    fn to_value(self) -> Value {
        Value::List(Vec::new())
    }
}

/// A Rust component type. Implement with `#[derive(Component)]` on a
/// `#[repr(C)]` struct whose fields all implement [`FieldValue`]; unit
/// structs become tags. `Default` supplies the field defaults.
pub trait Component: Copy + Default + 'static {
    /// The component's name in the registry.
    const NAME: &'static str;
    /// Build the schema. Called once at registration.
    fn schema() -> ComponentSchema;
}

/// All component schemas known to a world, by id, name, and Rust type.
#[derive(Clone, Debug, Default)]
pub struct Registry {
    schemas: Vec<ComponentSchema>,
    by_name: HashMap<String, ComponentId>,
    by_type: HashMap<TypeId, ComponentId>,
    /// `Inactive` and `InactiveUnder`, once registered: every query asks
    /// for them, so they are kept here rather than looked up by name.
    inactive: (Option<ComponentId>, Option<ComponentId>),
}

impl Registry {
    /// The ids of `Inactive` and `InactiveUnder` ([`crate::active`]), as
    /// far as they are registered.
    pub fn inactive_tags(&self) -> (Option<ComponentId>, Option<ComponentId>) {
        self.inactive
    }

    /// Register a schema. Registering the same name twice with an equal
    /// schema is a no-op returning the existing id; a different schema under
    /// an existing name is an error.
    pub fn register(&mut self, schema: ComponentSchema) -> Result<ComponentId> {
        schema.validate()?;
        if let Some(&id) = self.by_name.get(&schema.name) {
            let existing = &self.schemas[id.0 as usize];
            if existing.fields == schema.fields && existing.size == schema.size {
                return Ok(id);
            }
            return Err(ArgusError::new(
                codes::BAD_SCHEMA,
                format!(
                    "component {:?} is already registered with a different schema",
                    schema.name
                ),
            )
            // The way on is a scene load (found at the phase 5 gate: three
            // agents met this on a reload and found `scene load` by trying).
            .hint("A running world's rows cannot change shape: `argus scene load` starts the scene again with the new fields. To keep old saves loading, also raise the component's version and add a migration (its `migrate`; `argus docs`, Declarations).")
            .at(schema.source.clone()));
        }
        let id = ComponentId(self.schemas.len() as u32);
        match schema.name.as_str() {
            "Inactive" => self.inactive.0 = Some(id),
            "InactiveUnder" => self.inactive.1 = Some(id),
            _ => {}
        }
        self.by_name.insert(schema.name.clone(), id);
        if let Some(t) = schema.rust_type {
            self.by_type.insert(t, id);
        }
        self.schemas.push(schema);
        Ok(id)
    }

    /// Register a Rust component type.
    pub fn register_type<T: Component>(&mut self) -> Result<ComponentId> {
        if let Some(&id) = self.by_type.get(&TypeId::of::<T>()) {
            return Ok(id);
        }
        let mut schema = T::schema();
        schema.rust_type = Some(TypeId::of::<T>());
        let id = self.register(schema)?;
        self.by_type.insert(TypeId::of::<T>(), id);
        Ok(id)
    }

    /// Id by name.
    pub fn id(&self, name: &str) -> Option<ComponentId> {
        self.by_name.get(name).copied()
    }

    /// Id by Rust type.
    pub fn id_of<T: Component>(&self) -> Option<ComponentId> {
        self.by_type.get(&TypeId::of::<T>()).copied()
    }

    /// Id by name, or a structured error.
    pub fn require(&self, name: &str) -> Result<ComponentId> {
        self.id(name).ok_or_else(|| {
            ArgusError::new(
                codes::UNKNOWN_COMPONENT,
                format!("no component named {name:?}"),
            )
            .hint("world.describe lists every registered component.")
        })
    }

    /// Schema by id. Panics on an id from another registry.
    pub fn schema(&self, id: ComponentId) -> &ComponentSchema {
        &self.schemas[id.0 as usize]
    }

    /// All schemas in registration order.
    pub fn schemas(&self) -> &[ComponentSchema] {
        &self.schemas
    }

    /// Number of registered components.
    pub fn len(&self) -> usize {
        self.schemas.len()
    }

    /// True if nothing is registered.
    pub fn is_empty(&self) -> bool {
        self.schemas.is_empty()
    }

    /// Resolve a `Component.field` path (§5) to ids and the field schema.
    pub fn resolve_path<'a>(&'a self, path: &str) -> Result<(ComponentId, &'a FieldSchema)> {
        let (comp, field) = path.split_once('.').ok_or_else(|| {
            ArgusError::new(
                codes::BAD_PATH,
                format!("field path {path:?} must look like Component.field"),
            )
            .hint("Example: Transform.pos or Health.cur.")
        })?;
        let id = self.require(comp)?;
        let schema = self.schema(id);
        let f = schema.field(field).ok_or_else(|| {
            ArgusError::new(
                codes::UNKNOWN_FIELD,
                format!("{comp} has no field {field:?}"),
            )
            .hint(format!(
                "{comp} has: {}",
                schema
                    .fields
                    .iter()
                    .map(|f| f.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
            .at(schema.source.clone())
        })?;
        Ok((id, f))
    }
}
