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
//! [actions.pause.bindings]
//! keyboard = ["escape"]
//! platform = ["overlay"]         # held while the platform's overlay is open
//!
//! [slots.1]
//! devices = ["keyboard", "gamepad:0"]
//! [slots.2]
//! devices = ["gamepad:1"]
//! ```
//!
//! Every map also has the engine's own action [`DEVICE_ACTION`], `device`:
//! the kind of device the player last used, an axis whose value is a
//! [`crate::glyphs::Family`] (0 keyboard and mouse, 1 Xbox, 2 PlayStation,
//! 3 Switch, 4 another gamepad, 5 touch; phase 5 item 9). It is logged like any
//! action, so the button prompts a replay shows are the run's.

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
    /// A place on the UI canvas, in its pixels (phase 3 item 1: the mouse
    /// for menus). It stays where it was left.
    Pointer,
}

impl ActionKind {
    /// Map spelling.
    pub fn name(self) -> &'static str {
        match self {
            ActionKind::Button => "button",
            ActionKind::Axis => "axis",
            ActionKind::Axis2 => "axis2",
            ActionKind::Pointer => "pointer",
        }
    }

    fn parse(s: &str) -> Option<ActionKind> {
        Some(match s {
            "button" => ActionKind::Button,
            "axis" => ActionKind::Axis,
            "axis2" => ActionKind::Axis2,
            "pointer" => ActionKind::Pointer,
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
    /// Touch controls, `[touch.<name>]` (reaching players item 1), in file
    /// order.
    pub touch: Vec<crate::touch::TouchControl>,
}

const DEVICES: &[&str] = &["keyboard", "mouse", "gamepad", "platform", "touch"];

/// The engine's own action in every map: the kind of device the player
/// last used (a [`crate::glyphs::Family`] index), which button prompts
/// follow.
pub const DEVICE_ACTION: &str = "device";

/// The Actions field holding what a slot typed this tick (issue 23).
pub const TYPED: &str = "typed";
/// The Actions field that makes a slot's keyboard type instead of moving
/// actions (issue 23).
pub const TYPING: &str = "typing";

impl ActionMap {
    /// Read `input.toml` under `root`; `None` when the project has none.
    pub fn load(root: &Path) -> Result<Option<ActionMap>> {
        let p = root.join("input.toml");
        if !argus_os::fs::is_file(&p) {
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
                .hint("input.toml has [actions.<name>] tables with type = \"button\" | \"axis\" | \"axis2\" | \"pointer\", optional doc, toggle, and [actions.<name>.bindings] per device; [slots.<n>] tables list devices; [touch.<name>] tables are touch controls.")
                .at(Some(at))
        };
        // The touch controls first: a touch binding names one.
        let touch = match t.get("touch") {
            Some(i) => crate::touch::parse(file, text, i)?,
            None => Vec::new(),
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
                let kind = match it.get("type") {
                    Some(i) => {
                        let k = i.as_str().unwrap_or_default();
                        ActionKind::parse(k).ok_or_else(|| {
                            bad(
                                format!("actions.{name}.type {k:?} is not button, axis, axis2, or pointer"),
                                aat.clone(),
                            )
                        })?
                    }
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
                        // Keyboard names are open-ended; mouse and gamepad
                        // names are a fixed set, so a typo is an error now
                        // rather than a button that never fires.
                        let known: &[&str] = match dev {
                            // A pointer is where the mouse is.
                            "mouse" if kind == ActionKind::Pointer => &["position"],
                            // The wheel turns an axis: a notch is a one-tick
                            // push of 1 or -1 (issue 23).
                            "mouse" if kind == ActionKind::Axis => &["wheel"],
                            "mouse" => crate::devices::MOUSE_NAMES,
                            "gamepad" => crate::devices::PAD_NAMES,
                            "platform" => crate::devices::PLATFORM_NAMES,
                            _ => &[],
                        };
                        // A mouse button may carry modifiers (issue 33).
                        let fine = |i: &String| {
                            if dev == "mouse" && kind == ActionKind::Button {
                                crate::devices::mouse_button_ok(i)
                            } else {
                                known.contains(&i.as_str())
                            }
                        };
                        // A touch binding names a control of the right kind.
                        if dev == "touch" {
                            if let Some(bad_name) = items
                                .iter()
                                .find(|i| !crate::touch::binding_ok(&touch, kind, i))
                            {
                                let want = match kind {
                                    ActionKind::Button => "a button control's name",
                                    ActionKind::Axis2 => "a stick control's name",
                                    ActionKind::Axis => "a stick control's name with .x or .y",
                                    ActionKind::Pointer => "nothing: a finger off the controls is the mouse, so bind mouse = [\"position\"]",
                                };
                                let names: Vec<&str> =
                                    touch.iter().map(|c| c.name.as_str()).collect();
                                return Err(bad(
                                    format!(
                                        "actions.{name}.bindings.touch: {bad_name:?} is not {want}; {}",
                                        if names.is_empty() {
                                            "input.toml declares no [touch] controls".to_string()
                                        } else {
                                            format!("[touch] declares: {}", names.join(", "))
                                        }
                                    ),
                                    tomlv::item_ref(file, text, list),
                                ));
                            }
                        }
                        if !known.is_empty() {
                            if let Some(bad_name) = items.iter().find(|i| !fine(i)) {
                                return Err(bad(
                                    format!("actions.{name}.bindings.{dev}: unknown {dev} input {bad_name:?}; known: {}", known.join(", ")),
                                    tomlv::item_ref(file, text, list),
                                ));
                            }
                        }
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
        if let Some(a) = actions.iter().find(|a| a.name == DEVICE_ACTION) {
            return Err(bad(
                format!("actions.{DEVICE_ACTION} is the engine's: the kind of device the player last used, which button prompts follow"),
                a.at.clone(),
            ));
        }
        if let Some(a) = actions.iter().find(|a| a.name == TYPED || a.name == TYPING) {
            return Err(bad(
                format!(
                    "actions.{} is the engine's: Actions.typed is what was typed this tick, and Actions.typing makes the keyboard type",
                    a.name
                ),
                a.at.clone(),
            ));
        }
        actions.push(Action {
            name: DEVICE_ACTION.to_string(),
            kind: ActionKind::Axis,
            doc: "The kind of device the player last used (the engine's): 0 keyboard and mouse, 1 Xbox, 2 PlayStation, 3 Switch, 4 another gamepad, 5 touch".to_string(),
            bindings: BTreeMap::new(),
            toggle: false,
            at: "argus-input".to_string(),
        });
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
            touch,
        })
    }

    /// Bind `action` on `device` to `names` instead of what it had (§12
    /// "Rebinding at runtime is a UI over the same table"; phase 3 item 1).
    /// Mouse and gamepad names are checked as the file's are; an empty
    /// list unbinds the device.
    pub fn set_bindings(&mut self, action: &str, device: &str, names: Vec<String>) -> Result<()> {
        if !DEVICES.contains(&device) {
            return Err(ArgusError::new(
                codes::BAD_MAP,
                format!(
                    "unknown device {device:?}; devices are {}",
                    DEVICES.join(", ")
                ),
            ));
        }
        let kind = self.action(action)?.kind;
        let known: &[&str] = match device {
            "mouse" if kind == ActionKind::Pointer => &["position"],
            "mouse" => crate::devices::MOUSE_NAMES,
            "gamepad" => crate::devices::PAD_NAMES,
            "platform" => crate::devices::PLATFORM_NAMES,
            _ => &[],
        };
        let fine = |n: &String| {
            if device == "mouse" && kind == ActionKind::Button {
                crate::devices::mouse_button_ok(n)
            } else {
                known.contains(&n.as_str())
            }
        };
        if let Some(bad_name) = names
            .iter()
            .find(|n| device == "touch" && !crate::touch::binding_ok(&self.touch, kind, n))
        {
            return Err(ArgusError::new(
                codes::BAD_MAP,
                format!("{bad_name:?} is no touch control of the kind {action} needs"),
            ));
        }
        if let Some(bad_name) = names.iter().find(|n| !known.is_empty() && !fine(n)) {
            return Err(ArgusError::new(
                codes::BAD_MAP,
                format!(
                    "unknown {device} input {bad_name:?}; known: {}",
                    known.join(", ")
                ),
            ));
        }
        let a = self
            .actions
            .iter_mut()
            .find(|a| a.name == action)
            .expect("found above");
        if names.is_empty() {
            a.bindings.remove(device);
        } else {
            a.bindings.insert(device.to_string(), names);
        }
        Ok(())
    }

    /// Write `action`'s bindings on `device` into `input.toml` under
    /// `root`, keeping the rest of the file as it is: a rebinding the
    /// project keeps.
    pub fn save_bindings(root: &Path, action: &str, device: &str, names: &[String]) -> Result<()> {
        let path = root.join("input.toml");
        let text = argus_os::fs::read_to_string(&path)
            .map_err(|e| ArgusError::new(codes::NO_INPUT_MAP, format!("input.toml: {e}")))?;
        let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| {
            ArgusError::new(codes::BAD_MAP, format!("input.toml: {e}"))
        })?;
        let arr: toml_edit::Array = names.iter().map(String::as_str).collect();
        let bindings = &mut doc["actions"][action]["bindings"];
        if names.is_empty() {
            if let Some(t) = bindings.as_table_like_mut() {
                t.remove(device);
            }
        } else {
            bindings[device] = toml_edit::value(arr);
        }
        argus_os::fs::write(&path, doc.to_string())
            .map_err(|e| ArgusError::new(codes::BAD_MAP, format!("input.toml: {e}")))
    }

    /// Whether a press of `action` (a button) toggles it instead of
    /// holding it: the hold-or-toggle choice a player makes (section 12
    /// accessibility; phase 5 item 2).
    pub fn set_toggle(&mut self, action: &str, toggle: bool) -> Result<()> {
        if toggle && self.action(action)?.kind != ActionKind::Button {
            return Err(ArgusError::new(
                codes::BAD_MAP,
                format!("{action}: toggle applies to buttons only"),
            ));
        }
        let a = self
            .actions
            .iter_mut()
            .find(|a| a.name == action)
            .ok_or_else(|| {
                ArgusError::new(codes::UNKNOWN_ACTION, format!("no action named {action:?}"))
            })?;
        a.toggle = toggle;
        Ok(())
    }

    /// The names `action` is bound to on `device`.
    pub fn bindings(&self, action: &str, device: &str) -> Vec<String> {
        self.action(action)
            .ok()
            .and_then(|a| a.bindings.get(device).cloned())
            .unwrap_or_default()
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
                    // The device action holds a family's index, 0 to 5.
                    Some(if a.name == DEVICE_ACTION {
                        (0.0, 5.0)
                    } else {
                        (-1.0, 1.0)
                    }),
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
                ActionKind::Pointer => b.field_full(
                    &a.name,
                    FieldType::Vec2,
                    Value::Vec2(argus_ecs::Vec2::ZERO),
                    None,
                    format!("{doc} (a place on the UI canvas, in its pixels)"),
                    false,
                ),
            };
        }
        b.field_full(
            TYPED,
            FieldType::Str,
            Value::Str(String::new()),
            None,
            "What the player typed this tick, while typing is true (the engine's): text, with backspace, enter, and escape as \\u{8}, \\r, and \\u{1b}",
            false,
        )
        .field_full(
            TYPING,
            FieldType::Bool,
            Value::Bool(false),
            None,
            "While true, the slot's keyboard types (into typed) instead of moving actions (the engine's; a text box sets it)",
            false,
        )
        .build()
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
