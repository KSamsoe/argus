//! Save migrations (§6, phase 3 item 0): the script host brings rows saved
//! by older versions of a component up to the live one, by the component's
//! `migrate` declaration (`decl::MigrateDecl`).
//!
//! A row reaches Luau as a table of its fields by the names they were saved
//! with, and comes back as the live version's fields. A step table (rename,
//! default, drop) runs in Rust; a function runs in the VM under the same
//! instruction budget a system has. Entity fields pass through as handles
//! that cannot be used, only kept; a function that needs the world is not
//! a migration.

use crate::bind::{lua_to_value, value_to_lua, Link};
use crate::decl::{MigrateDecl, Step};
use crate::{to_argus, ScriptHost};
use argus_ecs::{ArgusError, ComponentSchema, Migrate, Result, Row, Value};
use mlua::{Function, Lua, RegistryKey, Table, Value as LuaValue};
use std::cell::Cell;
use std::rc::Rc;

impl Migrate for ScriptHost {
    fn declares(&self, component: &str) -> bool {
        self.migrations.contains_key(component)
    }

    fn migrate(&mut self, live: &ComponentSchema, from: u32, row: &mut Row) -> Result<()> {
        let Some(decl) = self.migrations.get(&live.name) else {
            return Ok(());
        };
        let link: Link = Rc::new(Cell::new(std::ptr::null_mut()));
        let limit = self.config.interrupt_budget as i64;
        match decl {
            MigrateDecl::Whole(key) => {
                self.budget.set(limit);
                *row = call(&self.lua, key, row, Some(from), live, &link)?;
            }
            MigrateDecl::Steps(steps) => {
                for (to, step) in steps.iter().filter(|(to, _)| *to > from) {
                    if *to > live.version {
                        break;
                    }
                    match step {
                        Step::Table {
                            rename,
                            default,
                            drop,
                        } => {
                            for (old, new) in rename {
                                if row.iter().any(|(n, _)| n == old) {
                                    row.retain(|(n, _)| n != new);
                                    if let Some(f) = row.iter_mut().find(|(n, _)| n == old) {
                                        f.0 = new.clone();
                                    }
                                }
                            }
                            for (field, v) in default {
                                if !row.iter().any(|(n, _)| n == field) {
                                    row.push((field.clone(), typed(v.clone(), live, field)));
                                }
                            }
                            row.retain(|(n, _)| !drop.contains(n));
                        }
                        Step::Function(key) => {
                            self.budget.set(limit);
                            *row = call(&self.lua, key, row, None, live, &link).map_err(|e| {
                                ArgusError::new(e.code, format!("step {to}: {}", e.message))
                                    .hint(&e.hint)
                                    .at(e.r#ref.clone())
                            })?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// A step table's value as the live field wants it, when there is one: a
/// Lua number for an integer field becomes that integer.
fn typed(v: Value, live: &ComponentSchema, field: &str) -> Value {
    match (live.field(field), &v) {
        (Some(f), Value::Float(n)) if f.ty.is_numeric() && n.fract() == 0.0 => match f.ty.zero() {
            Value::UInt(_) if *n >= 0.0 => Value::UInt(*n as u64),
            Value::Int(_) => Value::Int(*n as i64),
            _ => v,
        },
        _ => v,
    }
}

/// Call a migration function with the row as a table (and the version it
/// was saved at, for a whole-component function); the row it returns (or
/// the table it changed, if it returns nothing), as the live fields.
fn call(
    lua: &Lua,
    key: &RegistryKey,
    row: &Row,
    from: Option<u32>,
    live: &ComponentSchema,
    link: &Link,
) -> Result<Row> {
    let f: Function = lua.registry_value(key).map_err(to_argus)?;
    let t = lua.create_table().map_err(to_argus)?;
    for (n, v) in row {
        t.set(
            n.as_str(),
            value_to_lua(lua, v.clone(), link).map_err(to_argus)?,
        )
        .map_err(to_argus)?;
    }
    let out = match from {
        Some(v) => f.call::<LuaValue>((t.clone(), v)),
        None => f.call::<LuaValue>(t.clone()),
    }
    .map_err(to_argus)?;
    let t = match out {
        LuaValue::Table(t) => t,
        LuaValue::Nil => t,
        other => {
            return Err(ArgusError::new(
                argus_ecs::codes::TYPE_MISMATCH,
                format!("migrate returned a {}, not a row", other.type_name()),
            )
            .hint("Return the row table (or change it in place and return nothing)."))
        }
    };
    back(t, live, link)
}

/// A table back as a row: the live fields first, in schema order, typed as
/// the live component has them; then any other names, sorted, loosely
/// typed (they are left over, and reported).
fn back(t: Table, live: &ComponentSchema, link: &Link) -> Result<Row> {
    let mut row = Row::new();
    for f in &live.fields {
        let v: LuaValue = t.get(f.name.as_str()).map_err(to_argus)?;
        // Not given: the field keeps its default (a null entity, for one).
        if v.is_nil() {
            continue;
        }
        let v = lua_to_value(v, f.ty, link)
            .map_err(to_argus)
            .map_err(|e| ArgusError::new(e.code, format!("{}: {}", f.name, e.message)))?;
        row.push((f.name.clone(), v));
    }
    let mut rest = Vec::new();
    for pair in t.pairs::<String, LuaValue>() {
        let (n, v) = pair.map_err(to_argus)?;
        if live.field(&n).is_none() {
            let v = match v {
                LuaValue::Boolean(b) => Value::Bool(b),
                LuaValue::Integer(i) => Value::Int(i as i64),
                LuaValue::Number(x) => Value::Float(x),
                LuaValue::String(s) => Value::Str(s.to_string_lossy().to_string()),
                _ => Value::Bool(true),
            };
            rest.push((n, v));
        }
    }
    rest.sort_by(|a, b| a.0.cmp(&b.0));
    row.extend(rest);
    Ok(row)
}
