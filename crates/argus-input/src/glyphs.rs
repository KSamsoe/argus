//! Button prompts (§15 "input glyphs", phase 5 item 9): what the player
//! presses for an action, on the kind of device they last used.
//!
//! The device a player last pressed something on is the map's `device`
//! action, a [`Family`]: logged like any action, so a replay shows the
//! prompts the run showed. An [`InputGlyph`] names an action; `input.glyphs`
//! writes its `label` (`Space`, `A`, `Cross`) for a UiText hole
//! (`{InputGlyph.label}`) and its `image`, the sprite `glyph/<family>/<input>`
//! (`glyph/xbox/south`), which a platform with pictures of its buttons
//! provides (Steam's) and a UiPanel on the same entity draws when `panel`
//! is set. Without one, the label is the prompt.

use crate::map::{ActionMap, DEVICE_ACTION};
use crate::state::{ActionValue, InputState};
use argus_ecs::{Component, ComponentId, FieldSchema, Result, StrId, Value, World};
use argus_sim::{Scheduler, Stage, SystemDesc};
use std::cell::RefCell;
use std::rc::Rc;

/// The prompt system's name.
pub const GLYPHS: &str = "input.glyphs";

/// A kind of device, whose buttons have their own names.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Family {
    /// Keyboard and mouse.
    #[default]
    Keyboard,
    /// An Xbox pad (and the XInput pads named for it).
    Xbox,
    /// A PlayStation pad.
    PlayStation,
    /// A Nintendo Switch pad: its A is east, B south.
    Switch,
    /// Any other gamepad, named by position.
    Gamepad,
}

impl Family {
    /// Every family, in index order.
    pub const ALL: [Family; 5] = [
        Family::Keyboard,
        Family::Xbox,
        Family::PlayStation,
        Family::Switch,
        Family::Gamepad,
    ];

    /// Its value as the `device` action holds it.
    pub fn index(self) -> f32 {
        self as u8 as f32
    }

    /// The family a `device` value names (a value out of range is a
    /// gamepad's).
    pub fn from_index(v: f32) -> Family {
        match v.round() as i64 {
            0 => Family::Keyboard,
            1 => Family::Xbox,
            2 => Family::PlayStation,
            3 => Family::Switch,
            _ => Family::Gamepad,
        }
    }

    /// Its name: `keyboard`, `xbox`, `playstation`, `switch`, `gamepad`.
    pub fn name(self) -> &'static str {
        match self {
            Family::Keyboard => "keyboard",
            Family::Xbox => "xbox",
            Family::PlayStation => "playstation",
            Family::Switch => "switch",
            Family::Gamepad => "gamepad",
        }
    }

    /// The family a name names.
    pub fn parse(s: &str) -> Option<Family> {
        Family::ALL.into_iter().find(|f| f.name() == s)
    }

    /// What a pad is, from the name its driver gives it.
    pub fn of_pad_name(name: &str) -> Family {
        let n = name.to_ascii_lowercase();
        let has = |w: &[&str]| w.iter().any(|x| n.contains(x));
        if has(&[
            "dualsense",
            "dualshock",
            "playstation",
            "ps3",
            "ps4",
            "ps5",
            "sony",
        ]) {
            Family::PlayStation
        } else if has(&["switch", "pro controller", "joy-con", "joycon", "nintendo"]) {
            Family::Switch
        } else if has(&["xbox", "xinput", "x-box"]) {
            Family::Xbox
        } else {
            Family::Gamepad
        }
    }

    /// The device of the action map whose bindings this family presses.
    fn device(self) -> &'static str {
        match self {
            Family::Keyboard => "keyboard",
            _ => "gamepad",
        }
    }
}

/// What a binding is called on a family of device: `space` is `Space`,
/// `south` is `A` on an Xbox pad, `Cross` on a PlayStation, `B` on a Switch.
pub fn label(family: Family, binding: &str) -> String {
    if family == Family::Keyboard {
        return key_label(binding);
    }
    let pad = |xbox: &str, ps: &str, switch: &str, generic: &str| {
        match family {
            Family::Xbox => xbox,
            Family::PlayStation => ps,
            Family::Switch => switch,
            _ => generic,
        }
        .to_string()
    };
    match binding {
        "south" => pad("A", "Cross", "B", "South"),
        "east" => pad("B", "Circle", "A", "East"),
        "west" => pad("X", "Square", "Y", "West"),
        "north" => pad("Y", "Triangle", "X", "North"),
        "lb" => pad("LB", "L1", "L", "LB"),
        "rb" => pad("RB", "R1", "R", "RB"),
        "lt" => pad("LT", "L2", "ZL", "LT"),
        "rt" => pad("RT", "R2", "ZR", "RT"),
        "select" => pad("View", "Create", "-", "Select"),
        "start" => pad("Menu", "Options", "+", "Start"),
        "mode" => pad("Xbox", "PS", "Home", "Home"),
        "lthumb" => pad("LS", "L3", "L stick", "L3"),
        "rthumb" => pad("RS", "R3", "R stick", "R3"),
        "dpad_up" => "D-pad up".to_string(),
        "dpad_down" => "D-pad down".to_string(),
        "dpad_left" => "D-pad left".to_string(),
        "dpad_right" => "D-pad right".to_string(),
        "dpad" => "D-pad".to_string(),
        "left_stick" | "left_x" | "left_y" => "Left stick".to_string(),
        "right_stick" | "right_x" | "right_y" => "Right stick".to_string(),
        other => other.to_string(),
    }
}

