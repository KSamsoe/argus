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
    /// The `migrate` function, if declared (used by save migrations, phase 3).
    pub migrate: Option<RegistryKey>,
    /// File it came from.
    pub file: String,
}

/// A `system "name" { ... }` declaration, before validation against the world.
pub struct SystemDecl {
    /// System name.
    pub name: String,
    /// Stage name as written.
    pub stage: String,
    /// `after` list.
    pub after: Vec<String>,
    /// Component names read.
    pub reads: Vec<String>,
    /// Component names written.
    pub writes: Vec<String>,
    /// Clock binding as written (`driving`, `continuous`, `discrete`).
    pub clock: Option<String>,
    /// RNG stream.
    pub rng: Option<String>,
    /// Doc string.
    pub doc: String,
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

const FIELD_TYPES: [&str; 15] = [
    "bool", "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "f32", "f64", "vec2", "str",
    "entity", "ref",
];

fn make_field(lua: &Lua, ty: &'static str, spec: Option<Table>) -> LuaResult<Table> {
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
    g.set("field", field)?;

    g.set(
        "component",
        lua.create_function(|lua, name: String| {
            let source = here(lua);
            lua.create_function(move |lua, spec: Table| {
                let mut fields = Vec::new();
                let mut doc = String::new();
                let mut version = 1u32;
                let mut migrate = None;
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
                            migrate = Some(lua.create_registry_value(f)?)
                        }
                        ("doc" | "version" | "migrate", other) => {
                            return Err(bad(
                                format!(
                                    "component {name}: {k} has the wrong type ({})",
                                    other.type_name()
                                ),
                                "doc is a string, version a number, migrate a function.",
                                Some(source.clone()),
                            ))
                        }
                        (_, v) => fields.push(parse_field(&name, &k, v, &source)?),
                    }
                }
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
                for pair in spec.pairs::<String, LuaValue>() {
                    let (k, v) = pair?;
                    match (k.as_str(), v) {
                        ("doc", LuaValue::String(s)) => doc = s.to_str()?.to_string(),
                        ("key", LuaValue::String(s)) => key = s.to_str()?.to_string(),
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
                    .push(DefDecl { schema, key, file });
                Ok(spec)
            })
        })?,
    )?;

    g.set(
        "system",
        lua.create_function(|lua, name: String| {
            let source = here(lua);
            lua.create_function(move |lua, spec: Table| {
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
                    reads: string_list(&owner, "reads", spec.get("reads")?, &source)?,
                    writes: string_list(&owner, "writes", spec.get("writes")?, &source)?,
                    clock: spec.get("clock")?,
                    rng: spec.get("rng")?,
                    doc: spec.get::<Option<String>>("doc")?.unwrap_or_default(),
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
