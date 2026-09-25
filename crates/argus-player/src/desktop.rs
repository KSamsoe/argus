//! The desktop's devices in the input layer's names (phase 1 item 2, shared
//! in phase 2 item 7): the daemon's window and a release build read the
//! keyboard, the mouse, and gamepads through the same translation, so a pad
//! button means the same action in both.

use argus_input::DeviceInput;
use argus_render::window::DeviceEvent;
use std::collections::BTreeMap;

/// What a window event is to a player.
#[derive(Clone, Debug, PartialEq)]
pub enum WindowInput {
    /// Input for the action map.
    Device(DeviceInput),
    /// Text typed (a note).
    Text(String),
    /// The window's new size.
    Resized(u32, u32),
    /// The mouse moved to a place in the window, in its pixels (y down);
    /// the host puts it on the UI canvas (`argus_ui::Fit`) as a pointer.
    Cursor(f32, f32),
    /// The close button.
    Close,
}

/// A window event in the input layer's names; `None` for what nobody reads.
pub fn window_input(ev: DeviceEvent) -> Option<WindowInput> {
    Some(match ev {
        DeviceEvent::Key { code, pressed } => WindowInput::Device(DeviceInput::Key {
            name: code,
            pressed,
        }),
        DeviceEvent::Mouse {
            button, pressed, ..
        } => WindowInput::Device(DeviceInput::Mouse {
            button: button.to_ascii_lowercase(),
            pressed,
        }),
        DeviceEvent::Cursor { at } => WindowInput::Cursor(at.0, at.1),
        DeviceEvent::Focus { focused: false } => WindowInput::Device(DeviceInput::ReleaseAll),
        DeviceEvent::Text { text } => WindowInput::Text(text),
        DeviceEvent::Resized { width, height } => WindowInput::Resized(width, height),
        DeviceEvent::CloseRequested => WindowInput::Close,
        _ => return None,
    })
}

/// Gamepads, numbered in the order they were first seen.
pub struct Pads {
    gilrs: Option<gilrs::Gilrs>,
    announced: bool,
    index: BTreeMap<usize, u32>,
}

impl Pads {
    /// Open the platform's gamepads; `is_open` is false where there are none.
    pub fn open() -> Pads {
        let mut p = Pads {
            gilrs: gilrs::Gilrs::new().ok(),
            announced: false,
            index: BTreeMap::new(),
        };
        let ids: Vec<usize> = p
            .gilrs
            .iter()
            .flat_map(|g| g.gamepads().map(|(id, _)| usize::from(id)))
            .collect();
        for id in ids {
            p.index_of(id);
        }
        p
    }

    /// The platform has gamepads.
    pub fn is_open(&self) -> bool {
        self.gilrs.is_some()
    }

    fn index_of(&mut self, id: usize) -> u32 {
        let next = self.index.len() as u32;
        *self.index.entry(id).or_insert(next)
    }

    /// What the pads did since the last poll. Pads already plugged in are
    /// reported as connected on the first.
    pub fn poll(&mut self) -> Vec<DeviceInput> {
        let mut raw = Vec::new();
        if let Some(g) = &mut self.gilrs {
            if !self.announced {
                self.announced = true;
                for (id, gp) in g.gamepads() {
                    raw.push((
                        usize::from(id),
                        gp.name().to_string(),
                        gilrs::EventType::Connected,
                    ));
                }
            }
            while let Some(ev) = g.next_event() {
                let name = g.gamepad(ev.id).name().to_string();
                raw.push((usize::from(ev.id), name, ev.event));
            }
        }
        let mut out = Vec::new();
        for (id, name, ev) in raw {
            let pad = self.index_of(id);
            use gilrs::EventType as E;
            match ev {
                E::Connected => out.push(DeviceInput::PadConnected {
                    pad,
                    name,
                    connected: true,
                }),
                E::Disconnected => out.push(DeviceInput::PadConnected {
                    pad,
                    name,
                    connected: false,
                }),
                E::ButtonPressed(b, _) | E::ButtonRepeated(b, _) => {
                    if let Some(b) = pad_button(b) {
                        out.push(DeviceInput::PadButton {
                            pad,
                            button: b.into(),
                            pressed: true,
                        });
                    }
                }
                E::ButtonReleased(b, _) => {
                    if let Some(b) = pad_button(b) {
                        out.push(DeviceInput::PadButton {
                            pad,
                            button: b.into(),
                            pressed: false,
                        });
                    }
                }
                E::ButtonChanged(b, v, _) => {
                    // Analog triggers arrive as button values.
                    if let Some(n @ ("lt" | "rt")) = pad_button(b) {
                        out.push(DeviceInput::PadAxis {
                            pad,
                            axis: n.into(),
                            value: v,
                        });
                    }
                }
                E::AxisChanged(a, v, _) => {
                    if let Some(a) = pad_axis(a) {
                        out.push(DeviceInput::PadAxis {
                            pad,
                            axis: a.into(),
                            value: v,
                        });
                    }
                }
                _ => {}
            }
        }
        out
    }
}

fn pad_button(b: gilrs::Button) -> Option<&'static str> {
    use gilrs::Button as B;
    Some(match b {
        B::South => "south",
        B::East => "east",
        B::West => "west",
        B::North => "north",
        B::LeftTrigger => "lb",
        B::RightTrigger => "rb",
        B::LeftTrigger2 => "lt",
        B::RightTrigger2 => "rt",
        B::Select => "select",
        B::Start => "start",
        B::Mode => "mode",
        B::LeftThumb => "lthumb",
        B::RightThumb => "rthumb",
        B::DPadUp => "dpad_up",
        B::DPadDown => "dpad_down",
        B::DPadLeft => "dpad_left",
        B::DPadRight => "dpad_right",
        _ => return None,
    })
}

fn pad_axis(a: gilrs::Axis) -> Option<&'static str> {
    use gilrs::Axis as A;
    Some(match a {
        A::LeftStickX => "left_x",
        A::LeftStickY => "left_y",
        A::RightStickX => "right_x",
        A::RightStickY => "right_y",
        A::LeftZ => "lt",
        A::RightZ => "rt",
        _ => return None,
    })
}
