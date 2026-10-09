//! Real devices (phase 1, item 2): keyboard, mouse, and gamepads, read
//! through the bindings `input.toml` has declared since phase 0.
//!
//! This module holds raw device state and resolves it into action values
//! per player slot. It knows no window system: the daemon feeds it
//! [`DeviceInput`] from winit and gilrs. What gameplay sees is unchanged
//! (§12 "gameplay never sees keys"): a resolved value that differs from
//! the last one is scheduled into [`crate::InputState`] like an inject, so
//! the log records actions and a replay survives a rebind (§6).
//!
//! Binding vocabulary, per device:
//!
//! - keyboard: a key name (`w`, `space`, `enter`, `left`, `1`, `period`,
//!   `shiftleft`, `f1`); for an `axis`, a pair `neg/pos` (`a/d`); for an
//!   `axis2`, `wasd`, `arrows`, `ijkl`, or four names `up,left,down,right`.
//! - mouse: `left`, `right`, `middle`, `back`, `forward`.
//! - gamepad: `south`, `east`, `west`, `north`, `lb`, `rb`, `lt`, `rt`,
//!   `select`, `start`, `mode`, `lthumb`, `rthumb`, `dpad_up`,
//!   `dpad_down`, `dpad_left`, `dpad_right`; axes `left_x`, `left_y`,
//!   `right_x`, `right_y`, `lt`, `rt`; sticks `left_stick`, `right_stick`,
//!   `dpad` for an `axis2`.
//!
//! - platform: `overlay`, held while the platform's overlay (Steam's) is
//!   open (phase 5 item 9), so a game binds its pause to it.
//!
//! A slot's `devices` list says which devices feed it: `keyboard`,
//! `mouse`, `gamepad` (any pad), `gamepad:N` (the Nth pad connected,
//! from 0), so two gamepads are two players (§12). The platform feeds
//! every slot.
//!
//! The device a slot's player last pressed something on is its
//! [`Family`], the value of the map's `device` action.

use crate::glyphs::Family;
use crate::map::{ActionKind, ActionMap, DEVICE_ACTION};
use crate::state::ActionValue;
use std::collections::{BTreeMap, BTreeSet};

/// Stick dead zone.
pub const DEAD_ZONE: f32 = 0.2;
/// A trigger past this is a pressed button.
pub const TRIGGER_PRESS: f32 = 0.5;

/// One raw device event, in the names this module resolves.
#[derive(Clone, Debug, PartialEq)]
pub enum DeviceInput {
    /// A key went down or up; any spelling [`key_name`] accepts.
    Key {
        /// Key name.
        name: String,
        /// Down or up.
        pressed: bool,
    },
    /// A mouse button.
    Mouse {
        /// `left`, `right`, `middle`, `back`, `forward`.
        button: String,
        /// Down or up.
        pressed: bool,
    },
    /// A gamepad button, by pad index in connection order.
    PadButton {
        /// Pad index from 0.
        pad: u32,
        /// Button name (see module docs).
        button: String,
        /// Down or up.
        pressed: bool,
    },
    /// A gamepad axis in -1..1 (triggers 0..1).
    PadAxis {
        /// Pad index from 0.
        pad: u32,
        /// Axis name (see module docs).
        axis: String,
        /// Value.
        value: f32,
    },
    /// A pad connected or went away.
    PadConnected {
        /// Pad index from 0.
        pad: u32,
        /// The device's name.
        name: String,
        /// Connected or disconnected.
        connected: bool,
    },
    /// The mouse is at a place on the UI canvas, in its pixels (the host
    /// converts from the window's; phase 3 item 1).
    Pointer {
        /// Canvas x, from the left.
        x: f32,
        /// Canvas y, from the top.
        y: f32,
    },
    /// What kind of pad it is, when the host knows better than its name
    /// (Steam Input does; phase 5 item 9).
    PadFamily {
        /// Pad index from 0.
        pad: u32,
        /// Its kind.
        family: Family,
    },
    /// Something of the platform's held or let go: `overlay` (phase 5
    /// item 9).
    Platform {
        /// Its name.
        name: String,
        /// Held.
        on: bool,
    },
    /// Focus lost: everything held is released (no stuck keys).
    ReleaseAll,
}

