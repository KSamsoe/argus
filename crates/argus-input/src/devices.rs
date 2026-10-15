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
//! - mouse: `left`, `right`, `middle`, `back`, `forward`, each with any
//!   modifiers before it (`ctrl+left`, `shift+alt+right`; issue 33): held
//!   modifiers make it a binding of its own, and a plain `left` stands
//!   down while a `ctrl+left` of the slot's is pressed.
//! - gamepad: `south`, `east`, `west`, `north`, `lb`, `rb`, `lt`, `rt`,
//!   `select`, `start`, `mode`, `lthumb`, `rthumb`, `dpad_up`,
//!   `dpad_down`, `dpad_left`, `dpad_right`; axes `left_x`, `left_y`,
//!   `right_x`, `right_y`, `lt`, `rt`; sticks `left_stick`, `right_stick`,
//!   `dpad` for an `axis2`.
//!
//! - platform: `overlay`, held while the platform's overlay (Steam's) is
//!   open (phase 5 item 9), so a game binds its pause to it.
//! - touch: a control's name from `[touch.<name>]` (reaching players item
//!   1): a button for a button, a stick for an axis2, `<stick>.x` or
//!   `<stick>.y` for an axis. A finger that lands on no control is the
//!   mouse (its left button and the pointer), so a game made for the mouse
//!   works under a finger.
//!
//! A slot's `devices` list says which devices feed it: `keyboard`,
//! `mouse`, `gamepad` (any pad), `gamepad:N` (the Nth pad connected,
//! from 0), so two gamepads are two players (§12), `touch`. The platform
//! feeds every slot, and touch goes with the mouse: a slot that lists
//! `mouse` takes it too.
//!
//! The device a slot's player last pressed something on is its
//! [`Family`], the value of the map's `device` action.

use crate::glyphs::Family;
use crate::map::{ActionKind, ActionMap, DEVICE_ACTION};
use crate::state::ActionValue;
use crate::touch::{TouchArea, TouchKind, TouchPhase, TouchShown};
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
    /// The mouse wheel turned: lines, up positive (issue 23).
    Wheel {
        /// Lines turned, up positive.
        lines: f32,
    },
    /// Text typed: characters, and backspace, enter, and escape as
    /// `\u{8}`, `\r`, and `\u{1b}` (issue 23). A slot that is typing
    /// gets it; otherwise it is dropped.
    Text {
        /// What was typed.
        text: String,
    },
    /// A finger on a touchscreen, at a place on the UI canvas in its
    /// pixels (the host converts; reaching players item 1).
    Touch {
        /// The finger: the same id from its landing to its lift.
        id: u64,
        /// Landed, moved, lifted, or taken away.
        phase: TouchPhase,
        /// Canvas x, from the left.
        x: f32,
        /// Canvas y, from the top.
        y: f32,
    },
    /// Focus lost: everything held is released (no stuck keys).
    ReleaseAll,
}

/// The most text kept between two polls of the devices.
pub const MAX_TYPED: usize = 4096;

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

/// A finger down (reaching players item 1).
#[derive(Clone, Debug, PartialEq)]
struct Finger {
    at: (f32, f32),
    grab: Grab,
}

/// What a finger is doing.
#[derive(Clone, Debug, PartialEq)]
enum Grab {
    /// Landed since the controls were last looked at.
    New,
    /// Holding a control, with the centre it is held from (a floating
    /// stick's is where the finger landed).
    Control(String, (f32, f32)),
    /// Being the mouse: its left button and the pointer.
    Mouse,
    /// A second finger off the controls: nothing.
    Idle,
}

