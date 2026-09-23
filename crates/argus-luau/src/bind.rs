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
        m.add_method("rng", |lua, this, ()| {
            lua.create_userdata(RngRef {
                link: this.link.clone(),
            })
        });
    }
}