/// Normalise a key name: winit's `KeyW`, `Digit1`, `ArrowLeft` and a
/// binding's `w`, `1`, `left` all become the same lowercase token.
pub fn key_name(s: &str) -> String {
    let s = s.trim();
    let s = s
        .strip_prefix("Code(")
        .and_then(|x| x.strip_suffix(')'))
        .unwrap_or(s);
    let t = if let Some(r) = s.strip_prefix("Key").filter(|r| r.len() == 1) {
        r
    } else if let Some(r) = s.strip_prefix("Digit").filter(|r| r.len() == 1) {
        r
    } else if let Some(r) = s.strip_prefix("Arrow") {
        r
    } else {
        s
    };
    match t.to_ascii_lowercase().as_str() {
        "return" | "numpadenter" => "enter".to_string(),
        "esc" => "escape".to_string(),
        other => other.to_string(),
    }
}

#[derive(Clone, Debug, Default)]
struct Pad {
    name: String,
    family: Option<Family>,
    buttons: BTreeSet<String>,
    axes: BTreeMap<String, f32>,
    /// When something was last pressed on it (the devices' own count).
    used: u64,
}

impl Pad {
    fn family(&self) -> Family {
        self.family
            .unwrap_or_else(|| Family::of_pad_name(&self.name))
    }
}

/// Raw state of every device.
#[derive(Clone, Debug, Default)]
pub struct Devices {
    keys: BTreeSet<String>,
    mouse: BTreeSet<String>,
    pointer: Option<(f32, f32)>,
    pads: BTreeMap<u32, Pad>,
    platform: BTreeSet<String>,
    /// A count of presses: what was used last has the highest.
    uses: u64,
    /// When a key or the mouse was last used.
    keyboard_used: u64,
    /// Events seen, for `input.devices`.
    pub events_seen: u64,
}

impl Devices {
    /// Apply one event.
    pub fn apply(&mut self, ev: &DeviceInput) {
        self.events_seen += 1;
        match ev {
            DeviceInput::Key { name, pressed } => {
                let k = key_name(name);
                if *pressed {
                    self.keys.insert(k);
                    self.uses += 1;
                    self.keyboard_used = self.uses;
                } else {
                    self.keys.remove(&k);
                }
            }
            DeviceInput::Mouse { button, pressed } => {
                let b = button.to_ascii_lowercase();
                if *pressed {
                    self.mouse.insert(b);
                    self.uses += 1;
                    self.keyboard_used = self.uses;
                } else {
                    self.mouse.remove(&b);
                }
            }
            DeviceInput::PadButton {
                pad,
                button,
                pressed,
            } => {
                let p = self.pads.entry(*pad).or_default();
                if *pressed {
                    p.buttons.insert(button.clone());
                    self.uses += 1;
                    p.used = self.uses;
                } else {
                    p.buttons.remove(button);
                }
            }
            DeviceInput::Pointer { x, y } => {
                // A mouse that moves is the mouse in use.
                if self
                    .pointer
                    .is_some_and(|(px, py)| (px - x).abs() + (py - y).abs() > 2.0)
                {
                    self.uses += 1;
                    self.keyboard_used = self.uses;
                }
                self.pointer = Some((*x, *y));
            }
            DeviceInput::PadAxis { pad, axis, value } => {
                let p = self.pads.entry(*pad).or_default();
                let v = value.clamp(-1.0, 1.0);
                let was = p.axes.get(axis).copied().unwrap_or(0.0);
                let past = |v: f32| match axis.as_str() {
                    "lt" | "rt" => v > TRIGGER_PRESS,
                    _ => v.abs() > 0.5,
                };
                if past(v) && !past(was) {
                    self.uses += 1;
                    p.used = self.uses;
                }
                p.axes.insert(axis.clone(), v);
            }
            DeviceInput::PadConnected {
                pad,
                name,
                connected,
            } => {
                if *connected {
                    self.pads.entry(*pad).or_default().name = name.clone();
                } else {
                    self.pads.remove(pad);
                }
            }
            DeviceInput::PadFamily { pad, family } => {
                self.pads.entry(*pad).or_default().family = Some(*family);
            }
            DeviceInput::Platform { name, on } => {
                if *on {
                    self.platform.insert(name.clone());
                } else {
                    self.platform.remove(name);
                }
            }
            DeviceInput::ReleaseAll => {
                self.keys.clear();
                self.mouse.clear();
                for p in self.pads.values_mut() {
                    p.buttons.clear();
                    p.axes.clear();
                }
            }
        }
    }

