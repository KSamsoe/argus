//! The declaration globals a script sees: `component`, `field`, `system`,
//! `def`. Each call validates its spec and records a pending declaration
//! that the host registers after the chunk finishes (§3, §4, §7).
//!
//! ```luau
//! return component "HitStop" {
//!   doc    = "Freezes an entity's simulation for N frames after a hit.",
//!   frames = field.u8 { default = 0, range = {0, 30}, doc = "Remaining frozen frames" },
//!   scale  = field.f32{ default = 0.0, range = {0, 1}, tune = true, doc = "Time scale while frozen" },
//! }
//! ```

use crate::bind::{argus_err, lua_to_value, script_codes, Link};
use argus_ecs::{ArgusError, ComponentSchema, FieldType, SchemaBuilder, Value};
use mlua::{Function, Lua, RegistryKey, Result as LuaResult, Table, Value as LuaValue};
use std::cell::Cell;
use std::rc::Rc;

/// A `component "Name" { ... }` declaration.
pub struct ComponentDecl {
    /// The schema, with `source` set to `file:line`.
    pub schema: ComponentSchema,
    /// The `migrate` declaration, if any (save migrations, §6).
    pub migrate: Option<MigrateDecl>,
    /// File it came from.
    pub file: String,
}

/// How rows saved by older versions of a component come up to this one
/// (§6 save games; §19: "a declarative rename/default/drop table ... and
/// the function covers the rest. Probably both, table first").
///
/// ```luau
/// return component "Needs" {
///   version = 3,
///   hunger = field.f32 {}, rest = field.f32 {},
///   migrate = {
///     [2] = { rename = { food = "hunger" }, drop = { "mood" } },       -- v1 to v2
///     [3] = function(row) row.rest = 1 - row.hunger; return row end,  -- v2 to v3
///   },
/// }
/// ```
///
/// or one function for any older version: `migrate = function(row, from)
/// ... return row end`.
pub enum MigrateDecl {
    /// One function, called with the row (fields by name) and the version
    /// it was saved at; it returns the row as the live version has it.
    Whole(RegistryKey),
    /// A step per version reached, applied in order from the saved one.
    Steps(Vec<(u32, Step)>),
}

/// One version's step.
pub enum Step {
    /// Data: fields renamed (old, new), given a value when absent, dropped.
    Table {
        /// (old name, new name).
        rename: Vec<(String, String)>,
        /// Values for fields the row does not have.
        default: Vec<(String, Value)>,
        /// Fields removed.
        drop: Vec<String>,
    },
    /// A function of the row, returning it.
    Function(RegistryKey),
}

/// A Lua value as a field value, before the field's type is known.
fn loose_value(v: &LuaValue) -> Option<Value> {
    Some(match v {
        LuaValue::Boolean(b) => Value::Bool(*b),
        LuaValue::Integer(i) => Value::Float(*i as f64),
        LuaValue::Number(n) => Value::Float(*n),
        LuaValue::String(s) => Value::Str(s.to_str().ok()?.to_string()),
        LuaValue::Vector(v) => Value::Vec2(argus_ecs::Vec2::new(v.x(), v.y())),
        _ => return None,
    })
}

