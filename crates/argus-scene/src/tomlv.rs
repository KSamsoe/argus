//! TOML parsing helpers and the conversion between `toml_edit` values and
//! [`argus_ecs::Value`]s. Every error carries the `file:line` of the TOML
//! that caused it (§5 "Errors are documentation").

use crate::codes;
use argus_ecs::schema::shortest_f64;
use argus_ecs::{ArgusError, FieldType, Result, Value, Vec2};
use std::path::Path;
use toml_edit::{Array, DocumentMut, ImDocument, Item, TableLike, Value as TValue};

/// Line number (1-based) of a byte offset in `text`.
pub fn line_of(text: &str, offset: usize) -> usize {
    let end = offset.min(text.len());
    text[..end].matches('\n').count() + 1
}

/// `file:line` for an item, if the parser kept its span.
pub fn item_ref(file: &str, text: &str, item: &Item) -> String {
    match item.span() {
        Some(span) => format!("{file}:{}", line_of(text, span.start)),
        None => file.to_string(),
    }
}

/// `file:line` for a value.
pub fn value_ref(file: &str, text: &str, v: &TValue) -> String {
    match v.span() {
        Some(span) => format!("{file}:{}", line_of(text, span.start)),
        None => file.to_string(),
    }
}

/// Parse a TOML document keeping spans. The error is code 4601 with the
/// parser's message and the offending line.
pub fn parse(file: &str, text: &str) -> Result<ImDocument<String>> {
    ImDocument::parse(text.to_string()).map_err(|e| {
        let line = e.span().map(|s| line_of(text, s.start));
        let r = match line {
            Some(l) => format!("{file}:{l}"),
            None => file.to_string(),
        };
        ArgusError::new(
            codes::BAD_TOML,
            format!("{file} is not valid TOML: {}", e.message()),
        )
        .hint("Fix the syntax at the referenced line; TOML needs quoted strings and one key per line.")
        .at(Some(r))
    })
}

/// Parse a TOML document for editing (formatting and comments preserved).
pub fn parse_mut(file: &str, text: &str) -> Result<DocumentMut> {
    text.parse::<DocumentMut>().map_err(|e| {
        let line = e.span().map(|s| line_of(text, s.start));
        let r = match line {
            Some(l) => format!("{file}:{l}"),
            None => file.to_string(),
        };
        ArgusError::new(
            codes::BAD_TOML,
            format!("{file} is not valid TOML: {}", e.message()),
        )
        .at(Some(r))
    })
}

/// Read a file into a string with a 4602 error naming the path.
pub fn read(path: &Path) -> Result<String> {
    argus_os::fs::read_to_string(path).map_err(|e| {
        ArgusError::new(
            codes::MISSING_FILE,
            format!("cannot read {}: {e}", path.display()),
        )
        .hint("Check the path; scene.list shows the scenes the project knows about.")
    })
}

/// Convert a TOML value to the [`Value`] a field of type `ty` stores.
/// Entity fields yield a `Value::Str` placeholder (`@id`, `e:n`, or empty
/// for null) that the scene loader resolves once the entities exist; every
/// other type is fully checked here.
pub fn to_value(v: &TValue, ty: FieldType, at: &str) -> Result<Value> {
    let mismatch = |got: &str| {
        ArgusError::new(
            argus_ecs::codes::TYPE_MISMATCH,
            format!("expected a {} value; got {got}", ty.name()),
        )
        .at(Some(at.to_string()))
    };
    let out = match (ty, v) {
        (FieldType::Bool, TValue::Boolean(b)) => Value::Bool(*b.value()),
        (FieldType::Vec2, TValue::Array(a)) => {
            let xs: Vec<f64> = a
                .iter()
                .map(|x| match x {
                    TValue::Float(f) => Some(*f.value()),
                    TValue::Integer(i) => Some(*i.value() as f64),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| mismatch("an array with a non-number"))?;
            if xs.len() != 2 {
                return Err(mismatch(&format!("an array of {} numbers", xs.len())));
            }
            Value::Vec2(Vec2::new(xs[0] as f32, xs[1] as f32))
        }
        (FieldType::Str, TValue::String(s)) => Value::Str(s.value().clone()),
        (FieldType::Ref, TValue::String(s)) => Value::Ref(s.value().clone()),
        (FieldType::Entity, TValue::String(s)) => Value::Str(s.value().clone()),
        (_, TValue::Integer(i)) if ty.is_numeric() => Value::Int(*i.value()),
        (_, TValue::Float(f)) if ty.is_numeric() => Value::Float(*f.value()),
        (_, other) => return Err(mismatch(kind_name(other))),
    };
    if ty.is_numeric() && ty != FieldType::F32 && ty != FieldType::F64 {
        // Integer fields reject fractions rather than truncating them.
        if let Value::Float(f) = out {
            if f.fract() != 0.0 {
                return Err(mismatch(&format!("the fraction {f}")));
            }
            return Ok(Value::Int(f as i64));
        }
    }
    Ok(out)
}

/// Human name of a TOML value's kind.
pub fn kind_name(v: &TValue) -> &'static str {
    match v {
        TValue::String(_) => "a string",
        TValue::Integer(_) => "an integer",
        TValue::Float(_) => "a float",
        TValue::Boolean(_) => "a boolean",
        TValue::Datetime(_) => "a datetime",
        TValue::Array(_) => "an array",
        TValue::InlineTable(_) => "a table",
    }
}

