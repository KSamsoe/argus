//! What a Luau system sees: the `q` handle passed to `run`, entity and
//! component proxies, the declared RNG stream, and value conversion.
//!
//! Every proxy holds a link to the running system's context. The link is
//! cleared the moment `run` returns, so a proxy stashed in a local and used
//! on a later frame fails with a clear error: that is the runtime half of
//! §7 "a system may not hold references across frames".

use argus_ecs::{codes, ArgusError, ComponentId, Entity, FieldSchema, FieldType, Value, Vec2};
use argus_sim::SystemCtx;
use mlua::{
    AnyUserData, Lua, MetaMethod, MultiValue, Result as LuaResult, Table as LuaTable, UserData,
    UserDataMethods, Value as LuaValue, Variadic,
};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

/// Error codes this crate adds to the 4xxx block.
pub mod script_codes {
    /// A Luau runtime error inside a system.
    pub const SCRIPT_RUNTIME: u32 = 4501;
    /// A proxy (`q`, entity, component) was used after its system's run returned.
    pub const STALE_HANDLE: u32 = 4502;
    /// The system exceeded its instruction budget.
    pub const BUDGET_EXCEEDED: u32 = 4503;
    /// Syntax error compiling a script.
    pub const SYNTAX: u32 = 4504;
    /// A declaration (`component`, `system`, `def`) is malformed.
    pub const BAD_DECLARATION: u32 = 4505;
}

/// Shared pointer to the running system's context. Null outside `run`.
pub(crate) type Link = Rc<Cell<*mut SystemCtx<'static>>>;

fn ctx<'a>(link: &Link) -> LuaResult<&'a mut SystemCtx<'static>> {
    let p = link.get();
    if p.is_null() {
        return Err(argus_err(
            ArgusError::new(
                script_codes::STALE_HANDLE,
                "q, entity, or component handle used outside its system's run",
            )
            .hint("Handles are valid only during the run call that created them; re-query each frame instead of keeping one."),
        ));
    }
    // SAFETY: the scheduler set this pointer from a live `&mut SystemCtx` for
    // the duration of one `run` call and nulls it afterwards; Lua calls into
    // Rust one at a time so no two `&mut` derived here are alive together.
    Ok(unsafe { &mut *p })
}

pub(crate) fn argus_err(e: ArgusError) -> mlua::Error {
    mlua::Error::external(e)
}

// ----- value conversion -----------------------------------------------------

pub(crate) fn value_to_lua(lua: &Lua, v: Value, link: &Link) -> LuaResult<LuaValue> {
    Ok(match v {
        Value::Bool(b) => LuaValue::Boolean(b),
        Value::Int(i) => LuaValue::Number(i as f64),
        Value::UInt(u) => LuaValue::Number(u as f64),
        Value::Float(f) => LuaValue::Number(f),
        // Luau's native vector: no allocation, `v.x`/`v.y`, arithmetic in
        // the VM (phase-1 item 0; the phase-0 gate's finding).
        Value::Vec2(v) => LuaValue::Vector(mlua::Vector::new(v.x, v.y, 0.0)),
        Value::Str(s) | Value::Ref(s) => LuaValue::String(lua.create_string(&s)?),
        Value::Entity(e) => {
            if e.is_null() {
                LuaValue::Nil
            } else {
                LuaValue::UserData(lua.create_userdata(EntityRef {
                    entity: e,
                    link: link.clone(),
                })?)
            }
        }
    })
}

/// Convert a Lua value for a field of type `ty`. Numbers become the integer
/// or float the field wants; tables `{x=,y=}` or `{1,2}` become vec2;
/// strings name entities (`@id`, `e:n`) for entity fields.
pub(crate) fn lua_to_value(v: LuaValue, ty: FieldType, link: &Link) -> LuaResult<Value> {
    let mismatch = |got: &str| {
        argus_err(ArgusError::new(
            codes::TYPE_MISMATCH,
            format!("expected a {} value; got {got}", ty.name()),
        ))
    };
    Ok(match (ty, v) {
        (FieldType::Bool, LuaValue::Boolean(b)) => Value::Bool(b),
        (t, LuaValue::Integer(i)) if t.is_numeric() => number_for(t, i as f64),
        (t, LuaValue::Number(n)) if t.is_numeric() => number_for(t, n),
        (FieldType::Vec2, LuaValue::Vector(v)) => Value::Vec2(Vec2::new(v.x(), v.y())),
        (FieldType::Vec2, LuaValue::Table(t)) => {
            let x: Option<f32> = t.get("x")?;
            let y: Option<f32> = t.get("y")?;
            match (x, y) {
                (Some(x), Some(y)) => Value::Vec2(Vec2::new(x, y)),
                _ => {
                    let x: Option<f32> = t.get(1)?;
                    let y: Option<f32> = t.get(2)?;
                    match (x, y) {
                        (Some(x), Some(y)) => Value::Vec2(Vec2::new(x, y)),
                        _ => return Err(mismatch("a table without x and y")),
                    }
                }
            }
        }
        (FieldType::Str, LuaValue::String(s)) => Value::Str(s.to_str()?.to_string()),
        (FieldType::Ref, LuaValue::String(s)) => Value::Ref(s.to_str()?.to_string()),
        (FieldType::Entity, LuaValue::Nil) => Value::Entity(Entity::NULL),
        (FieldType::Entity, LuaValue::UserData(ud)) => {
            let e = ud
                .borrow::<EntityRef>()
                .map_err(|_| mismatch("a non-entity userdata"))?;
            Value::Entity(e.entity)
        }
        (FieldType::Entity, LuaValue::String(s)) => {
            let s = s.to_str()?.to_string();
            let c = ctx(link)?;
            Value::Entity(c.entity(&s).map_err(argus_err)?)
        }
        (_, other) => return Err(mismatch(other.type_name())),
    })
}