/// Parse `migrate = { [v] = step, ... }` for a component at `version`.
fn parse_steps(
    lua: &Lua,
    name: &str,
    version: u32,
    t: Table,
    source: &Option<String>,
) -> LuaResult<Vec<(u32, Step)>> {
    let fail = |why: String| {
        bad(
            format!("component {name}: migrate {why}"),
            "migrate = { [2] = { rename = { old = \"new\" }, default = { field = value }, drop = { \"field\" } }, [3] = function(row) ... return row end }: a step per version from 2 to the component's version.",
            source.clone(),
        )
    };
    let mut steps = Vec::new();
    for pair in t.pairs::<LuaValue, LuaValue>() {
        let (k, v) = pair?;
        let to = match k {
            LuaValue::Integer(i) => i as i64,
            LuaValue::Number(n) if n.fract() == 0.0 => n as i64,
            other => {
                return Err(fail(format!(
                    "has a key {} that is not a version",
                    other.type_name()
                )))
            }
        };
        if to < 2 || to > version as i64 {
            return Err(fail(format!(
                "has a step for version {to}; this is version {version}, so steps are 2 to {version}"
            )));
        }
        let step = match v {
            LuaValue::Function(f) => Step::Function(lua.create_registry_value(f)?),
            LuaValue::Table(st) => {
                let (mut rename, mut default, mut drop) = (Vec::new(), Vec::new(), Vec::new());
                for pair in st.pairs::<String, LuaValue>() {
                    let (key, val) = pair?;
                    let LuaValue::Table(inner) = val else {
                        return Err(fail(format!("[{to}].{key} is not a table")));
                    };
                    match key.as_str() {
                        "rename" => {
                            for p in inner.pairs::<String, String>() {
                                rename.push(p?);
                            }
                        }
                        "default" => {
                            for p in inner.pairs::<String, LuaValue>() {
                                let (f, val) = p?;
                                let v = loose_value(&val).ok_or_else(|| {
                                    fail(format!("[{to}].default.{f} is a {}", val.type_name()))
                                })?;
                                default.push((f, v));
                            }
                        }
                        "drop" => {
                            for p in inner.sequence_values::<String>() {
                                drop.push(p?);
                            }
                        }
                        other => {
                            return Err(fail(format!(
                                "[{to}] has {other:?}; a step is rename, default, and drop"
                            )))
                        }
                    }
                }
                // Pairs come in no order; the file's order does not matter
                // for these, so make it stable.
                rename.sort();
                default.sort_by(|a, b| a.0.cmp(&b.0));
                drop.sort();
                Step::Table {
                    rename,
                    default,
                    drop,
                }
            }
            other => return Err(fail(format!("[{to}] is a {}", other.type_name()))),
        };
        steps.push((to as u32, step));
    }
    steps.sort_by_key(|(v, _)| *v);
    Ok(steps)
}

/// A `system "name" { ... }` declaration, before validation against the world.
pub struct SystemDecl {
    /// System name.
    pub name: String,
    /// Stage name as written.
    pub stage: String,
    /// `after` list.
    pub after: Vec<String>,
    /// `before` list.
    pub before: Vec<String>,
    /// Component names read.
    pub reads: Vec<String>,
    /// Component names written.
    pub writes: Vec<String>,
    /// Prefabs it spawns (`q:spawn_prefab`): each grants the prefab's
    /// components, so the system need not list them in `writes`.
    pub spawns: Vec<String>,
    /// Clock binding as written (`driving`, `continuous`, `discrete`).
    pub clock: Option<String>,
    /// RNG stream.
    pub rng: Option<String>,
    /// Doc string.
    pub doc: String,
    /// `on_load = true`: also run once when the scene loads.
    pub on_load: bool,
    /// `unfrozen = true`: keep running during a hit-stop.
    pub unfrozen: bool,
    /// `between_turns = true`: in a turn-based scene, run on the idle ticks
    /// between turns, and take no time on a turn (2D staples item 5).
    pub between_turns: bool,
    /// The `run` function.
    pub run: RegistryKey,
    /// `file:line`.
    pub source: String,
    /// File it came from.
    pub file: String,
}

/// A `def "table" { ... }` declaration: the schema for a `defs/` data table.
pub struct DefDecl {
    /// Row schema; `name` is the table name.
    pub schema: ComponentSchema,
    /// Field that identifies a row (`item:key.brass` refers to it). Default `id`.
    pub key: String,
    /// Reference prefix (`item` in `item:key.brass`). Defaults to the table name.
    pub prefix: String,
    /// File it came from.
    pub file: String,
}

/// Declarations collected while a chunk runs.
#[derive(Default)]
pub(crate) struct Pending {
    pub components: Vec<ComponentDecl>,
    pub systems: Vec<SystemDecl>,
    pub defs: Vec<DefDecl>,
    pub next_order: u64,
    pub current_file: String,
}

fn bad(msg: impl Into<String>, hint: &str, source: Option<String>) -> mlua::Error {
    argus_err(
        ArgusError::new(script_codes::BAD_DECLARATION, msg)
            .hint(hint)
            .at(source),
    )
}

/// The keys a `system "name" { ... }` declaration takes.
pub(crate) const SYSTEM_KEYS: [&str; 13] = [
    "stage",
    "run",
    "after",
    "before",
    "reads",
    "writes",
    "spawns",
    "clock",
    "rng",
    "doc",
    "on_load",
    "unfrozen",
    "between_turns",
];

fn here(lua: &Lua) -> String {
    let file = lua
        .app_data_ref::<Pending>()
        .map(|p| p.current_file.clone())
        .unwrap_or_default();
    // Level 1 is the Lua caller of the Rust function.
    let line = lua.inspect_stack(1).map(|d| d.curr_line()).unwrap_or(-1);
    if line > 0 {
        format!("{file}:{line}")
    } else {
        file
    }
}