/// A key's name as a player reads it: `Space`, `Esc`, `Left Shift`, `W`.
fn key_label(binding: &str) -> String {
    let named = match binding {
        "wasd" => "WASD",
        "arrows" => "Arrow keys",
        "ijkl" => "IJKL",
        "space" => "Space",
        "escape" => "Esc",
        "enter" => "Enter",
        "tab" => "Tab",
        "backspace" => "Backspace",
        "up" => "Up",
        "down" => "Down",
        "left" => "Left",
        "right" => "Right",
        "shiftleft" => "Left Shift",
        "shiftright" => "Right Shift",
        "controlleft" => "Left Ctrl",
        "controlright" => "Right Ctrl",
        "altleft" => "Left Alt",
        "altright" => "Right Alt",
        "period" => ".",
        "comma" => ",",
        "slash" => "/",
        "semicolon" => ";",
        "minus" => "-",
        "equal" => "=",
        _ => "",
    };
    if !named.is_empty() {
        return named.to_string();
    }
    if let Some((neg, pos)) = binding.split_once('/') {
        return format!("{}/{}", key_label(neg), key_label(pos));
    }
    let mut c = binding.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// The input a family presses for an action, and its label: its first
/// binding on the family's device (the mouse's, for the keyboard family,
/// when it has no key).
pub fn prompt(map: &ActionMap, action: &str, family: Family) -> Option<(String, String)> {
    let a = map.action(action).ok()?;
    let b = a.bindings.get(family.device()).and_then(|v| v.first());
    if b.is_none() && family == Family::Keyboard {
        if let Some(m) = a.bindings.get("mouse").and_then(|v| v.first()) {
            let l = match m.as_str() {
                "left" => "Left click".to_string(),
                "right" => "Right click".to_string(),
                "middle" => "Middle click".to_string(),
                other => format!("Mouse {other}"),
            };
            return Some((format!("mouse_{m}"), l));
        }
    }
    let b = b?;
    Some((b.clone(), label(family, b)))
}

/// A button prompt on a UI node.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A button prompt: what the player presses for an action on the kind of device they last used. Its label fills a UiText hole ({InputGlyph.label}); with panel set, its image, a picture of the button where the platform has one (Steam), is drawn by the UiPanel on the same entity"
)]
pub struct InputGlyph {
    /// The action.
    #[argus(doc = "The action whose input it shows")]
    pub action: StrId,
    /// The slot whose device it follows.
    #[argus(doc = "The player slot whose device it follows, 1 first")]
    pub slot: u32,
    /// A family to show instead.
    #[argus(
        doc = "Show this kind of device instead of the player's: keyboard, xbox, playstation, switch, gamepad; empty follows the player"
    )]
    pub family: StrId,
    /// What to press.
    #[argus(
        doc = "What to press, as the player reads it: Space, A, Cross (written by input.glyphs)"
    )]
    pub label: StrId,
    /// The button's picture.
    #[argus(
        doc = "The button's picture, the sprite glyph/<family>/<input> the platform provides (empty for a key; written by input.glyphs)"
    )]
    pub image: StrId,
    /// The family shown.
    #[argus(doc = "The kind of device shown (written by input.glyphs)")]
    pub shown: StrId,
    /// Draw the picture as the entity's panel.
    #[argus(
        doc = "Draw the button's picture as this entity's UiPanel image (none for a key, or where the platform has no picture)"
    )]
    pub panel: bool,
}

impl Default for InputGlyph {
    fn default() -> Self {
        InputGlyph {
            action: StrId::EMPTY,
            slot: 1,
            family: StrId::EMPTY,
            label: StrId::EMPTY,
            image: StrId::EMPTY,
            shown: StrId::EMPTY,
            panel: false,
        }
    }
}

