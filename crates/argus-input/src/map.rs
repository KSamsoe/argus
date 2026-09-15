//! `input.toml`: the action map (§12).
//!
//! ```toml
//! [actions.move]
//! type = "axis2"                 # button | axis | axis2
//! doc  = "Walk direction"
//! [actions.move.bindings]
//! keyboard = ["wasd"]            # per device; phase 0 has no devices, so
//! gamepad  = ["left_stick"]      # bindings are kept and reported, not read
//!
//! [actions.fire]
//! type = "button"
//! [actions.fire.bindings]
//! keyboard = ["space"]
//! gamepad  = ["south"]
//!
//! [actions.crouch]
//! type   = "button"
//! toggle = true                  # accessibility: a press toggles instead of holding
//!
//! [slots.1]
//! devices = ["keyboard", "gamepad:0"]
//! [slots.2]
//! devices = ["gamepad:1"]
//! ```

use crate::codes;
use argus_ecs::{ArgusError, ComponentSchema, FieldType, Result, SchemaBuilder, Value};
use argus_scene::tomlv;
use serde_json::{json, Value as Json};
use std::collections::BTreeMap;
use std::path::Path;

/// What kind of value an action carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    /// Held or not.
    Button,
    /// One axis in `[-1, 1]`.
    Axis,
    /// Two axes in `[-1, 1]` each.
    Axis2,
}

impl ActionKind {
    /// Map spelling.
    pub fn name(self) -> &'static str {
        match self {
            ActionKind::Button => "button",
            ActionKind::Axis => "axis",
            ActionKind::Axis2 => "axis2",
        }
    }

    fn parse(s: &str) -> Option<ActionKind> {
        Some(match s {
            "button" => ActionKind::Button,
            "axis" => ActionKind::Axis,
            "axis2" => ActionKind::Axis2,
            _ => return None,
        })
    }
}

/// One declared action.
#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    /// Name; also the `Actions` field name.
    pub name: String,
    /// Kind.
    pub kind: ActionKind,
    /// What it means.
    pub doc: String,
    /// Bindings per device (`keyboard`, `gamepad`, `mouse`).
    pub bindings: BTreeMap<String, Vec<String>>,
    /// A press toggles the held state instead of holding (buttons only).
    pub toggle: bool,
    /// `file:line`.
    pub at: String,
}

/// One player slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    /// 1-based slot number.
    pub index: u8,
    /// Devices feeding it (`keyboard`, `gamepad:0`).
    pub devices: Vec<String>,
}

/// The parsed map.
#[derive(Clone, Debug, PartialEq)]
pub struct ActionMap {
    /// Actions, sorted by name.
    pub actions: Vec<Action>,
    /// Slots, sorted; at least one.
    pub slots: Vec<Slot>,
    /// Project-relative file.
    pub file: String,
}

const DEVICES: &[&str] = &["keyboard", "mouse", "gamepad"];

impl ActionMap {
    /// Read `input.toml` under `root`; `None` when the project has none.
    pub fn load(root: &Path) -> Result<Option<ActionMap>> {
        let p = root.join("input.toml");
        if !p.is_file() {
            return Ok(None);
        }
        let text = tomlv::read(&p)?;
        Self::parse("input.toml", &text).map(Some)
    }