pub(crate) const FIELD_TYPES: [&str; 15] = [
    "bool", "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "f32", "f64", "vec2", "str",
    "entity", "ref",
];

/// `field.list { of = "ref", default = { ... }, doc = ... }` (issue 22): a
/// list of any scalar but an entity.
fn make_list(lua: &Lua, spec: Option<Table>) -> LuaResult<Table> {
    let of: Option<String> = match &spec {
        Some(t) => t.get("of")?,
        None => None,
    };
    let Some(of) = of else {
        return Err(bad(
            "field.list: it needs of, the type of its elements".to_string(),
            "Example: pool = field.list { of = \"ref\", doc = \"...\" }.",
            Some(here(lua)),
        ));
    };
    let Some(list) = FieldType::parse(&of)
        .and_then(argus_ecs::ListOf::of)
        .map(FieldType::List)
    else {
        return Err(bad(
            format!("field.list: of = {of:?} is not a type a list holds"),
            "A list holds bool, u8 to u64, i8 to i64, f32, f64, vec2, str, or ref; not entity.",
            Some(here(lua)),
        ));
    };
    make_field(lua, list.name(), spec)
}

fn make_field(lua: &Lua, ty: &'static str, spec: Option<Table>) -> LuaResult<Table> {
    let list = ty.starts_with("list of ");
    let t = lua.create_table()?;
    t.set("__argus_field", ty)?;
    let order = {
        let mut p = lua.app_data_mut::<Pending>().expect("pending app data");
        p.next_order += 1;
        p.next_order
    };
    t.set("__order", order)?;
    if let Some(spec) = spec {
        for pair in spec.pairs::<String, LuaValue>() {
            let (k, v) = pair?;
            match k.as_str() {
                "of" if list => {}
                "range" | "tune" if list => {
                    return Err(bad(
                        format!("field.list: a list takes no {k}"),
                        "A list's attributes are of, default, and doc.",
                        Some(here(lua)),
                    ))
                }
                "default" | "range" | "doc" | "tune" => t.set(k, v)?,
                other => {
                    return Err(bad(
                        format!("field.{ty}: unknown attribute {other:?}"),
                        "Field attributes are default, range, doc, tune.",
                        Some(here(lua)),
                    ))
                }
            }
        }
    }
    Ok(t)
}

struct ParsedField {
    order: u64,
    name: String,
    ty: FieldType,
    default: Option<Value>,
    range: Option<(f64, f64)>,
    doc: String,
    tune: bool,
}

fn parse_field(owner: &str, name: &str, v: LuaValue, source: &str) -> LuaResult<ParsedField> {
    let LuaValue::Table(t) = v else {
        return Err(bad(
            format!(
                "{owner}.{name}: expected a field.<type> {{ ... }} declaration; got {}",
                v.type_name()
            ),
            "Fields look like frames = field.u8 { default = 0, doc = \"...\" }.",
            Some(source.to_string()),
        ));
    };
    let ty: Option<String> = t.get("__argus_field")?;
    let Some(ty) = ty.and_then(|s| FieldType::parse(&s)) else {
        return Err(bad(
            format!("{owner}.{name}: expected a field.<type> {{ ... }} declaration"),
            "Fields look like frames = field.u8 { default = 0, doc = \"...\" }.",
            Some(source.to_string()),
        ));
    };
    let order: u64 = t.get("__order")?;
    let link: Link = Rc::new(Cell::new(std::ptr::null_mut()));
    let default = match t.get::<LuaValue>("default")? {
        LuaValue::Nil => None,
        v => Some(lua_to_value(v, ty, &link).map_err(|e| {
            bad(
                format!("{owner}.{name}: default: {e}"),
                "The default must match the field type.",
                Some(source.to_string()),
            )
        })?),
    };
    let range = match t.get::<LuaValue>("range")? {
        LuaValue::Nil => None,
        LuaValue::Table(r) => {
            let lo: Option<f64> = r.get(1)?;
            let hi: Option<f64> = r.get(2)?;
            match (lo, hi) {
                (Some(lo), Some(hi)) => Some((lo, hi)),
                _ => {
                    return Err(bad(
                        format!("{owner}.{name}: range must be {{lo, hi}}"),
                        "Example: range = {0, 30}.",
                        Some(source.to_string()),
                    ))
                }
            }
        }
        other => {
            return Err(bad(
                format!(
                    "{owner}.{name}: range must be a table; got {}",
                    other.type_name()
                ),
                "Example: range = {0, 30}.",
                Some(source.to_string()),
            ))
        }
    };
    let doc: Option<String> = t.get("doc")?;
    let tune: Option<bool> = t.get("tune")?;
    Ok(ParsedField {
        order,
        name: name.to_string(),
        ty,
        default,
        range,
        doc: doc.unwrap_or_default(),
        tune: tune.unwrap_or(false),
    })
}