fn number_for(ty: FieldType, n: f64) -> Value {
    match ty {
        FieldType::F32 | FieldType::F64 => Value::Float(n),
        FieldType::U8 | FieldType::U16 | FieldType::U32 | FieldType::U64
            if n >= 0.0 && n.fract() == 0.0 =>
        {
            Value::UInt(n as u64)
        }
        FieldType::I8 | FieldType::I16 | FieldType::I32 | FieldType::I64 if n.fract() == 0.0 => {
            Value::Int(n as i64)
        }
        // Let the schema check produce the type/range error with the field name.
        _ => Value::Float(n),
    }
}

// ----- entity proxy ---------------------------------------------------------

/// A live entity handle as Lua sees it.
pub struct EntityRef {
    pub(crate) entity: Entity,
    pub(crate) link: Link,
}

/// A Lua table of event fields to values, sorted by key.
fn event_fields(fields: Option<mlua::Table>) -> LuaResult<Vec<(String, Value)>> {
    let mut out = Vec::new();
    if let Some(t) = fields {
        for pair in t.pairs::<String, LuaValue>() {
            let (k, v) = pair?;
            let v = match v {
                LuaValue::Boolean(b) => Value::Bool(b),
                LuaValue::Integer(i) => Value::Int(i as i64),
                LuaValue::Number(n) if n.fract() == 0.0 && n.abs() < 9.0e15 => Value::Int(n as i64),
                LuaValue::Number(n) => Value::Float(n),
                LuaValue::String(s) => Value::Str(s.to_str()?.to_string()),
                LuaValue::Vector(v) => Value::Vec2(Vec2::new(v.x(), v.y())),
                LuaValue::UserData(ud) => match ud.borrow::<EntityRef>() {
                    Ok(e) => Value::Entity(e.entity),
                    Err(_) => Value::Str(format!("{ud:?}")),
                },
                other => Value::Str(format!("{other:?}")),
            };
            out.push((k, v));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
    }
    Ok(out)
}

/// A list of component names to ids.
fn component_ids(link: &Link, names: &LuaTable) -> LuaResult<Vec<ComponentId>> {
    let mut ids = Vec::new();
    for v in names.sequence_values::<String>() {
        ids.push(component_id(link, &v?)?);
    }
    Ok(ids)
}

/// A point from a vector, `{x=, y=}`, or `{x, y}`.
fn lua_point(v: LuaValue) -> LuaResult<(f32, f32)> {
    match v {
        LuaValue::Vector(v) => Ok((v.x(), v.y())),
        LuaValue::Table(t) => {
            let x: Option<f32> = t.get("x")?;
            let y: Option<f32> = t.get("y")?;
            if let (Some(x), Some(y)) = (x, y) {
                return Ok((x, y));
            }
            let x: Option<f32> = t.get(1)?;
            let y: Option<f32> = t.get(2)?;
            match (x, y) {
                (Some(x), Some(y)) => Ok((x, y)),
                _ => Err(mlua::Error::runtime("a point needs x and y")),
            }
        }
        other => Err(mlua::Error::runtime(format!(
            "expected a vector or a table for a point; got {}",
            other.type_name()
        ))),
    }
}

/// The host's path queries, or an error saying there are none.
fn paths_of(lua: &Lua) -> LuaResult<Rc<dyn crate::Paths>> {
    lua.app_data_ref::<crate::PathsData>()
        .map(|p| p.0.clone())
        .ok_or_else(|| {
            mlua::Error::runtime("no navigation here: the host gave scripts no path queries")
        })
}

/// A system must declare what it reads, a path query too (section 3).
fn needs_read(c: &SystemCtx<'_>, name: &str) -> LuaResult<()> {
    let id = c.world().registry().require(name).map_err(argus_err)?;
    if c.access().allows_read(id) {
        return Ok(());
    }
    Err(argus_err(
        ArgusError::new(
            codes::ACCESS_VIOLATION,
            format!(
                "system {} asks a path query, which reads {name}, without declaring it",
                c.name
            ),
        )
        .hint(format!("Add {name:?} to the system's reads list.")),
    ))
}

/// A point: a vector, a table, or an entity (where it is).
fn place(c: &SystemCtx<'_>, v: LuaValue) -> LuaResult<[f32; 2]> {
    if let LuaValue::UserData(ud) = &v {
        if let Ok(r) = ud.borrow::<EntityRef>() {
            let t = c
                .world()
                .get::<argus_ecs::Transform>(r.entity)
                .map_err(argus_err)?;
            return Ok([t.pos.x, t.pos.y]);
        }
    }
    let (x, y) = lua_point(v)?;
    Ok([x, y])
}

/// An optional grid entity.
fn grid_arg(g: Option<AnyUserData>) -> LuaResult<Entity> {
    Ok(match g {
        Some(ud) => ud.borrow::<EntityRef>()?.entity,
        None => Entity::NULL,
    })
}

fn component_id(link: &Link, name: &str) -> LuaResult<ComponentId> {
    let c = ctx(link)?;
    c.world().registry().require(name).map_err(argus_err)
}

fn component_ref(lua: &Lua, link: &Link, entity: Entity, name: &str) -> LuaResult<AnyUserData> {
    let id = component_id(link, name)?;
    let c = ctx(link)?;
    if !c.access().allows_read(id) {
        return Err(argus_err(
            ArgusError::new(
                codes::ACCESS_VIOLATION,
                format!("system {} reads {name} without declaring it", c.name),
            )
            .hint(format!("Add {name:?} to the system's reads list.")),
        ));
    }
    if !c.has(entity, id) {
        return Err(argus_err(
            ArgusError::new(
                codes::MISSING_COMPONENT,
                format!("{entity} has no {name} component"),
            )
            .hint("Check with e:has(name) first, or insert it."),
        ));
    }
    let writable = c.access().allows_write(id);
    lua.create_userdata(ComponentRef {
        link: link.clone(),
        comp: id,
        name: Arc::from(name),
        entity: Cell::new(entity),
        writable,
    })
}

impl UserData for EntityRef {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(this.entity.to_string())
        });
        m.add_meta_method(MetaMethod::Eq, |_, this, other: AnyUserData| {
            Ok(other
                .borrow::<EntityRef>()
                .map(|o| o.entity == this.entity)
                .unwrap_or(false))
        });
        m.add_method("id", |_, this, ()| Ok(this.entity.to_string()));
        m.add_method("name", |_, this, ()| {
            let c = ctx(&this.link)?;
            Ok(c.world().string_id(this.entity).map(|s| s.to_string()))
        });
        m.add_method("alive", |_, this, ()| {
            Ok(ctx(&this.link)?.is_alive(this.entity))
        });
        m.add_method("has", |_, this, name: String| {
            let id = component_id(&this.link, &name)?;
            Ok(ctx(&this.link)?.has(this.entity, id))
        });
        m.add_method("get", |lua, this, name: String| {
            component_ref(lua, &this.link, this.entity, &name)
        });
        m.add_method(
            "insert",
            |lua, this, (name, fields): (String, Option<mlua::Table>)| {
                let id = component_id(&this.link, &name)?;
                let c = ctx(&this.link)?;
                c.insert_default(this.entity, id).map_err(argus_err)?;
                if let Some(fields) = fields {
                    let r = component_ref(lua, &this.link, this.entity, &name)?;
                    let r = r.borrow::<ComponentRef>()?;
                    for pair in fields.pairs::<String, LuaValue>() {
                        let (k, v) = pair?;
                        r.set(&k, v)?;
                    }
                }
                Ok(())
            },
        );
        m.add_method("remove", |_, this, name: String| {
            let id = component_id(&this.link, &name)?;
            ctx(&this.link)?.remove(this.entity, id).map_err(argus_err)
        });
        m.add_method("despawn", |_, this, ()| {
            ctx(&this.link)?.despawn(this.entity).map_err(argus_err)
        });
    }
}