    /// Parse a map from text.
    pub fn parse(file: &str, text: &str) -> Result<ActionMap> {
        let im = tomlv::parse(file, text)?;
        let t = im.as_table();
        let bad = |msg: String, at: String| {
            ArgusError::new(codes::BAD_MAP, msg)
                .hint("input.toml has [actions.<name>] tables with type = \"button\" | \"axis\" | \"axis2\", optional doc, toggle, and [actions.<name>.bindings] per device; [slots.<n>] tables list devices.")
                .at(Some(at))
        };
        let mut actions = Vec::new();
        if let Some(a) = t.get("actions") {
            let at = tomlv::item_ref(file, text, a);
            let at_ = tomlv::table_like(a, "actions", &at)?;
            for (name, item) in at_.iter() {
                let aat = tomlv::item_ref(file, text, item);
                if name.is_empty()
                    || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    || name.starts_with(|c: char| c.is_ascii_digit())
                {
                    return Err(bad(
                        format!("action {name:?} must be letters, digits, and underscores, and not start with a digit (it becomes a field of Actions)"),
                        aat,
                    ));
                }
                let it = tomlv::table_like(item, &format!("actions.{name}"), &aat)?;
                let kind = match it.get("type").and_then(|i| i.as_str()) {
                    Some(k) => ActionKind::parse(k).ok_or_else(|| {
                        bad(
                            format!("actions.{name}.type {k:?} is not button, axis, or axis2"),
                            aat.clone(),
                        )
                    })?,
                    None => ActionKind::Button,
                };
                let doc = it
                    .get("doc")
                    .and_then(|i| i.as_str())
                    .unwrap_or("")
                    .to_string();
                let toggle = it.get("toggle").and_then(|i| i.as_bool()).unwrap_or(false);
                if toggle && kind != ActionKind::Button {
                    return Err(bad(
                        format!("actions.{name}: toggle applies to buttons only"),
                        aat,
                    ));
                }
                let mut bindings = BTreeMap::new();
                if let Some(b) = it.get("bindings") {
                    let bt = tomlv::table_like(b, &format!("actions.{name}.bindings"), &aat)?;
                    for (dev, list) in bt.iter() {
                        if !DEVICES.contains(&dev) {
                            return Err(bad(
                                format!("actions.{name}.bindings: unknown device {dev:?}; devices are {}", DEVICES.join(", ")),
                                tomlv::item_ref(file, text, list),
                            ));
                        }
                        let items = tomlv::string_list(
                            list,
                            &format!("actions.{name}.bindings.{dev}"),
                            &aat,
                        )?;
                        bindings.insert(dev.to_string(), items);
                    }
                }
                actions.push(Action {
                    name: name.to_string(),
                    kind,
                    doc,
                    bindings,
                    toggle,
                    at: aat,
                });
            }
        }
        actions.sort_by(|a, b| a.name.cmp(&b.name));
        let mut slots = Vec::new();
        if let Some(s) = t.get("slots") {
            let sat = tomlv::item_ref(file, text, s);
            let st = tomlv::table_like(s, "slots", &sat)?;
            for (n, item) in st.iter() {
                let iat = tomlv::item_ref(file, text, item);
                let index: u8 =
                    n.parse()
                        .ok()
                        .filter(|i| (1..=8).contains(i))
                        .ok_or_else(|| {
                            bad(format!("slots.{n}: slot numbers are 1 to 8"), iat.clone())
                        })?;
                let it = tomlv::table_like(item, &format!("slots.{n}"), &iat)?;
                let devices = match it.get("devices") {
                    Some(d) => tomlv::string_list(d, &format!("slots.{n}.devices"), &iat)?,
                    None => Vec::new(),
                };
                slots.push(Slot { index, devices });
            }
        }
        if slots.is_empty() {
            slots.push(Slot {
                index: 1,
                devices: vec!["keyboard".to_string()],
            });
        }
        slots.sort_by_key(|s| s.index);
        Ok(ActionMap {
            actions,
            slots,
            file: file.to_string(),
        })
    }

    /// An action by name.
    pub fn action(&self, name: &str) -> Result<&Action> {
        self.actions.iter().find(|a| a.name == name).ok_or_else(|| {
            ArgusError::new(codes::UNKNOWN_ACTION, format!("no action named {name:?}")).hint(
                format!(
                    "Actions in {}: {}",
                    self.file,
                    self.actions
                        .iter()
                        .map(|a| a.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )
        })
    }

    /// A slot by number.
    pub fn slot(&self, index: u8) -> Result<&Slot> {
        self.slots.iter().find(|s| s.index == index).ok_or_else(|| {
            ArgusError::new(codes::BAD_SLOT, format!("no player slot {index}")).hint(format!(
                "Slots in {}: {}",
                self.file,
                self.slots
                    .iter()
                    .map(|s| s.index.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })
    }

    /// The `Actions` component schema: one field per action, plus
    /// `<name>_pressed` and `<name>_released` for buttons.
    pub fn schema(&self) -> Result<ComponentSchema> {
        let mut b = SchemaBuilder::new(crate::ACTIONS)
            .doc("This tick's actions for the entity's player slot, from the action map. Written by input.actions; gameplay reads it and never sees keys.")
            .source(format!("{}:1", self.file));
        for a in &self.actions {
            let doc = if a.doc.is_empty() {
                format!("action {}", a.name)
            } else {
                a.doc.clone()
            };
            b = match a.kind {
                ActionKind::Button => b
                    .field_full(
                        &a.name,
                        FieldType::Bool,
                        Value::Bool(false),
                        None,
                        doc.clone(),
                        false,
                    )
                    .field_full(
                        format!("{}_pressed", a.name),
                        FieldType::Bool,
                        Value::Bool(false),
                        None,
                        format!("{doc} (went down this tick)"),
                        false,
                    )
                    .field_full(
                        format!("{}_released", a.name),
                        FieldType::Bool,
                        Value::Bool(false),
                        None,
                        format!("{doc} (went up this tick)"),
                        false,
                    ),
                ActionKind::Axis => b.field_full(
                    &a.name,
                    FieldType::F32,
                    Value::Float(0.0),
                    Some((-1.0, 1.0)),
                    doc,
                    false,
                ),
                ActionKind::Axis2 => b.field_full(
                    &a.name,
                    FieldType::Vec2,
                    Value::Vec2(argus_ecs::Vec2::ZERO),
                    None,
                    doc,
                    false,
                ),
            };
        }
        b.build()
    }

    /// `input.bindings`: the map as the protocol reports it.
    pub fn to_json(&self) -> Json {
        json!({
            "file": self.file,
            "actions": self.actions.iter().map(|a| json!({
                "name": a.name,
                "type": a.kind.name(),
                "doc": a.doc,
                "toggle": a.toggle,
                "bindings": a.bindings,
                "at": a.at,
            })).collect::<Vec<_>>(),
            "slots": self.slots.iter().map(|s| json!({ "slot": s.index, "devices": s.devices })).collect::<Vec<_>>(),
        })
    }
}