fn build_schema(
    name: &str,
    fields: Vec<ParsedField>,
    doc: String,
    version: u32,
    source: &str,
) -> LuaResult<ComponentSchema> {
    let mut fields = fields;
    fields.sort_by_key(|f| f.order);
    let mut b = SchemaBuilder::new(name)
        .doc(doc)
        .version(version)
        .source(source);
    for f in fields {
        let default = f.default.unwrap_or_else(|| f.ty.zero());
        b = b.field_full(f.name, f.ty, default, f.range, f.doc, f.tune);
    }
    b.build().map_err(argus_err)
}

fn string_list(owner: &str, key: &str, v: LuaValue, source: &str) -> LuaResult<Vec<String>> {
    match v {
        LuaValue::Nil => Ok(Vec::new()),
        LuaValue::String(s) => Ok(vec![s.to_str()?.to_string()]),
        LuaValue::Table(t) => {
            let mut out = Vec::new();
            for item in t.sequence_values::<String>() {
                out.push(item?);
            }
            Ok(out)
        }
        other => Err(bad(
            format!(
                "{owner}: {key} must be a list of strings; got {}",
                other.type_name()
            ),
            "Example: reads = { \"HitStop\", \"TimeScale\" }.",
            Some(source.to_string()),
        )),
    }
}

