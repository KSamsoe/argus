//! Result shapes shared by the live world and the index (§4 "identical
//! result shapes"): `world.get`'s `resolved` and `overrides` blocks and
//! the `world.summary` object.

use argus_ecs::Value;
use argus_scene::{EntityDoc, ResolvedEntity};
use serde_json::{json, Map, Value as Json};

/// An engine value as JSON without a world: entity placeholders stay as
/// their strings (`"@id"`, or `""` for null), which is how files spell them.
pub fn value_json(v: &Value) -> Json {
    match v {
        Value::Bool(b) => json!(b),
        Value::Int(i) => json!(i),
        Value::UInt(u) => json!(u),
        Value::Float(x) => json!(x),
        Value::Vec2(p) => json!([p.x, p.y]),
        Value::Str(s) | Value::Ref(s) => json!(s),
        Value::Entity(e) => {
            if e.is_null() {
                Json::Null
            } else {
                json!(e.to_string())
            }
        }
    }
}

/// A `toml_edit` value as JSON.
pub fn toml_json(v: &toml_edit::Value) -> Json {
    match v {
        toml_edit::Value::String(s) => json!(s.value()),
        toml_edit::Value::Integer(i) => json!(i.value()),
        toml_edit::Value::Float(f) => json!(f.value()),
        toml_edit::Value::Boolean(b) => json!(b.value()),
        toml_edit::Value::Datetime(d) => json!(d.value().to_string()),
        toml_edit::Value::Array(a) => Json::Array(a.iter().map(toml_json).collect()),
        toml_edit::Value::InlineTable(t) => Json::Object(
            t.iter()
                .map(|(k, v)| (k.to_string(), toml_json(v)))
                .collect(),
        ),
    }
}

/// The `resolved` block of `world.get {resolved: true}`.
pub fn resolved_json(re: &ResolvedEntity) -> Json {
    let comps: Map<String, Json> = re
        .components
        .iter()
        .map(|c| {
            let fields: Map<String, Json> = c
                .fields
                .iter()
                .map(|f| {
                    (
                        f.name.clone(),
                        json!({ "value": value_json(&f.value), "from": f.from.to_string(), "at": f.at }),
                    )
                })
                .collect();
            (c.name.clone(), json!({ "from": c.from.to_string(), "fields": fields }))
        })
        .collect();
    json!({
        "id": re.id,
        "prefab": re.prefab,
        "chain": re.chain,
        "tags": re.tags.iter().map(|(t, p)| json!([t, p.to_string()])).collect::<Vec<_>>(),
        "components": comps,
        "children": re.children.iter().map(|c| c.id.clone()).collect::<Vec<_>>(),
        "file": re.file,
    })
}

/// The `overrides` block of `world.get {resolved: false}`.
pub fn overrides_json(doc: &EntityDoc) -> Json {
    let comps: Map<String, Json> = doc
        .components
        .iter()
        .map(|c| {
            (
                c.name.clone(),
                Json::Object(
                    c.fields
                        .iter()
                        .map(|f| (f.name.clone(), toml_json(&f.raw)))
                        .collect(),
                ),
            )
        })
        .collect();
    json!({
        "prefab": doc.prefab,
        "tags": doc.tags,
        "removed_tags": doc.removed_tags,
        "components": comps,
        "removed": doc.removed,
        "children": doc.children.iter().map(|(n, _)| n).collect::<Vec<_>>(),
        "removed_children": doc.removed_children,
        "at": doc.at,
    })
}

/// Everything `world.summary` says, gathered by either source.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    /// World frame (0 from the index).
    pub frame: u64,
    /// Driving-clock tick.
    pub tick: u64,
    /// Sim time.
    pub time: f64,
    /// Run seed.
    pub seed: u64,
    /// Clock spec.
    pub clock: String,
    /// Playing?
    pub playing: bool,
    /// Paused?
    pub paused: bool,
    /// Scene name.
    pub scene: Option<String>,
    /// Rooms loaded (live) or present (index).
    pub rooms: Vec<String>,
    /// Entity count.
    pub entities: usize,
    /// Authored entity count.
    pub authored: usize,
    /// Registered components.
    pub components: usize,
    /// Archetypes: component names and entity count, largest first.
    pub archetypes: Vec<(Vec<String>, usize)>,
    /// Named entities (first 50) with their components.
    pub named: Vec<(String, Vec<String>)>,
    /// Total named entities.
    pub named_total: usize,
    /// Enabled systems.
    pub systems: Vec<String>,
    /// Disabled systems.
    pub disabled: Vec<String>,
    /// Events not yet drained.
    pub pending_events: usize,
    /// `live` or `index`.
    pub source: &'static str,
}

impl Summary {
    /// The result object. Archetypes are ordered largest first, then by
    /// component names, so the live world and the index agree on ties.
    pub fn to_json(&self) -> Json {
        let mut archetypes = self.archetypes.clone();
        archetypes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        json!({
            "frame": self.frame,
            "tick": self.tick,
            "time": self.time,
            "seed": self.seed,
            "clock": self.clock,
            "playing": self.playing,
            "paused": self.paused,
            "scene": self.scene,
            "rooms": self.rooms,
            "entities": self.entities,
            "authored": self.authored,
            "components": self.components,
            "archetypes": archetypes.iter().map(|(c, n)| json!([c, n])).collect::<Vec<_>>(),
            "named": self.named.iter().map(|(id, c)| json!([format!("@{id}"), c])).collect::<Vec<_>>(),
            "named_total": self.named_total,
            "systems": self.systems,
            "disabled": self.disabled,
            "pending_events": self.pending_events,
            "source": self.source,
        })
    }
}