// ----- component proxy ------------------------------------------------------

/// One component on one entity; field reads and writes go through the
/// schema, so ranges and types are checked and errors carry `file:line`.
pub struct ComponentRef {
    pub(crate) link: Link,
    pub(crate) comp: ComponentId,
    pub(crate) name: Arc<str>,
    pub(crate) entity: Cell<Entity>,
    pub(crate) writable: bool,
}

impl ComponentRef {
    fn field<'a>(&self, c: &'a SystemCtx<'static>, key: &str) -> LuaResult<&'a FieldSchema> {
        let schema = c.world().registry().schema(self.comp);
        schema.field(key).ok_or_else(|| {
            argus_err(
                ArgusError::new(
                    codes::UNKNOWN_FIELD,
                    format!("{} has no field {key:?}", self.name),
                )
                .hint(format!(
                    "{} has: {}",
                    self.name,
                    schema
                        .fields
                        .iter()
                        .map(|f| f.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
                .at(schema.source.clone()),
            )
        })
    }

    pub(crate) fn get(&self, lua: &Lua, key: &str) -> LuaResult<LuaValue> {
        let c = ctx(&self.link)?;
        let f = self.field(c, key)?;
        let v = c
            .world()
            .get_field_by(self.entity.get(), self.comp, f)
            .map_err(argus_err)?;
        value_to_lua(lua, v, &self.link)
    }

    pub(crate) fn set(&self, key: &str, v: LuaValue) -> LuaResult<()> {
        if !self.writable {
            let c = ctx(&self.link)?;
            return Err(argus_err(
                ArgusError::new(
                    codes::ACCESS_VIOLATION,
                    format!(
                        "system {} writes {}.{key} without declaring it",
                        c.name, self.name
                    ),
                )
                .hint(format!("Add {:?} to the system's writes list.", self.name)),
            ));
        }
        let c = ctx(&self.link)?;
        let f = self.field(c, key)?;
        let value = lua_to_value(v, f.ty, &self.link)?;
        c.world()
            .set_field_by(self.entity.get(), self.comp, f, value)
            .map_err(argus_err)
    }
}

impl UserData for ComponentRef {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            this.get(lua, &key)
        });
        m.add_meta_method(
            MetaMethod::NewIndex,
            |_, this, (key, v): (String, LuaValue)| this.set(&key, v),
        );
        m.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("{}@{}", this.name, this.entity.get()))
        });
    }
}