    /// Keys held, mouse buttons held, and connected pads, for `input.devices`.
    pub fn summary(&self) -> (Vec<String>, Vec<String>, Vec<(u32, String)>) {
        (
            self.keys.iter().cloned().collect(),
            self.mouse.iter().cloned().collect(),
            self.pads
                .iter()
                .map(|(i, p)| (*i, p.name.clone()))
                .collect(),
        )
    }

    fn key(&self, k: &str) -> bool {
        self.keys.contains(&key_name(k))
    }

    /// The kind of device a slot's player last pressed something on: the
    /// keyboard and mouse if they feed it and nothing else was used since,
    /// else the pad used last (a pad never used counts if nothing was);
    /// `None` when nothing feeds the slot (its `device` is left to
    /// injects, as an action no device binds is).
    pub fn family(&self, map: &ActionMap, slot: u8) -> Option<Family> {
        let s = map.slot(slot).ok()?;
        let keyboard = s.devices.iter().any(|d| d == "keyboard" || d == "mouse");
        let mut best: Option<(u64, Family)> =
            keyboard.then_some((self.keyboard_used, Family::Keyboard));
        for p in self.pads_for(&s.devices) {
            if best.is_none_or(|(u, _)| p.used > u) {
                best = Some((p.used, p.family()));
            }
        }
        best.map(|(_, f)| f)
    }

    fn pad_axis(p: &Pad, axis: &str) -> f32 {
        let v = p.axes.get(axis).copied().unwrap_or(0.0);
        if v.abs() < DEAD_ZONE {
            0.0
        } else {
            v
        }
    }

    fn pad_button(p: &Pad, b: &str) -> bool {
        if p.buttons.contains(b) {
            return true;
        }
        match b {
            "lt" | "rt" => p.axes.get(b).copied().unwrap_or(0.0) > TRIGGER_PRESS,
            _ => false,
        }
    }

    /// The pads a slot's device list names.
    fn pads_for<'a>(&'a self, devices: &'a [String]) -> Vec<&'a Pad> {
        let mut out = Vec::new();
        for d in devices {
            if d == "gamepad" {
                out.extend(self.pads.values());
            } else if let Some(n) = d
                .strip_prefix("gamepad:")
                .and_then(|n| n.parse::<u32>().ok())
            {
                // The Nth pad in connection order, not a raw id.
                if let Some(p) = self.pads.values().nth(n as usize) {
                    out.push(p);
                }
            }
        }
        out
    }