/// Register `InputGlyph` and add `input.glyphs` to the sim stage, after
/// the rebinding system when there is one (a rebind shows at once).
pub fn install(
    world: &mut World,
    sched: &mut Scheduler,
    state: Rc<RefCell<InputState>>,
) -> Result<()> {
    let id = match world.component_id_of::<InputGlyph>() {
        Some(id) => id,
        None => world.register::<InputGlyph>()?,
    };
    let panel = world.component_id("UiPanel");
    let image_f: Option<FieldSchema> =
        panel.and_then(|p| world.registry().schema(p).field("image").cloned());
    let mut desc = SystemDesc::new(GLYPHS, Stage::Sim)
        .unfrozen()
        .writes(id)
        .source("argus-input:glyphs")
        .doc("Button prompts: every InputGlyph's label and image say what its player presses for its action on the kind of device they last used (the device action); with panel set, the UiPanel on the same entity draws the image.");
    if let Some(p) = panel {
        desc = desc.writes(p);
    }
    if sched.is_enabled(crate::REBIND).is_ok() {
        desc = desc.after(crate::REBIND);
    }
    sched.add_system(
        desc,
        Box::new(move |ctx| {
            let world: &World = ctx.world();
            if !world
                .archetypes()
                .iter()
                .any(|a| !a.is_empty() && a.has(id))
            {
                return Ok(());
            }
            let st = state.borrow();
            let strings = world.strings();
            write_all(world, &st, id, panel, image_f.as_ref(), &|s: &str| {
                strings.try_intern(s)
            })
        }),
    )
}

fn write_all(
    world: &World,
    st: &InputState,
    id: ComponentId,
    panel: Option<ComponentId>,
    image_f: Option<&FieldSchema>,
    intern: &dyn Fn(&str) -> Result<StrId>,
) -> Result<()> {
    world.query().has(id).for_each_view(|view| {
        let arch = view.archetype();
        let mut gs = view.column_mut::<InputGlyph>()?;
        let panels = match panel.filter(|p| arch.has(*p)) {
            Some(p) => Some(view.dyn_column_mut(p)?),
            None => None,
        };
        for row in 0..view.len() {
            let g = &mut gs[row];
            let forced = if g.family == StrId::EMPTY {
                None
            } else {
                Family::parse(&world.strings().resolve(g.family))
            };
            let family =
                forced.unwrap_or_else(|| match st.value(g.slot.clamp(1, 8) as u8, DEVICE_ACTION) {
                    Some(ActionValue::Axis(v)) => Family::from_index(v),
                    _ => Family::Keyboard,
                });
            let action = world.strings().resolve(g.action);
            let (label, image) = match prompt(st.map(), &action, family) {
                Some((input, label)) => {
                    let image = if family == Family::Keyboard {
                        String::new()
                    } else {
                        format!("glyph/{}/{input}", family.name())
                    };
                    (label, image)
                }
                None => (String::new(), String::new()),
            };
            let (label, image, shown) = (intern(&label)?, intern(&image)?, intern(family.name())?);
            if (g.label, g.image, g.shown) != (label, image, shown) {
                g.label = label;
                g.image = image;
                g.shown = shown;
            }
            if let (Some(pc), Some(f), true) = (&panels, image_f, g.panel) {
                if pc.str_id_by(row, f) != Some(image) {
                    pc.set_str_id_by(row, f, image)?;
                }
            }
        }
        Ok(())
    })
}

/// The value the `device` action holds for a family, for an inject
/// (`input.inject {action: "device", value: 1}` is a player on an Xbox
/// pad).
pub fn device_value(family: Family) -> Value {
    Value::Float(family.index() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_button_is_named_as_its_pad_names_it() {
        assert_eq!(label(Family::Xbox, "south"), "A");
        assert_eq!(label(Family::PlayStation, "south"), "Cross");
        assert_eq!(label(Family::Switch, "south"), "B");
        assert_eq!(label(Family::Switch, "east"), "A");
        assert_eq!(label(Family::Gamepad, "start"), "Start");
        assert_eq!(label(Family::Keyboard, "space"), "Space");
        assert_eq!(label(Family::Keyboard, "z"), "Z");
        assert_eq!(label(Family::Keyboard, "a/d"), "A/D");
        assert_eq!(label(Family::Keyboard, "shiftleft"), "Left Shift");
        assert_eq!(label(Family::Keyboard, "f5"), "F5");
    }

    #[test]
    fn a_pads_family_comes_from_its_name() {
        assert_eq!(Family::of_pad_name("Xbox Controller"), Family::Xbox);
        assert_eq!(
            Family::of_pad_name("DualSense Wireless Controller"),
            Family::PlayStation
        );
        assert_eq!(Family::of_pad_name("Pro Controller"), Family::Switch);
        assert_eq!(Family::of_pad_name("8BitDo SN30"), Family::Gamepad);
        for f in Family::ALL {
            assert_eq!(Family::from_index(f.index()), f);
            assert_eq!(Family::parse(f.name()), Some(f));
        }
    }
}