// ----- rng proxy ------------------------------------------------------------

/// The system's declared RNG stream.
pub struct RngRef {
    pub(crate) link: Link,
}

impl UserData for RngRef {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("next", |_, this, ()| {
            Ok(ctx(&this.link)?.rng().map_err(argus_err)?.next_f64())
        });
        m.add_method("below", |_, this, n: u32| {
            Ok(ctx(&this.link)?.rng().map_err(argus_err)?.below(n))
        });
        m.add_method("range", |_, this, (lo, hi): (i64, i64)| {
            Ok(ctx(&this.link)?.rng().map_err(argus_err)?.range(lo, hi))
        });
        m.add_method("chance", |_, this, p: f32| {
            Ok(ctx(&this.link)?.rng().map_err(argus_err)?.chance(p))
        });
    }
}

// ----- the q handle ---------------------------------------------------------

/// The `q` argument of a system's `run`.
pub struct QueryHandle {
    pub(crate) link: Link,
}

impl UserData for QueryHandle {
    fn add_fields<F: mlua::UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("tick", |_, this| Ok(ctx(&this.link)?.tick));
        f.add_field_method_get("time", |_, this| Ok(ctx(&this.link)?.time));
        // The game's days (`[time] day` in argus.toml; phase 3 item 0):
        // q.day_length its sim seconds, q.day the whole days since sim time
        // 0, q.day_time how far into this one (0 to 1).
        let day = |lua: &Lua| {
            lua.app_data_ref::<crate::DayLength>()
                .map_or(86_400.0, |d| d.0)
        };
        f.add_field_method_get("day_length", move |lua, _| Ok(day(lua)));
        f.add_field_method_get("day", move |lua, this| {
            Ok((ctx(&this.link)?.time / day(lua)).floor() as u64)
        });
        f.add_field_method_get("day_time", move |lua, this| {
            Ok((ctx(&this.link)?.time / day(lua)).fract())
        });
        f.add_field_method_get("dt", |_, this| Ok(ctx(&this.link)?.dt));
        f.add_field_method_get("name", |_, this| Ok(ctx(&this.link)?.name.to_string()));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        // for e, a, b in q:each("A", "B") do ... end
        m.add_method("each", |lua, this, names: Variadic<String>| {
            let link = this.link.clone();
            let c = ctx(&link)?;
            let mut ids = Vec::with_capacity(names.len());
            for n in names.iter() {
                let id = c.world().registry().require(n).map_err(argus_err)?;
                if !c.access().allows_read(id) {
                    return Err(argus_err(
                        ArgusError::new(
                            codes::ACCESS_VIOLATION,
                            format!("system {} reads {n} without declaring it", c.name),
                        )
                        .hint(format!("Add {n:?} to the system's reads list.")),
                    ));
                }
                ids.push(id);
            }
            let mut q = c.world().query();
            for &id in &ids {
                q = q.has(id);
            }
            // Snapshot the entity list so spawns and despawns during the loop are safe.
            let entities = q.entities();
            let refs: Vec<AnyUserData> = names
                .iter()
                .zip(&ids)
                .map(|(n, &id)| {
                    lua.create_userdata(ComponentRef {
                        link: link.clone(),
                        comp: id,
                        name: Arc::from(n.as_str()),
                        entity: Cell::new(Entity::NULL),
                        writable: c.access().allows_write(id),
                    })
                })
                .collect::<LuaResult<_>>()?;
            let entity_ud = lua.create_userdata(EntityRef {
                entity: Entity::NULL,
                link: link.clone(),
            })?;
            let mut i = 0usize;
            let iter = lua.create_function_mut(move |_, ()| {
                let c = ctx(&link)?;
                while i < entities.len() {
                    let e = entities[i];
                    i += 1;
                    if !c.is_alive(e) || !ids.iter().all(|&id| c.has(e, id)) {
                        continue;
                    }
                    entity_ud.borrow_mut::<EntityRef>()?.entity = e;
                    let mut out = Vec::with_capacity(refs.len() + 1);
                    out.push(LuaValue::UserData(entity_ud.clone()));
                    for r in &refs {
                        r.borrow::<ComponentRef>()?.entity.set(e);
                        out.push(LuaValue::UserData(r.clone()));
                    }
                    return Ok(MultiValue::from_vec(out));
                }
                Ok(MultiValue::new())
            })?;
            Ok(iter)
        });
        m.add_method("count", |_, this, names: Variadic<String>| {
            let c = ctx(&this.link)?;
            let mut q = c.world().query();
            for n in names.iter() {
                q = q.has(c.world().registry().require(n).map_err(argus_err)?);
            }
            Ok(q.count())
        });
        m.add_method("entity", |lua, this, r: String| {
            let c = ctx(&this.link)?;
            let e = c.entity(&r).map_err(argus_err)?;
            lua.create_userdata(EntityRef {
                entity: e,
                link: this.link.clone(),
            })
        });
        m.add_method("get", |lua, this, (e, name): (AnyUserData, String)| {
            let e = e.borrow::<EntityRef>()?.entity;
            component_ref(lua, &this.link, e, &name)
        });
        // spawn(): a bare entity. spawn("id"): authored. spawn({"A", "B"}):
        // straight into that archetype at schema defaults, one placement
        // instead of one move per component.
        m.add_method("spawn", |lua, this, what: Option<LuaValue>| {
            let e = match what {
                None | Some(LuaValue::Nil) => ctx(&this.link)?.spawn(),
                Some(LuaValue::String(id)) => {
                    let id = id.to_str()?.to_string();
                    ctx(&this.link)?.spawn_authored(&id).map_err(argus_err)?
                }
                Some(LuaValue::Table(names)) => {
                    let ids = component_ids(&this.link, &names)?;
                    ctx(&this.link)?.spawn_with(&ids).map_err(argus_err)?
                }
                Some(other) => {
                    return Err(mlua::Error::runtime(format!(
                        "spawn takes nothing, an id string, or a list of component names; got {}",
                        other.type_name()
                    )))
                }
            };
            lua.create_userdata(EntityRef {
                entity: e,
                link: this.link.clone(),
            })
        });
        // spawn_many(n, {"A", "B"}, function(e, i) ... end): n entities in
        // one call, each handed to the callback through one reused handle.
        m.add_method(
            "spawn_many",
            |lua, this, (n, names, f): (u32, LuaTable, Option<mlua::Function>)| {
                let ids = component_ids(&this.link, &names)?;
                let handle = lua.create_userdata(EntityRef {
                    entity: Entity::NULL,
                    link: this.link.clone(),
                })?;
                for i in 0..n {
                    let e = ctx(&this.link)?.spawn_with(&ids).map_err(argus_err)?;
                    if let Some(f) = &f {
                        handle.borrow_mut::<EntityRef>()?.entity = e;
                        f.call::<()>((handle.clone(), i + 1))?;
                    }
                }
                Ok(n)
            },
        );
        // within(pos, r, "A", "B"): like each, over the entities within r of
        // pos (nearest first) that have every named component; the distance
        // comes last: `for e, a, b, d in q:within(p, 3, "A", "B")`.
        m.add_method(
            "within",
            |lua, this, (pos, r, names): (LuaValue, f32, Variadic<String>)| {
                let link = this.link.clone();
                let (x, y) = lua_point(pos)?;
                let c = ctx(&link)?;
                let mut ids = Vec::with_capacity(names.len());
                for n in names.iter() {
                    let id = c.world().registry().require(n).map_err(argus_err)?;
                    if !c.access().allows_read(id) {
                        return Err(argus_err(
                            ArgusError::new(
                                codes::ACCESS_VIOLATION,
                                format!("system {} reads {n} without declaring it", c.name),
                            )
                            .hint(format!("Add {n:?} to the system's reads list.")),
                        ));
                    }
                    ids.push(id);
                }
                let found = c.within(x, y, r);
                let refs: Vec<AnyUserData> = names
                    .iter()
                    .zip(&ids)
                    .map(|(n, &id)| {
                        lua.create_userdata(ComponentRef {
                            link: link.clone(),
                            comp: id,
                            name: Arc::from(n.as_str()),
                            entity: Cell::new(Entity::NULL),
                            writable: c.access().allows_write(id),
                        })
                    })
                    .collect::<LuaResult<_>>()?;
                let entity_ud = lua.create_userdata(EntityRef {
                    entity: Entity::NULL,
                    link: link.clone(),
                })?;
                let mut i = 0usize;
                let iter = lua.create_function_mut(move |_, ()| {
                    let c = ctx(&link)?;
                    while i < found.len() {
                        let (e, d) = found[i];
                        i += 1;
                        if !c.is_alive(e) || !ids.iter().all(|&id| c.has(e, id)) {
                            continue;
                        }
                        entity_ud.borrow_mut::<EntityRef>()?.entity = e;
                        let mut out = Vec::with_capacity(refs.len() + 2);
                        out.push(LuaValue::UserData(entity_ud.clone()));
                        for r in &refs {
                            r.borrow::<ComponentRef>()?.entity.set(e);
                            out.push(LuaValue::UserData(r.clone()));
                        }
                        out.push(LuaValue::Number(d as f64));
                        return Ok(MultiValue::from_vec(out));
                    }
                    Ok(MultiValue::new())
                })?;
                Ok(iter)
            },
        );
        // nearest(pos, r, "A", "B"): the closest entity within r having the
        // named components, and its distance; nil when none.
        m.add_method(
            "nearest",
            |lua, this, (pos, r, names): (LuaValue, f32, Variadic<String>)| {
                let (x, y) = lua_point(pos)?;
                let c = ctx(&this.link)?;
                let mut ids = Vec::with_capacity(names.len());
                for n in names.iter() {
                    ids.push(c.world().registry().require(n).map_err(argus_err)?);
                }
                match c.nearest(x, y, r, |e| ids.iter().all(|&id| c.has(e, id))) {
                    Some((e, d)) => Ok((
                        LuaValue::UserData(lua.create_userdata(EntityRef {
                            entity: e,
                            link: this.link.clone(),
                        })?),
                        LuaValue::Number(d as f64),
                    )),
                    None => Ok((LuaValue::Nil, LuaValue::Nil)),
                }
            },
        );
        // Paths (phase 3 item 2, section 12): q:path(from, to, grid?) is
        // the waypoints (vectors) and the cost in steps, or nil;
        // q:walkable(pos, grid?); q:distance(field, pos) is the steps to a
        // NavField's entity, or nil; q:downhill(field, pos) the next cell's
        // centre toward it, or nil. A point is a vector, a table, or an
        // entity (its position). A system that asks declares reads of
        // NavGrid (path, walkable) or NavField (distance, downhill).
        m.add_method(
            "path",
            |lua, this, (from, to, grid): (LuaValue, LuaValue, Option<AnyUserData>)| {
                let c = ctx(&this.link)?;
                needs_read(c, "NavGrid")?;
                let paths = paths_of(lua)?;
                let (a, b) = (place(c, from)?, place(c, to)?);
                let g = grid_arg(grid)?;
                match paths.path(c.world(), a, b, g).map_err(argus_err)? {
                    Some((pts, cost)) => {
                        let t = lua.create_table()?;
                        for (i, p) in pts.iter().enumerate() {
                            t.set(i + 1, LuaValue::Vector(mlua::Vector::new(p[0], p[1], 0.0)))?;
                        }
                        Ok((LuaValue::Table(t), LuaValue::Number(cost as f64)))
                    }
                    None => Ok((LuaValue::Nil, LuaValue::Nil)),
                }
            },
        );
        m.add_method(
            "walkable",
            |lua, this, (at, grid): (LuaValue, Option<AnyUserData>)| {
                let c = ctx(&this.link)?;
                needs_read(c, "NavGrid")?;
                let paths = paths_of(lua)?;
                paths
                    .walkable(c.world(), place(c, at)?, grid_arg(grid)?)
                    .map_err(argus_err)
            },
        );
        m.add_method(
            "distance",
            |lua, this, (field, at): (AnyUserData, LuaValue)| {
                let c = ctx(&this.link)?;
                needs_read(c, "NavField")?;
                let paths = paths_of(lua)?;
                let f = field.borrow::<EntityRef>()?.entity;
                Ok(paths
                    .distance(c.world(), f, place(c, at)?)
                    .map_err(argus_err)?
                    .map(|d| d as f64))
            },
        );
        m.add_method(
            "downhill",
            |lua, this, (field, at): (AnyUserData, LuaValue)| {
                let c = ctx(&this.link)?;
                needs_read(c, "NavField")?;
                let paths = paths_of(lua)?;
                let f = field.borrow::<EntityRef>()?.entity;
                Ok(paths
                    .downhill(c.world(), f, place(c, at)?)
                    .map_err(argus_err)?
                    .map(|p| LuaValue::Vector(mlua::Vector::new(p[0], p[1], 0.0)))
                    .unwrap_or(LuaValue::Nil))
            },
        );
        m.add_method("despawn", |_, this, e: AnyUserData| {
            let e = e.borrow::<EntityRef>()?.entity;
            ctx(&this.link)?.despawn(e).map_err(argus_err)
        });
        m.add_method(
            "emit",
            |_, this, (name, fields): (String, Option<mlua::Table>)| {
                let out = event_fields(fields)?;
                ctx(&this.link)?.emit(name, out);
                Ok(())
            },
        );
        // events("body.landed"): the game events emitted so far this tick
        // with that name (all of them for no name), oldest first, each a
        // table of its fields plus `name`. How a script reacts to a contact
        // or a landing the physics step reported (section 8 L2, section 12).
        m.add_method("events", |lua, this, name: Option<String>| {
            let c = ctx(&this.link)?;
            let name = name.unwrap_or_default();
            let out = lua.create_table()?;
            let mut n = 0;
            for r in c.events_now(&name) {
                if let argus_ecs::Event::Game { name: en, fields } = &r.event {
                    let t = lua.create_table()?;
                    t.set("name", en.as_str())?;
                    for (k, v) in fields {
                        t.set(k.as_str(), value_to_lua(lua, v.clone(), &this.link)?)?;
                    }
                    n += 1;
                    out.set(n, t)?;
                }
            }
            Ok(out)
        });
        // tween(e, "Transform.scale", {to = vector.create(1, 1, 0), from = ...,
        // duration = 0.15, easing = "out_back", on_complete = "squash.done"}):
        // spawn a Tween entity driving one field of e (section 13). Needs a
        // declared write of Tween. Returns the tween's entity.
        m.add_method(
            "tween",
            |lua, this, (target, path, opts): (AnyUserData, String, Option<mlua::Table>)| {
                let target = target.borrow::<EntityRef>()?.entity;
                let c = ctx(&this.link)?;
                let tw = c.world().registry().require("Tween").map_err(argus_err)?;
                let e = c.spawn_with(&[tw]).map_err(argus_err)?;
                let set = |f: &str, v: Value| c.set_field(e, &format!("Tween.{f}"), v).map_err(argus_err);
                set("target", Value::Entity(target))?;
                set("path", Value::Str(path))?;
                let vec = |v: LuaValue| -> LuaResult<Value> {
                    match v {
                        LuaValue::Integer(i) => Ok(Value::Vec2(Vec2::new(i as f32, 0.0))),
                        LuaValue::Number(n) => Ok(Value::Vec2(Vec2::new(n as f32, 0.0))),
                        other => lua_to_value(other, FieldType::Vec2, &this.link),
                    }
                };
                if let Some(o) = opts {
                    for pair in o.pairs::<String, LuaValue>() {
                        let (k, v) = pair?;
                        match k.as_str() {
                            "from" => {
                                set("from", vec(v)?)?;
                                set("from_current", Value::Bool(false))?;
                            }
                            "to" => set("to", vec(v)?)?,
                            "duration" | "delay" | "repeats" | "yoyo" | "keep" | "easing" | "on_complete" => {
                                let ty = c.world().registry().resolve_path(&format!("Tween.{k}")).map_err(argus_err)?.1.ty;
                                set(&k, lua_to_value(v, ty, &this.link)?)?;
                            }
                            other => {
                                return Err(argus_err(ArgusError::new(
                                    codes::TYPE_MISMATCH,
                                    format!("unknown tween option {other:?}"),
                                )
                                .hint("Options are from, to, duration, delay, easing, repeats, yoyo, on_complete, keep.")))
                            }
                        }
                    }
                }
                lua.create_userdata(EntityRef {
                    entity: e,
                    link: this.link.clone(),
                })
            },
        );
        // play("sfx/step", {entity = e, gain = 0.8, pitch = 1.2}): ask the
        // mixer for a sound. It is a `sound.play` event; the daemon answers
        // with `sound.played` naming the variation it chose (section 13).
        m.add_method(
            "play",
            |_, this, (sound, fields): (String, Option<mlua::Table>)| {
                let mut out = event_fields(fields)?;
                out.retain(|(k, _)| k != "sound");
                out.push(("sound".to_string(), Value::Str(sound)));
                out.sort_by(|a, b| a.0.cmp(&b.0));
                ctx(&this.link)?.emit("sound.play", out);
                Ok(())
            },
        );
        // save("auto") / load("auto"): ask whatever runs the game to save
        // it, or load a save, between ticks (section 6; phase 3 item 0). A
        // `save.requested` or `load.requested` event: the save holds the
        // world at the end of this tick; a load happens after the step. The
        // daemon keeps saves in .argus/saves/, a release build where the
        // platform says, the web page in the browser's storage; each
        // answers with game.saved or game.loaded.
        for (method, event) in [("save", "save.requested"), ("load", "load.requested")] {
            m.add_method(method, move |_, this, name: Option<String>| {
                let name = name.unwrap_or_else(|| "auto".to_string());
                ctx(&this.link)?.emit(event, vec![("name".to_string(), Value::Str(name))]);
                Ok(())
            });
        }
        // speed(x): ask for x ticks a frame (section 6, Outpost's 1x/3x/10x):
        // a `speed.requested` event the scheduler answers at the end of this
        // tick, from the next frame, inside the run, so every host and
        // every replay agree. speed(): the clock's speed as this tick began,
        // however it was set (a game's request, the editor's bar, sim.speed),
        // for a HUD to show. Not 0: no tick would run to ask for another
        // speed; a game pauses itself with q:freeze().
        m.add_method("speed", |_, this, x: Option<f64>| {
            let Some(x) = x else {
                return Ok(Some(ctx(&this.link)?.speed));
            };
            if !(x > 0.0 && x <= 1000.0) {
                return Err(mlua::Error::runtime(format!(
                    "speed {x} is not a speed: more than 0, up to 1000 ticks a frame (a game pauses with q:freeze())"
                )));
            }
            ctx(&this.link)?.emit("speed.requested", vec![("x".to_string(), Value::Float(x))]);
            Ok(None)
        });
        // freeze(): the rest of this tick stands still, but for systems
        // declared unfrozen (the UI's among them): a pause menu, a hit-stop
        // of the game's own (phase 3 item 1; phase 2 item 6's freeze).
        m.add_method("freeze", |_, this, ()| {
            ctx(&this.link)?.freeze();
            Ok(())
        });
        m.add_method("rng", |lua, this, ()| {
            lua.create_userdata(RngRef {
                link: this.link.clone(),
            })
        });
        // text("hud.day", {day = 3}): a player-facing string in the world's
        // language (section 12, phase 3 item 4), its {name} holes filled
        // from the table and its {@id:Component.field} holes from the
        // world; "[key]" where no string table has the key.
        m.add_method(
            "text",
            |lua, this, (key, args): (String, Option<mlua::Table>)| {
                let texts = lua.app_data_ref::<crate::TextsData>().map(|t| t.0.clone());
                let c = ctx(&this.link)?;
                let raw = texts.as_ref().and_then(|t| t.text(c.world(), &key));
                let mut s = raw.unwrap_or_else(|| format!("[{key}]"));
                if let Some(a) = args {
                    for pair in a.pairs::<String, LuaValue>() {
                        let (k, v) = pair?;
                        let v = match v {
                            LuaValue::String(s) => s.to_str()?.to_string(),
                            LuaValue::Integer(i) => i.to_string(),
                            LuaValue::Number(n) if n.fract() == 0.0 && n.abs() < 1e15 => {
                                (n as i64).to_string()
                            }
                            LuaValue::Number(n) => format!("{n}"),
                            LuaValue::Boolean(b) => b.to_string(),
                            other => other.to_string()?,
                        };
                        s = s.replace(&format!("{{{k}}}"), &v);
                    }
                }
                if s.contains("{@") {
                    let mut problems = Vec::new();
                    s = argus_ecs::text::fill(c.world(), Entity::NULL, &s, &mut problems);
                }
                Ok(s)
            },
        );
        // languages(): the languages there are string tables for, the base
        // first (a settings screen's choices).
        m.add_method("languages", |lua, _, ()| {
            let list = lua.create_table()?;
            let texts = lua.app_data_ref::<crate::TextsData>().map(|t| t.0.clone());
            if let Some(t) = texts {
                for (i, l) in t.languages().into_iter().enumerate() {
                    list.set(i + 1, l)?;
                }
            }
            Ok(list)
        });
        // A data table's row, read-only (§4): q:def("item:bread").name.
        m.add_method("def", |lua, _, r: String| def_row(lua, &r));
        // A table's rows in file order: for _, r in q:defs("recipes") do.
        m.add_method("defs", |lua, _, name: String| def_table(lua, &name));
    }
}