    /// Resolve every action of `map` for `slot` from the devices the slot
    /// lists. Actions no device of the slot binds are left out, so they
    /// stay whatever injects made them.
    pub fn resolve(&self, map: &ActionMap, slot: u8) -> Vec<(String, ActionValue)> {
        let Ok(s) = map.slot(slot) else {
            return Vec::new();
        };
        let has = |d: &str| s.devices.iter().any(|x| x == d);
        let keyboard = has("keyboard");
        let mouse = has("mouse");
        let pads = self.pads_for(&s.devices);
        let mut out = Vec::new();
        for a in &map.actions {
            if a.name == DEVICE_ACTION {
                if let Some(f) = self.family(map, slot) {
                    out.push((a.name.clone(), ActionValue::Axis(f.index())));
                }
                continue;
            }
            let kb = a.bindings.get("keyboard").filter(|_| keyboard);
            let ms = a.bindings.get("mouse").filter(|_| mouse);
            let gp = a.bindings.get("gamepad").filter(|_| !pads.is_empty());
            let pf = a.bindings.get("platform");
            if kb.is_none() && ms.is_none() && gp.is_none() && pf.is_none() {
                continue;
            }
            let v = match a.kind {
                ActionKind::Button => {
                    let mut on = false;
                    for b in kb.into_iter().flatten() {
                        on |= self.key(b);
                    }
                    for b in ms.into_iter().flatten() {
                        on |= self.mouse.contains(&b.to_ascii_lowercase());
                    }
                    for b in gp.into_iter().flatten() {
                        on |= pads.iter().any(|p| Self::pad_button(p, b));
                    }
                    for b in pf.into_iter().flatten() {
                        on |= self.platform.contains(b);
                    }
                    ActionValue::Button(on)
                }
                ActionKind::Axis => {
                    let mut best = 0.0f32;
                    let mut take = |v: f32| {
                        if v.abs() > best.abs() {
                            best = v;
                        }
                    };
                    for b in kb.into_iter().flatten() {
                        if let Some((neg, pos)) = b.split_once('/') {
                            take(self.key(pos) as i32 as f32 - self.key(neg) as i32 as f32);
                        } else {
                            take(self.key(b) as i32 as f32);
                        }
                    }
                    for b in gp.into_iter().flatten() {
                        for p in &pads {
                            take(Self::pad_axis(p, b));
                        }
                    }
                    ActionValue::Axis(best)
                }
                // Where the mouse is, once it has been anywhere.
                ActionKind::Pointer => match (ms, self.pointer) {
                    (Some(_), Some((x, y))) => ActionValue::Axis2(x, y),
                    _ => continue,
                },
                ActionKind::Axis2 => {
                    let mut best = (0.0f32, 0.0f32);
                    let mut take = |v: (f32, f32)| {
                        if v.0 * v.0 + v.1 * v.1 > best.0 * best.0 + best.1 * best.1 {
                            best = v;
                        }
                    };
                    for b in kb.into_iter().flatten() {
                        let names: Vec<String> = match b.as_str() {
                            "wasd" => vec!["w", "a", "s", "d"],
                            "arrows" => vec!["up", "left", "down", "right"],
                            "ijkl" => vec!["i", "j", "k", "l"],
                            other => other.split(',').map(str::trim).collect(),
                        }
                        .into_iter()
                        .map(str::to_string)
                        .collect();
                        if names.len() == 4 {
                            let x = self.key(&names[3]) as i32 - self.key(&names[1]) as i32;
                            let y = self.key(&names[0]) as i32 - self.key(&names[2]) as i32;
                            let (x, y) = (x as f32, y as f32);
                            let len = (x * x + y * y).sqrt();
                            take(if len > 1.0 {
                                (x / len, y / len)
                            } else {
                                (x, y)
                            });
                        }
                    }
                    for b in gp.into_iter().flatten() {
                        for p in &pads {
                            let v = match b.as_str() {
                                "left_stick" => {
                                    (Self::pad_axis(p, "left_x"), Self::pad_axis(p, "left_y"))
                                }
                                "right_stick" => {
                                    (Self::pad_axis(p, "right_x"), Self::pad_axis(p, "right_y"))
                                }
                                "dpad" => (
                                    p.buttons.contains("dpad_right") as i32 as f32
                                        - p.buttons.contains("dpad_left") as i32 as f32,
                                    p.buttons.contains("dpad_up") as i32 as f32
                                        - p.buttons.contains("dpad_down") as i32 as f32,
                                ),
                                _ => (0.0, 0.0),
                            };
                            take(v);
                        }
                    }
                    ActionValue::Axis2(best.0, best.1)
                }
            };
            out.push((a.name.clone(), v));
        }
        out
    }
}

/// Gamepad names this module understands, for binding validation.
pub const PAD_NAMES: &[&str] = &[
    "south",
    "east",
    "west",
    "north",
    "lb",
    "rb",
    "lt",
    "rt",
    "select",
    "start",
    "mode",
    "lthumb",
    "rthumb",
    "dpad_up",
    "dpad_down",
    "dpad_left",
    "dpad_right",
    "left_x",
    "left_y",
    "right_x",
    "right_y",
    "left_stick",
    "right_stick",
    "dpad",
];

/// Mouse names this module understands.
pub const MOUSE_NAMES: &[&str] = &["left", "right", "middle", "back", "forward"];

/// The platform's names: its overlay, held while it is open.
pub const PLATFORM_NAMES: &[&str] = &["overlay"];