/// Convert a [`Value`] to a TOML value for writing. Entity values must be
/// given as `@id` strings by the caller (only authored ids belong in files).
pub fn from_value(v: &Value) -> Result<TValue> {
    Ok(match v {
        Value::Bool(b) => TValue::from(*b),
        Value::Int(i) => TValue::from(*i),
        Value::UInt(u) => {
            let i = i64::try_from(*u).map_err(|_| {
                ArgusError::new(
                    argus_ecs::codes::OUT_OF_RANGE,
                    format!("{u} does not fit in a TOML integer"),
                )
            })?;
            TValue::from(i)
        }
        Value::Float(f) => TValue::from(*f),
        // A vector is two f32s: written through their shortest text, so
        // 21.2 is not written as 21.200000762939453 (found when a proposal
        // moved Hollow's gem in the phase 2 gate).
        Value::Vec2(p) => {
            let mut a = Array::new();
            a.push(shortest_f64(p.x));
            a.push(shortest_f64(p.y));
            TValue::Array(a)
        }
        Value::Str(s) | Value::Ref(s) => TValue::from(s.as_str()),
        Value::Entity(e) => {
            if e.is_null() {
                TValue::from("")
            } else {
                return Err(ArgusError::new(
                    codes::NOT_AUTHORED,
                    format!("{e} is a runtime handle and cannot be written to a file"),
                )
                .hint(
                    "Write authored references as \"@id\" strings; runtime entities have no id.",
                ));
            }
        }
    })
}

/// A string array value, for `tags = [...]` style keys.
pub fn string_list(item: &Item, what: &str, at: &str) -> Result<Vec<String>> {
    let arr = item.as_array().ok_or_else(|| {
        ArgusError::new(
            codes::BAD_SCENE,
            format!("{what} must be an array of strings"),
        )
        .at(Some(at.to_string()))
    })?;
    arr.iter()
        .map(|v| {
            v.as_str().map(str::to_string).ok_or_else(|| {
                ArgusError::new(
                    codes::BAD_SCENE,
                    format!("{what} must contain only strings; got {}", kind_name(v)),
                )
                .at(Some(at.to_string()))
            })
        })
        .collect()
}

/// A table-like item (`[header]` or inline), or a shaped error.
pub fn table_like<'a>(item: &'a Item, what: &str, at: &str) -> Result<&'a dyn TableLike> {
    item.as_table_like().ok_or_else(|| {
        ArgusError::new(codes::BAD_SCENE, format!("{what} must be a table"))
            .at(Some(at.to_string()))
    })
}

/// Optional `[x0, y0, x1, y1]` bounds.
pub fn bounds(item: &Item, at: &str) -> Result<[f32; 4]> {
    let arr = item.as_array().ok_or_else(|| {
        ArgusError::new(codes::BAD_SCENE, "bounds must be [x0, y0, x1, y1]")
            .at(Some(at.to_string()))
    })?;
    let xs: Vec<f32> = arr
        .iter()
        .filter_map(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
        .map(|f| f as f32)
        .collect();
    if xs.len() != 4 || arr.len() != 4 {
        return Err(
            ArgusError::new(codes::BAD_SCENE, "bounds must be [x0, y0, x1, y1]")
                .at(Some(at.to_string())),
        );
    }
    Ok([xs[0], xs[1], xs[2], xs[3]])
}
