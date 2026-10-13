//! The world read as JSON, for a page script and its headless checks
//! (issue 26: a web build's page could read the world only through
//! `digest_rows()`, the digest checks' Rust debug text, which names
//! entities by number and generation). Entities are spelled as the
//! protocol spells them: `@id` for one with a string id, `e:12` for the
//! rest; values as `world.get` gives them.

use argus_ecs::{ArgusError, ComponentId, Entity, Result, Value, World};
use serde_json::{json, Map, Value as Json};

/// An entity as the protocol spells it; `null` for none.
pub fn entity_ref(world: &World, e: Entity) -> Json {
    if e.is_null() {
        return Json::Null;
    }
    match world.string_id(e) {
        Some(id) => json!(format!("@{id}")),
        None => json!(e.to_string()),
    }
}

/// A field's value as JSON.
pub fn value(world: &World, v: &Value) -> Json {
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

/// A component of an entity, its fields by name.
fn component(world: &World, e: Entity, c: ComponentId) -> Result<Json> {
    let schema = world.registry().schema(c);
    let mut out = Map::new();
    for f in &schema.fields {
        let v = world.get_field(e, &format!("{}.{}", schema.name, f.name))?;
        out.insert(f.name.clone(), value(world, &v));
    }
    Ok(Json::Object(out))
}

/// An entity with its components (`only`: just those named; empty: all),
/// `{entity, components: {Name: {field: value}}}`.
fn entity(world: &World, e: Entity, only: &[ComponentId]) -> Result<Json> {
    let mut comps = Map::new();
    for &c in world.components_of(e)? {
        if !only.is_empty() && !only.contains(&c) {
            continue;
        }
        comps.insert(
            world.registry().schema(c).name.clone(),
            component(world, e, c)?,
        );
    }
    Ok(json!({ "entity": entity_ref(world, e), "components": comps }))
}

/// `player.get(ref, path)`: an entity (`@id`, `e:12`) as
/// `{entity, components}`, one component (`path` "Health"), or one field
/// ("Health.cur").
pub fn get(world: &World, r: &str, path: &str) -> Result<Json> {
    let e = world.entity(r)?;
    if path.is_empty() {
        return entity(world, e, &[]);
    }
    if path.contains('.') {
        return Ok(value(world, &world.get_field(e, path)?));
    }
    let c = world.component_id(path).ok_or_else(|| {
        ArgusError::new(
            argus_ecs::codes::UNKNOWN_COMPONENT,
            format!("there is no component {path:?}"),
        )
    })?;
    component(world, e, c)
}

/// `player.query(components)`: every entity that has all these components
/// (names, comma-separated), oldest first, each with those components.
pub fn query(world: &World, components: &str) -> Result<Json> {
    let mut ids = Vec::new();
    for name in components
        .split(',')
        .map(str::trim)
        .filter(|n| !n.is_empty())
    {
        ids.push(world.component_id(name).ok_or_else(|| {
            ArgusError::new(
                argus_ecs::codes::UNKNOWN_COMPONENT,
                format!("there is no component {name:?}"),
            )
        })?);
    }
    if ids.is_empty() {
        return Err(ArgusError::new(
            argus_ecs::codes::UNKNOWN_COMPONENT,
            "query names its components: player.query(\"Monster,Health\")",
        ));
    }
    let mut q = world.query();
    for &c in &ids {
        q = q.has(c);
    }
    let mut es = q.entities();
    es.sort();
    let rows = es
        .into_iter()
        .map(|e| entity(world, e, &ids))
        .collect::<Result<Vec<Json>>>()?;
    Ok(Json::Array(rows))
}
