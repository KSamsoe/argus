//! Conversions between engine values and JSON, and parameter helpers.
//! Entities cross the boundary as `"@id"` for authored entities and
//! `"e:4011"` for runtime ones (§3, §5); null is JSON `null`.

use crate::codes;
use argus_ecs::{ArgusError, Entity, FieldType, Result, Value, Vec2, World};
use serde_json::{json, Map, Value as Json};

/// Protocol spelling of an entity handle.
pub fn entity_ref(world: &World, e: Entity) -> Json {
    if e.is_null() {
        return Json::Null;
    }
    match world.string_id(e) {
        Some(id) => Json::String(format!("@{id}")),
        None => Json::String(e.to_string()),
    }
}

/// Protocol spelling as a string (never null; a null entity is `""`).
pub fn entity_str(world: &World, e: Entity) -> String {
    match entity_ref(world, e) {
        Json::String(s) => s,
        _ => String::new(),
    }
}

/// Engine value to JSON.
pub fn to_json(world: &World, v: &Value) -> Json {
    match v {
        Value::Bool(b) => json!(b),
        Value::Int(i) => json!(i),
        Value::UInt(u) => json!(u),
        Value::Float(f) => json!(f),
        Value::Vec2(p) => json!([p.x, p.y]),
        Value::Str(s) | Value::Ref(s) => json!(s),
        Value::Entity(e) => entity_ref(world, *e),
    }
}

/// JSON to an engine value for a field of type `ty`. Entity strings are
/// resolved against the world; `null` and `""` are the null entity.
pub fn from_json(world: &World, j: &Json, ty: FieldType) -> Result<Value> {
    let mismatch = |got: &str| {
        ArgusError::new(
            argus_ecs::codes::TYPE_MISMATCH,
            format!("expected a {} value; got {got}", ty.name()),
        )
    };
    Ok(match (ty, j) {
        (FieldType::Bool, Json::Bool(b)) => Value::Bool(*b),
        (FieldType::Vec2, Json::Array(a)) if a.len() == 2 => {
            let x = a[0].as_f64().ok_or_else(|| mismatch("a non-number"))?;
            let y = a[1].as_f64().ok_or_else(|| mismatch("a non-number"))?;
            Value::Vec2(Vec2::new(x as f32, y as f32))
        }
        (FieldType::Str, Json::String(s)) => Value::Str(s.clone()),
        (FieldType::Ref, Json::String(s)) => Value::Ref(s.clone()),
        (FieldType::Ref, Json::Null) => Value::Ref(String::new()),
        (FieldType::Entity, Json::Null) => Value::Entity(Entity::NULL),
        (FieldType::Entity, Json::String(s)) if s.is_empty() || s == "e:null" => {
            Value::Entity(Entity::NULL)
        }
        (FieldType::Entity, Json::String(s)) => Value::Entity(world.entity(s)?),
        (_, Json::Number(n)) if ty.is_numeric() => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(u) = n.as_u64() {
                Value::UInt(u)
            } else {
                Value::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        (_, other) => return Err(mismatch(kind(other))),
    })
}

/// Human name of a JSON value's kind.
pub fn kind(j: &Json) -> &'static str {
    match j {
        Json::Null => "null",
        Json::Bool(_) => "a boolean",
        Json::Number(_) => "a number",
        Json::String(_) => "a string",
        Json::Array(_) => "an array",
        Json::Object(_) => "an object",
    }
}

/// The JSON-RPC error object for an engine error: the code and message
/// unchanged, hint and ref in `data` (§5 "Errors are documentation").
pub fn error_json(e: &ArgusError) -> Json {
    let mut data = Map::new();
    if !e.hint.is_empty() {
        data.insert("hint".into(), json!(e.hint));
    }
    if let Some(r) = &e.r#ref {
        data.insert("ref".into(), json!(r));
    }
    json!({ "code": e.code, "message": e.message, "data": data })
}

/// A parameter error.
pub fn bad_params(msg: impl Into<String>) -> ArgusError {
    ArgusError::new(codes::BAD_PARAMS, msg)
        .hint("session.docs {method} shows the parameters each method takes.")
}

/// Required string parameter.
pub fn str_param<'a>(params: &'a Json, key: &str) -> Result<&'a str> {
    params
        .get(key)
        .and_then(Json::as_str)
        .ok_or_else(|| bad_params(format!("{key} must be a string")))
}

/// Optional string parameter.
pub fn opt_str<'a>(params: &'a Json, key: &str) -> Result<Option<&'a str>> {
    match params.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(bad_params(format!("{key} must be a string"))),
    }
}

/// Optional unsigned integer with a default.
pub fn u64_param(params: &Json, key: &str, default: u64) -> Result<u64> {
    match params.get(key) {
        None | Some(Json::Null) => Ok(default),
        Some(Json::Number(n)) => n
            .as_u64()
            .ok_or_else(|| bad_params(format!("{key} must be a non-negative integer"))),
        Some(_) => Err(bad_params(format!("{key} must be a non-negative integer"))),
    }
}

/// Optional float with a default.
pub fn f64_param(params: &Json, key: &str, default: f64) -> Result<f64> {
    match params.get(key) {
        None | Some(Json::Null) => Ok(default),
        Some(Json::Number(n)) => Ok(n.as_f64().unwrap_or(default)),
        Some(_) => Err(bad_params(format!("{key} must be a number"))),
    }
}

/// Optional boolean with a default.
pub fn bool_param(params: &Json, key: &str, default: bool) -> Result<bool> {
    match params.get(key) {
        None | Some(Json::Null) => Ok(default),
        Some(Json::Bool(b)) => Ok(*b),
        Some(_) => Err(bad_params(format!("{key} must be true or false"))),
    }
}

/// Optional list of strings (missing means empty).
pub fn str_list(params: &Json, key: &str) -> Result<Vec<String>> {
    match params.get(key) {
        None | Some(Json::Null) => Ok(Vec::new()),
        Some(Json::Array(a)) => a
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| bad_params(format!("{key} must be an array of strings")))
            })
            .collect(),
        Some(Json::String(s)) => Ok(vec![s.clone()]),
        Some(_) => Err(bad_params(format!("{key} must be an array of strings"))),
    }
}

/// A `toml_edit` value as JSON (for `world.get {resolved: false}`).
pub fn toml_to_json(v: &toml_edit::Value) -> Json {
    match v {
        toml_edit::Value::String(s) => json!(s.value()),
        toml_edit::Value::Integer(i) => json!(i.value()),
        toml_edit::Value::Float(f) => json!(f.value()),
        toml_edit::Value::Boolean(b) => json!(b.value()),
        toml_edit::Value::Datetime(d) => json!(d.value().to_string()),
        toml_edit::Value::Array(a) => Json::Array(a.iter().map(toml_to_json).collect()),
        toml_edit::Value::InlineTable(t) => Json::Object(
            t.iter()
                .map(|(k, v)| (k.to_string(), toml_to_json(v)))
                .collect(),
        ),
    }
}
