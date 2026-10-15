//! Touch controls (reaching players item 1): buttons and sticks on the
//! screen, declared in `input.toml` as `[touch.<name>]` and bound to
//! actions as a key is (`touch = ["jump"]`).
//!
//! A control is placed on the UI canvas from an anchor (a corner, an
//! edge's middle, or the centre) by `at`, its centre measured inward from
//! that anchor, and `size`, its diameter, all in canvas pixels. A finger
//! that lands on one holds it until it lifts; a floating stick is held by
//! a finger landing anywhere in its half of the screen, and is centred
//! where it landed. The first finger that lands on no control is the
//! mouse: its left button and the pointer.
//!
//! The engine draws the controls over the game while touch is the device
//! the player last used ([`crate::glyphs::Family::Touch`]), unless a
//! control says `draw = false` and the game draws its own.

use argus_ecs::{ArgusError, Result};
use argus_scene::tomlv;
use toml_edit::Item;

use crate::codes;

/// A finger's part in a touch: it lands, it moves, it lifts, or the
/// system took it away (an incoming call, a gesture of the OS's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchPhase {
    /// The finger landed.
    Start,
    /// It moved.
    Move,
    /// It lifted.
    End,
    /// The system took it: as a lift.
    Cancel,
}

impl TouchPhase {
    /// `start`, `move`, `end`, or `cancel`.
    pub fn parse(s: &str) -> Option<TouchPhase> {
        Some(match s {
            "start" => TouchPhase::Start,
            "move" => TouchPhase::Move,
            "end" => TouchPhase::End,
            "cancel" => TouchPhase::Cancel,
            _ => return None,
        })
    }
}

/// What a control is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchKind {
    /// Held while a finger is on it: a button action.
    Button,
    /// A drag from its centre: an axis2 action, or one of its axes
    /// (`<name>.x`, `<name>.y`) for an axis.
    Stick,
}

/// The anchors a control may be placed from.
pub const ANCHORS: &[&str] = &[
    "top-left",
    "top",
    "top-right",
    "left",
    "center",
    "right",
    "bottom-left",
    "bottom",
    "bottom-right",
];

/// A control as `input.toml` declares it.
#[derive(Clone, Debug, PartialEq)]
pub struct TouchControl {
    /// Its name, which bindings use.
    pub name: String,
    /// Button or stick.
    pub kind: TouchKind,
    /// One of [`ANCHORS`].
    pub anchor: String,
    /// Its centre, from the anchor inward, in canvas pixels.
    pub at: [f32; 2],
    /// Its diameter in canvas pixels.
    pub size: f32,
    /// A button's words; empty: the name of the action bound to it.
    pub label: String,
    /// A stick held from anywhere in its half of the screen, centred where
    /// the finger lands.
    pub floating: bool,
    /// Drawn by the engine (the default), or by the game itself.
    pub draw: bool,
}

/// A control placed on a canvas of a size.
#[derive(Clone, Debug, PartialEq)]
pub struct TouchArea {
    /// Its name.
    pub name: String,
    /// Button or stick.
    pub kind: TouchKind,
    /// Its centre in canvas pixels (a floating stick's when no finger
    /// holds it).
    pub centre: (f32, f32),
    /// Half its size.
    pub radius: f32,
    /// A floating stick's half of the screen: x0, y0, x1, y1.
    pub floating: Option<[f32; 4]>,
    /// Its words.
    pub label: String,
    /// Drawn by the engine.
    pub draw: bool,
}

impl TouchArea {
    /// Whether a finger landing at `p` takes this control; how near its
    /// centre, to choose between two (a floating stick's area is far).
    pub fn reach(&self, p: (f32, f32)) -> Option<f32> {
        let d = ((p.0 - self.centre.0).powi(2) + (p.1 - self.centre.1).powi(2)).sqrt();
        // A little past its edge: a thumb lands roughly.
        if d <= self.radius * 1.25 {
            return Some(d);
        }
        match self.floating {
            Some([x0, y0, x1, y1]) if p.0 >= x0 && p.0 < x1 && p.1 >= y0 && p.1 < y1 => {
                Some(f32::MAX / 2.0)
            }
            _ => None,
        }
    }

    /// A stick's value for a finger at `p` from `centre`: up positive, as
    /// an axis2's, at most 1 long, nothing in the middle tenth.
    pub fn stick(&self, centre: (f32, f32), p: (f32, f32)) -> (f32, f32) {
        let (dx, dy) = (p.0 - centre.0, p.1 - centre.1);
        let len = (dx * dx + dy * dy).sqrt();
        if len < self.radius * 0.1 || self.radius <= 0.0 {
            return (0.0, 0.0);
        }
        let k = len.max(self.radius);
        (dx / k, -dy / k)
    }
}