/// A def value as Lua: the numbers, strings, and vectors a table holds.
fn def_value(lua: &Lua, v: Value) -> LuaResult<LuaValue> {
    Ok(match v {
        Value::Bool(b) => LuaValue::Boolean(b),
        Value::Int(i) => LuaValue::Number(i as f64),
        Value::UInt(u) => LuaValue::Number(u as f64),
        Value::Float(f) => LuaValue::Number(f),
        Value::Vec2(v) => LuaValue::Vector(mlua::Vector::new(v.x, v.y, 0.0)),
        Value::Str(s) | Value::Ref(s) => LuaValue::String(lua.create_string(&s)?),
        Value::Entity(_) => LuaValue::Nil,
    })
}

fn defs_of(lua: &Lua) -> LuaResult<mlua::AppDataRef<'_, crate::DefsData>> {
    lua.app_data_ref::<crate::DefsData>()
        .ok_or_else(|| mlua::Error::runtime("no data tables here: the host gave scripts none"))
}

/// A frozen row table (`key` and `ref` beside its fields), made once.
fn make_row(
    lua: &Lua,
    prefix_key: &str,
    key: &str,
    fields: Vec<(String, Value)>,
) -> LuaResult<mlua::Table> {
    let t = lua.create_table()?;
    for (k, v) in fields {
        t.set(k, def_value(lua, v)?)?;
    }
    t.set("key", key)?;
    t.set("ref", prefix_key)?;
    t.set_readonly(true);
    Ok(t)
}