/// Raw state of every device.
#[derive(Clone, Debug, Default)]
pub struct Devices {
    /// Fingers down, by id.
    touches: BTreeMap<u64, Finger>,
    /// When a finger last landed (the devices' own count).
    touch_used: u64,
    /// The touch controls on the canvas, from the map
    /// ([`Devices::set_touch_layout`]).
    touch_layout: Vec<TouchArea>,
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
    /// The wheel's lines since the last poll ([`Devices::take_typed`]).
    wheel: f32,
    /// Text typed since the last poll.
    text: String,
    /// Keys held when typing stopped: none of them moves an action until
    /// it is let go, so the Enter that sent a text box's line does not
    /// press the button it lands on (issue 23).
    muted: BTreeSet<String>,
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
                    self.muted.remove(&k);
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
            DeviceInput::Wheel { lines } => {
                if lines.is_finite() {
                    self.wheel += lines;
                    self.uses += 1;
                    self.keyboard_used = self.uses;
                }
            }
            DeviceInput::Text { text } => {
                if self.text.len() + text.len() <= MAX_TYPED {
                    self.text.push_str(text);
                }
            }
            DeviceInput::Touch { id, phase, x, y } => {
                let at = (*x, *y);
                match phase {
                    TouchPhase::Start => {
                        // It takes its control when the devices are next
                        // resolved, with the controls laid out by then.
                        self.touches.insert(
                            *id,
                            Finger {
                                at,
                                grab: Grab::New,
                            },
                        );
                        self.uses += 1;
                        self.touch_used = self.uses;
                    }
                    TouchPhase::Move => {
                        if let Some(f) = self.touches.get_mut(id) {
                            f.at = at;
                            if f.grab == Grab::Mouse {
                                self.pointer = Some(at);
                            }
                        }
                    }
                    TouchPhase::End | TouchPhase::Cancel => {
                        if let Some(f) = self.touches.remove(id) {
                            if f.grab == Grab::Mouse {
                                self.pointer = Some(at);
                                self.mouse.remove("left");
                            }
                        }
                    }
                }
            }
            DeviceInput::ReleaseAll => {
                self.touches.clear();
                self.keys.clear();
                self.mouse.clear();
                for p in self.pads.values_mut() {
                    p.buttons.clear();
                    p.axes.clear();
                }
            }
        }
    }

    /// The touch controls as laid out on the canvas (the input state's,
    /// from its map). Fingers that landed before it was known take their
    /// controls now.
    pub fn set_touch_layout(&mut self, layout: &[TouchArea]) {
        if self.touch_layout != layout {
            self.touch_layout = layout.to_vec();
        }
        self.settle_touches();
    }

    /// Each finger that landed since the last look takes the control it
    /// landed on (the nearest, a button before a floating stick's area; a
    /// stick one finger at a time), else is the mouse if no finger is, else
    /// does nothing.
    fn settle_touches(&mut self) {
        let new: Vec<u64> = self
            .touches
            .iter()
            .filter(|(_, f)| f.grab == Grab::New)
            .map(|(id, _)| *id)
            .collect();
        for id in new {
            let at = self.touches[&id].at;
            let held = |name: &str, kind: TouchKind| {
                kind == TouchKind::Stick
                    && self
                        .touches
                        .values()
                        .any(|f| matches!(&f.grab, Grab::Control(n, _) if n == name))
            };
            let control = self
                .touch_layout
                .iter()
                .filter(|c| !held(&c.name, c.kind))
                .filter_map(|c| c.reach(at).map(|d| (d, c)))
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, c)| {
                    let centre = if c.floating.is_some() { at } else { c.centre };
                    Grab::Control(c.name.clone(), centre)
                });
            let grab = control.unwrap_or_else(|| {
                if self.touches.values().any(|f| f.grab == Grab::Mouse) {
                    Grab::Idle
                } else {
                    self.mouse.insert("left".to_string());
                    self.pointer = Some(at);
                    Grab::Mouse
                }
            });
            self.touches.get_mut(&id).expect("listed above").grab = grab;
        }
    }

    /// The fingers holding a touch control: the centre each holds it from,
    /// and where it is.
    fn touch_held<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = ((f32, f32), (f32, f32))> + 'a {
        self.touches.values().filter_map(move |f| match &f.grab {
            Grab::Control(n, c) if n == name => Some((*c, f.at)),
            _ => None,
        })
    }

    /// The touch controls the engine draws, as they are now: while the
    /// `device` action of the slot touch feeds (the first that lists
    /// `touch` or `mouse`) is touch, else none. That is the run's value, as
    /// the button prompts follow, not these devices': so a new run starts
    /// without them, and a seek or a replay shows them as the run did.
    pub fn touch_shown(&self, st: &crate::state::InputState) -> Vec<TouchShown> {
        let map = st.map();
        let Some(slot) = map
            .slots
            .iter()
            .find(|s| s.devices.iter().any(|d| d == "touch" || d == "mouse"))
        else {
            return Vec::new();
        };
        let family = match st.value(slot.index, DEVICE_ACTION) {
            Some(ActionValue::Axis(v)) => Family::from_index(v),
            _ => Family::Keyboard,
        };
        if family != Family::Touch {
            return Vec::new();
        }
        self.touch_layout
            .iter()
            .filter(|c| c.draw)
            .map(|c| {
                let held = self.touch_held(&c.name).next();
                let centre = held.map(|(c, _)| c).unwrap_or(c.centre);
                let knob = match (c.kind, held) {
                    (TouchKind::Stick, Some((from, at))) => {
                        let (vx, vy) = c.stick(from, at);
                        (from.0 + vx * c.radius, from.1 - vy * c.radius)
                    }
                    _ => centre,
                };
                // A button with no words of its own has its action's name.
                let label = if c.label.is_empty() && c.kind == TouchKind::Button {
                    map.actions
                        .iter()
                        .find(|a| a.bindings.get("touch").is_some_and(|v| v.contains(&c.name)))
                        .map(|a| crate::glyphs::words_of(&a.name))
                        .unwrap_or_default()
                } else {
                    c.label.clone()
                };
                TouchShown {
                    name: c.name.clone(),
                    kind: c.kind,
                    centre,
                    radius: c.radius,
                    knob,
                    pressed: held.is_some(),
                    label,
                }
            })
            .collect()
    }

    /// Fingers down, for `input.devices`: each finger's id, where it is,
    /// and what it holds (a control's name, `mouse`, or nothing).
    pub fn touches(&self) -> Vec<(u64, (f32, f32), String)> {
        self.touches
            .iter()
            .map(|(id, f)| {
                let what = match &f.grab {
                    Grab::Control(n, _) => n.clone(),
                    Grab::Mouse => "mouse".to_string(),
                    Grab::New | Grab::Idle => String::new(),
                };
                (*id, f.at, what)
            })
            .collect()
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
        let k = key_name(k);
        self.keys.contains(&k) && !self.muted.contains(&k)
    }

    /// Ignore every key held now until it is let go: typing stopped, and
    /// the keys that stopped it are still down.
    pub fn mute_held(&mut self) {
        self.muted = self.keys.clone();
    }

    /// The kind of device a slot's player last pressed something on: the
    /// keyboard and mouse if they feed it and nothing else was used since,
    /// else the pad used last (a pad never used counts if nothing was);
    /// `None` when nothing feeds the slot (its `device` is left to
    /// injects, as an action no device binds is).
    pub fn family(&self, map: &ActionMap, slot: u8) -> Option<Family> {
        let s = map.slot(slot).ok()?;
        let keyboard = s.devices.iter().any(|d| d == "keyboard" || d == "mouse");
        let touch = s.devices.iter().any(|d| d == "touch" || d == "mouse");
        let mut best: Option<(u64, Family)> =
            keyboard.then_some((self.keyboard_used, Family::Keyboard));
        // A finger that landed after the keys and the mouse were last used.
        if touch && self.touch_used > 0 && best.is_none_or(|(u, _)| self.touch_used > u) {
            best = Some((self.touch_used, Family::Touch));
        }
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
        self.resolve_typing(map, slot, false)
    }

    /// The wheel's lines and the text typed since the last call, taken.
    pub fn take_typed(&mut self) -> (f32, String) {
        (
            std::mem::take(&mut self.wheel),
            std::mem::take(&mut self.text),
        )
    }

    /// [`Devices::resolve`] for a slot that may be typing: then its
    /// keyboard moves no action, and what it held is let go (issue 23).
    pub fn resolve_typing(
        &self,
        map: &ActionMap,
        slot: u8,
        typing: bool,
    ) -> Vec<(String, ActionValue)> {
        let Ok(s) = map.slot(slot) else {
            return Vec::new();
        };
        let has = |d: &str| s.devices.iter().any(|x| x == d);
        let keyboard = has("keyboard");
        let mouse = has("mouse");
        // Touch goes with the mouse (reaching players item 1).
        let touch = has("touch") || mouse;
        let pads = self.pads_for(&s.devices);
        // A mouse binding with modifiers (issue 33), pressed with them held:
        // its button is that binding's, and a plain binding of the same
        // button stands down, so Ctrl and a click is not also a click.
        let held = |m: &str| {
            MODIFIERS
                .iter()
                .find(|(n, _)| *n == m)
                .is_some_and(|(_, keys)| keys.iter().any(|k| self.key(k)))
        };
        let pressed = |b: &str| -> bool {
            let (mods, button) = mouse_parts(b);
            self.mouse.contains(&button) && mods.iter().all(|m| held(m))
        };
        let mut taken: BTreeSet<String> = BTreeSet::new();
        if mouse {
            for a in &map.actions {
                for b in a.bindings.get("mouse").into_iter().flatten() {
                    let (mods, button) = mouse_parts(b);
                    if !mods.is_empty() && pressed(b) {
                        taken.insert(button);
                    }
                }
            }
        }
        let mut out = Vec::new();
        for a in &map.actions {
            if a.name == DEVICE_ACTION {
                if let Some(f) = self.family(map, slot) {
                    out.push((a.name.clone(), ActionValue::Axis(f.index())));
                }
                continue;
            }
            let kb_bound = a.bindings.get("keyboard").filter(|_| keyboard);
            let ms = a.bindings.get("mouse").filter(|_| mouse);
            let gp = a.bindings.get("gamepad").filter(|_| !pads.is_empty());
            let pf = a.bindings.get("platform");
            let tc = a.bindings.get("touch").filter(|_| touch);
            if kb_bound.is_none() && ms.is_none() && gp.is_none() && pf.is_none() && tc.is_none() {
                continue;
            }
            // Typing: the keyboard holds nothing, so what it held is let go.
            let kb = kb_bound.filter(|_| !typing);
            let v = match a.kind {
                ActionKind::Button => {
                    let mut on = false;
                    for b in kb.into_iter().flatten() {
                        on |= self.key(b);
                    }
                    for b in ms.into_iter().flatten() {
                        let (mods, button) = mouse_parts(b);
                        on |= pressed(b) && (!mods.is_empty() || !taken.contains(&button));
                    }
                    for b in gp.into_iter().flatten() {
                        on |= pads.iter().any(|p| Self::pad_button(p, b));
                    }
                    for b in pf.into_iter().flatten() {
                        on |= self.platform.contains(b);
                    }
                    for b in tc.into_iter().flatten() {
                        on |= self.touch_held(b).next().is_some();
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
                    for b in tc.into_iter().flatten() {
                        let Some((stick, axis)) = b.rsplit_once('.') else {
                            continue;
                        };
                        if let Some(c) = self.touch_layout.iter().find(|c| c.name == stick) {
                            for (from, at) in self.touch_held(stick) {
                                let v = c.stick(from, at);
                                take(if axis == "x" { v.0 } else { v.1 });
                            }
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
                    for b in tc.into_iter().flatten() {
                        if let Some(c) = self.touch_layout.iter().find(|c| c.name == *b) {
                            for (from, at) in self.touch_held(b) {
                                take(c.stick(from, at));
                            }
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

/// Modifier keys a mouse binding may carry (issue 33), and the keys that
/// hold each.
pub const MODIFIERS: &[(&str, &[&str])] = &[
    ("ctrl", &["controlleft", "controlright"]),
    ("shift", &["shiftleft", "shiftright"]),
    ("alt", &["altleft", "altright"]),
    (
        "meta",
        &["metaleft", "metaright", "superleft", "superright"],
    ),
];

/// A mouse binding's modifiers and button: `ctrl+shift+left` is
/// `(["ctrl", "shift"], "left")`.
pub fn mouse_parts(b: &str) -> (Vec<String>, String) {
    let mut parts: Vec<String> = b
        .split('+')
        .map(|p| p.trim().to_ascii_lowercase())
        .collect();
    let button = parts.pop().unwrap_or_default();
    (parts, button)
}

/// Whether a mouse button binding reads: a button, with known modifiers.
pub fn mouse_button_ok(b: &str) -> bool {
    let (mods, button) = mouse_parts(b);
    MOUSE_NAMES.contains(&button.as_str())
        && mods.iter().all(|m| MODIFIERS.iter().any(|(n, _)| n == m))
}

/// The platform's names: its overlay, held while it is open.
pub const PLATFORM_NAMES: &[&str] = &["overlay"];