/// Install `component`, `field`, `system`, `def` as globals. Call before
/// `sandbox(true)`.
pub(crate) fn install_globals(lua: &Lua) -> LuaResult<()> {
    let g = lua.globals();

    let field = lua.create_table()?;
    for ty in FIELD_TYPES {
        field.set(
            ty,
            lua.create_function(move |lua, spec: Option<Table>| make_field(lua, ty, spec))?,
        )?;
    }
    field.set("list", lua.create_function(make_list)?)?;
    g.set("field", field)?;

    g.set(
        "component",
        lua.create_function(|lua, name: String| {
            let source = here(lua);
            lua.create_function(move |lua, spec: Table| {
                let mut fields = Vec::new();
                let mut doc = String::new();
                let mut version = 1u32;
                let mut migrate_fn = None;
                let mut migrate_steps = None;
                for pair in spec.pairs::<LuaValue, LuaValue>() {
                    let (k, v) = pair?;
                    let LuaValue::String(k) = k else {
                        return Err(bad(
                            format!(
                                "component {name}: keys must be field names; got {}",
                                k.type_name()
                            ),
                            "Use name = field.<type> { ... } entries.",
                            Some(source.clone()),
                        ));
                    };
                    let k = k.to_str()?.to_string();
                    match (k.as_str(), v) {
                        ("doc", LuaValue::String(s)) => doc = s.to_str()?.to_string(),
                        ("version", LuaValue::Integer(i)) => version = i as u32,
                        ("version", LuaValue::Number(n)) => version = n as u32,
                        ("migrate", LuaValue::Function(f)) => {
                            migrate_fn = Some(lua.create_registry_value(f)?)
                        }
                        ("migrate", LuaValue::Table(t)) => migrate_steps = Some(t),
                        ("doc" | "version" | "migrate", other) => {
                            return Err(bad(
                                format!(
                                    "component {name}: {k} has the wrong type ({})",
                                    other.type_name()
                                ),
                                "doc is a string, version a number, migrate a function or a table of steps by version.",
                                Some(source.clone()),
                            ))
                        }
                        (_, v) => fields.push(parse_field(&name, &k, v, &source)?),
                    }
                }
                let migrate = match (migrate_fn, migrate_steps) {
                    (Some(f), _) => Some(MigrateDecl::Whole(f)),
                    (None, Some(t)) => Some(MigrateDecl::Steps(parse_steps(
                        lua,
                        &name,
                        version,
                        t,
                        &Some(source.clone()),
                    )?)),
                    (None, None) => None,
                };
                let schema = build_schema(&name, fields, doc, version, &source)?;
                let file = lua
                    .app_data_ref::<Pending>()
                    .map(|p| p.current_file.clone())
                    .unwrap_or_default();
                lua.app_data_mut::<Pending>()
                    .expect("pending")
                    .components
                    .push(ComponentDecl {
                        schema,
                        migrate,
                        file,
                    });
                Ok(spec)
            })
        })?,
    )?;

    g.set(
        "def",
        lua.create_function(|lua, name: String| {
            let source = here(lua);
            lua.create_function(move |lua, spec: Table| {
                let mut fields = Vec::new();
                let mut doc = String::new();
                let mut key = "id".to_string();
                let mut prefix = name.clone();
                for pair in spec.pairs::<String, LuaValue>() {
                    let (k, v) = pair?;
                    match (k.as_str(), v) {
                        ("doc", LuaValue::String(s)) => doc = s.to_str()?.to_string(),
                        ("key", LuaValue::String(s)) => key = s.to_str()?.to_string(),
                        ("prefix", LuaValue::String(s)) => prefix = s.to_str()?.to_string(),
                        (_, v) => fields.push(parse_field(&name, &k, v, &source)?),
                    }
                }
                let schema = build_schema(&name, fields, doc, 1, &source)?;
                if schema.field(&key).is_none() && key != "id" {
                    return Err(bad(
                        format!("def {name}: key field {key:?} is not declared"),
                        "Declare the key as a str field, or omit key to use id.",
                        Some(source.clone()),
                    ));
                }
                let file = lua
                    .app_data_ref::<Pending>()
                    .map(|p| p.current_file.clone())
                    .unwrap_or_default();
                lua.app_data_mut::<Pending>()
                    .expect("pending")
                    .defs
                    .push(DefDecl {
                        schema,
                        key,
                        prefix,
                        file,
                    });
                Ok(spec)
            })
        })?,
    )?;

    g.set(
        "system",
        lua.create_function(|lua, name: String| {
            let source = here(lua);
            lua.create_function(move |lua, spec: Table| {
                // A key the declaration does not know is a mistake, not a
                // no-op (found at the phase 5 gate: `before = {...}` and a
                // misspelt key loaded silently, the system wherever the
                // scheduler put it).
                for pair in spec.pairs::<LuaValue, LuaValue>() {
                    let (k, _) = pair?;
                    let key = match &k {
                        LuaValue::String(s) => s.to_str()?.to_string(),
                        other => format!("{other:?}"),
                    };
                    if !SYSTEM_KEYS.contains(&key.as_str()) {
                        let hint = format!("A system takes {}.", SYSTEM_KEYS.join(", "));
                        return Err(bad(
                            format!("system {name}: unknown key {key}"),
                            &hint,
                            Some(source.clone()),
                        ));
                    }
                }
                let stage: Option<String> = spec.get("stage")?;
                let Some(stage) = stage else {
                    return Err(bad(
                        format!("system {name}: stage is required"),
                        "One of input, sim, late, hierarchy, present.",
                        Some(source.clone()),
                    ));
                };
                let run: Option<Function> = spec.get("run")?;
                let Some(run) = run else {
                    return Err(bad(
                        format!("system {name}: run function is required"),
                        "run = function(q) ... end",
                        Some(source.clone()),
                    ));
                };
                let owner = format!("system {name}");
                let decl = SystemDecl {
                    name: name.clone(),
                    stage,
                    after: string_list(&owner, "after", spec.get("after")?, &source)?,
                    before: string_list(&owner, "before", spec.get("before")?, &source)?,
                    reads: string_list(&owner, "reads", spec.get("reads")?, &source)?,
                    writes: string_list(&owner, "writes", spec.get("writes")?, &source)?,
                    spawns: string_list(&owner, "spawns", spec.get("spawns")?, &source)?,
                    clock: spec.get("clock")?,
                    rng: spec.get("rng")?,
                    doc: spec.get::<Option<String>>("doc")?.unwrap_or_default(),
                    on_load: spec.get::<Option<bool>>("on_load")?.unwrap_or(false),
                    unfrozen: spec.get::<Option<bool>>("unfrozen")?.unwrap_or(false),
                    between_turns: spec.get::<Option<bool>>("between_turns")?.unwrap_or(false),
                    run: lua.create_registry_value(run)?,
                    source: source.clone(),
                    file: lua
                        .app_data_ref::<Pending>()
                        .map(|p| p.current_file.clone())
                        .unwrap_or_default(),
                };
                lua.app_data_mut::<Pending>()
                    .expect("pending")
                    .systems
                    .push(decl);
                Ok(spec)
            })
        })?,
    )?;

    Ok(())
}