/// Lay controls out on the screen `[x0, y0, x1, y1]`, in UI canvas pixels:
/// the canvas itself (`[0, 0, width, height]`), or more when the window is
/// wider or taller than the canvas, so the controls sit at the screen's
/// own edges.
pub fn layout(controls: &[TouchControl], screen: [f32; 4]) -> Vec<TouchArea> {
    let [x0, y0, x1, y1] = screen;
    let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    controls
        .iter()
        .map(|c| {
            let [ax, ay] = c.at;
            let (x, side) = match c.anchor.as_str() {
                "top-left" | "left" | "bottom-left" => (x0 + ax, -1),
                "top-right" | "right" | "bottom-right" => (x1 - ax, 1),
                _ => (mx + ax, 0),
            };
            let y = match c.anchor.as_str() {
                "top-left" | "top" | "top-right" => y0 + ay,
                "bottom-left" | "bottom" | "bottom-right" => y1 - ay,
                _ => my + ay,
            };
            let floating = (c.kind == TouchKind::Stick && c.floating).then_some(match side {
                -1 => [x0, y0, mx, y1],
                1 => [mx, y0, x1, y1],
                _ => [x0, y0, x1, y1],
            });
            TouchArea {
                name: c.name.clone(),
                kind: c.kind,
                centre: (x, y),
                radius: c.size / 2.0,
                floating,
                label: c.label.clone(),
                draw: c.draw,
            }
        })
        .collect()
}

/// A control as the engine draws it this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct TouchShown {
    /// Its name.
    pub name: String,
    /// Button or stick.
    pub kind: TouchKind,
    /// Its centre in canvas pixels (a floating stick's where it is held).
    pub centre: (f32, f32),
    /// Half its size.
    pub radius: f32,
    /// A stick's knob: where the finger is, kept within the stick.
    pub knob: (f32, f32),
    /// A finger is on it.
    pub pressed: bool,
    /// A button's words.
    pub label: String,
}

/// Parse `input.toml`'s `[touch]` table.
pub fn parse(file: &str, text: &str, item: &Item) -> Result<Vec<TouchControl>> {
    let at = tomlv::item_ref(file, text, item);
    let bad = |msg: String, at: String| {
        ArgusError::new(codes::BAD_MAP, msg)
            .hint("A touch control is [touch.<name>] with kind = \"button\" | \"stick\", anchor (top-left, top, top-right, left, center, right, bottom-left, bottom, bottom-right), at = [x, y] from the anchor inward, size (its diameter), and optional label, floating (a stick), and draw; all in UI canvas pixels.")
            .at(Some(at))
    };
    let t = tomlv::table_like(item, "touch", &at)?;
    let mut out = Vec::new();
    for (name, item) in t.iter() {
        let cat = tomlv::item_ref(file, text, item);
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(bad(
                format!("touch control {name:?} must be letters, digits, and underscores"),
                cat,
            ));
        }
        let c = tomlv::table_like(item, &format!("touch.{name}"), &cat)?;
        for (k, _) in c.iter() {
            if !["kind", "anchor", "at", "size", "label", "floating", "draw"].contains(&k) {
                return Err(bad(format!("touch.{name}: unknown key {k:?}"), cat.clone()));
            }
        }
        let kind = match c.get("kind").and_then(|i| i.as_str()) {
            Some("button") => TouchKind::Button,
            Some("stick") => TouchKind::Stick,
            other => {
                return Err(bad(
                    format!(
                        "touch.{name}.kind {:?} is not button or stick",
                        other.unwrap_or("")
                    ),
                    cat,
                ))
            }
        };
        let anchor = c
            .get("anchor")
            .and_then(|i| i.as_str())
            .unwrap_or("center")
            .to_string();
        if !ANCHORS.contains(&anchor.as_str()) {
            return Err(bad(
                format!(
                    "touch.{name}.anchor {anchor:?} is not one of {}",
                    ANCHORS.join(", ")
                ),
                cat,
            ));
        }
        let num = |i: &Item| i.as_float().or_else(|| i.as_integer().map(|v| v as f64));
        let at_xy = match c.get("at") {
            None => [0.0, 0.0],
            Some(i) => {
                let xs: Vec<f64> = i
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| {
                                v.as_float().or_else(|| v.as_integer().map(|n| n as f64))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if xs.len() != 2 {
                    return Err(bad(format!("touch.{name}.at is [x, y], two numbers"), cat));
                }
                [xs[0] as f32, xs[1] as f32]
            }
        };
        let size = match c.get("size") {
            None => 96.0,
            Some(i) => match num(i) {
                Some(v) if v > 0.0 => v as f32,
                _ => return Err(bad(format!("touch.{name}.size is a number above 0"), cat)),
            },
        };
        let flag = |k: &str, default: bool| c.get(k).and_then(|i| i.as_bool()).unwrap_or(default);
        let floating = flag("floating", false);
        if floating && kind != TouchKind::Stick {
            return Err(bad(
                format!("touch.{name}: floating applies to sticks only"),
                cat,
            ));
        }
        out.push(TouchControl {
            name: name.to_string(),
            kind,
            anchor,
            at: at_xy,
            size,
            label: c
                .get("label")
                .and_then(|i| i.as_str())
                .unwrap_or("")
                .to_string(),
            floating,
            draw: flag("draw", true),
        });
    }
    Ok(out)
}

/// Whether `name` is a touch binding a control in `controls` answers for
/// an action of `kind`: a button for a button, a stick for an axis2, a
/// stick's `.x` or `.y` for an axis.
pub fn binding_ok(controls: &[TouchControl], kind: crate::map::ActionKind, name: &str) -> bool {
    use crate::map::ActionKind;
    let find = |n: &str, k: TouchKind| controls.iter().any(|c| c.name == n && c.kind == k);
    match kind {
        ActionKind::Button => find(name, TouchKind::Button),
        ActionKind::Axis2 => find(name, TouchKind::Stick),
        ActionKind::Axis => name
            .rsplit_once('.')
            .is_some_and(|(n, a)| (a == "x" || a == "y") && find(n, TouchKind::Stick)),
        ActionKind::Pointer => false,
    }
}