fn def_row(lua: &Lua, r: &str) -> LuaResult<LuaValue> {
    let d = defs_of(lua)?;
    if let Some(k) = d.made.borrow().get(r) {
        return lua.registry_value(k);
    }
    let Some(fields) = d.rows.row(r) else {
        return Err(mlua::Error::runtime(format!(
            "no def row {r:?}: a reference is prefix:key (item:bread); world.defs lists the tables"
        )));
    };
    let key = r.split_once(':').map(|(_, k)| k).unwrap_or(r);
    let t = make_row(lua, r, key, fields)?;
    d.made
        .borrow_mut()
        .insert(r.to_string(), lua.create_registry_value(t.clone())?);
    Ok(LuaValue::Table(t))
}

fn def_table(lua: &Lua, name: &str) -> LuaResult<LuaValue> {
    let d = defs_of(lua)?;
    let slot = format!("table:{name}");
    if let Some(k) = d.made.borrow().get(&slot) {
        return lua.registry_value(k);
    }
    let Some((prefix, rows)) = d.rows.table(name) else {
        return Err(mlua::Error::runtime(format!(
            "no def table {name:?}: world.defs lists the tables"
        )));
    };
    let list = lua.create_table()?;
    for (i, (key, fields)) in rows.into_iter().enumerate() {
        let r = format!("{prefix}:{key}");
        list.set(i + 1, make_row(lua, &r, &key, fields)?)?;
    }
    list.set_readonly(true);
    d.made
        .borrow_mut()
        .insert(slot, lua.create_registry_value(list.clone())?);
    Ok(LuaValue::Table(list))
}
