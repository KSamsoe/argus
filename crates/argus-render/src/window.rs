//! The windowed target's host: a winit window pumped from the daemon
//! thread, never blocking it. Device events are collected here and read
//! by the input layer (phase 1, item 2); this module only opens, pumps,
//! resizes, and closes.

use argus_ecs::{ArgusError, Result};
use std::cell::RefCell;
use std::sync::Arc;
use std::time::Duration;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::platform::pump_events::EventLoopExtPumpEvents;
use winit::window::{Window, WindowId};

/// A device event the input layer will map to actions.
#[derive(Clone, Debug, PartialEq)]
pub enum DeviceEvent {
    /// A key went down or up; `code` is winit's physical key debug name.
    Key {
        /// Physical key name, e.g. `KeyW`, `Space`, `ArrowLeft`.
        code: String,
        /// Down or up.
        pressed: bool,
    },
    /// A mouse button went down or up at the last known position.
    Mouse {
        /// Button name: `Left`, `Right`, `Middle`, or `Other(n)`.
        button: String,
        /// Down or up.
        pressed: bool,
        /// Pixel position, y down.
        at: (f32, f32),
    },
    /// The cursor moved.
    Cursor {
        /// Pixel position, y down.
        at: (f32, f32),
    },
    /// The window was resized.
    Resized {
        /// New size in pixels.
        width: u32,
        /// New size in pixels.
        height: u32,
    },
    /// The user asked to close the window.
    CloseRequested,
    /// Text typed, as the keyboard layout gives it (a note after F8).
    Text {
        /// The characters.
        text: String,
    },
    /// The window gained or lost keyboard focus.
    Focus {
        /// Focused now.
        focused: bool,
    },
}

/// The name a key is sent under: winit's key code spelling (`KeyW`,
/// `ArrowLeft`, `Space`). `argus_input::key_name` turns it into a binding's
/// token.
pub fn physical_key_name(key: &PhysicalKey) -> String {
    format!("{key:?}")
        .trim_start_matches("Code(")
        .trim_end_matches(')')
        .to_string()
}

thread_local! {
    /// winit allows one event loop per process, so a closed window's loop is
    /// kept here for the next window this thread opens.
    static SPARE: RefCell<Option<EventLoop<()>>> = const { RefCell::new(None) };
}

struct App {
    title: String,
    width: u32,
    height: u32,
    window: Option<Arc<Window>>,
    /// The window's id, kept after the handle goes so its last events are
    /// told apart from a later window's on the same loop.
    id: Option<WindowId>,
    events: Vec<DeviceEvent>,
    cursor: (f32, f32),
    closed: bool,
    visible: bool,
    destroyed: bool,
}

impl App {
    fn ensure_window(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() || self.id.is_some() || self.closed {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(self.title.clone())
            .with_inner_size(winit::dpi::LogicalSize::new(self.width, self.height))
            .with_visible(self.visible);
        match el.create_window(attrs) {
            Ok(w) => {
                self.id = Some(w.id());
                self.window = Some(Arc::new(w));
            }
            Err(_) => self.closed = true,
        }
    }
}

impl ApplicationHandler for App {
    // A fresh loop resumes once; a reused one does not, so the window is
    // made from whichever comes first.
    fn resumed(&mut self, el: &ActiveEventLoop) {
        self.ensure_window(el);
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.ensure_window(el);
    }

    fn window_event(&mut self, _el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if Some(id) != self.id {
            return;
        }
        match event {
            WindowEvent::Focused(focused) => {
                self.events.push(DeviceEvent::Focus { focused });
            }
            WindowEvent::CloseRequested => {
                self.closed = true;
                self.events.push(DeviceEvent::CloseRequested);
            }
            WindowEvent::Destroyed => self.destroyed = true,
            WindowEvent::Resized(size) => {
                self.width = size.width;
                self.height = size.height;
                self.events.push(DeviceEvent::Resized {
                    width: size.width,
                    height: size.height,
                });
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                if pressed {
                    if let Some(t) = event
                        .text
                        .as_ref()
                        .filter(|t| !t.chars().any(char::is_control))
                    {
                        self.events.push(DeviceEvent::Text {
                            text: t.to_string(),
                        });
                    }
                }
                if !event.repeat {
                    self.events.push(DeviceEvent::Key {
                        code: physical_key_name(&event.physical_key),
                        pressed,
                    });
                } else if pressed && physical_key_name(&event.physical_key) == "Backspace" {
                    // Held backspace keeps deleting in a note.
                    self.events.push(DeviceEvent::Key {
                        code: "Backspace".into(),
                        pressed: true,
                    });
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x as f32, position.y as f32);
                self.events.push(DeviceEvent::Cursor { at: self.cursor });
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.events.push(DeviceEvent::Mouse {
                    button: format!("{button:?}"),
                    pressed: state == ElementState::Pressed,
                    at: self.cursor,
                });
            }
            _ => {}
        }
    }
}

/// An open window and its event loop, pumped on demand.
pub struct WindowHost {
    /// Always `Some` until the host drops and parks it in `SPARE`.
    event_loop: Option<EventLoop<()>>,
    app: App,
}

impl WindowHost {
    /// Open a window. On Windows this may run on any thread; elsewhere the
    /// thread that opens it must keep pumping it.
    pub fn open(title: &str, width: u32, height: u32) -> Result<WindowHost> {
        Self::open_with(title, width, height, true)
    }

    /// Open a window, shown or not. A hidden one takes no focus and puts
    /// nothing on screen, which is what tests of the window's life want.
    pub fn open_with(title: &str, width: u32, height: u32, visible: bool) -> Result<WindowHost> {
        let event_loop = match SPARE.with(|s| s.borrow_mut().take()) {
            Some(l) => l,
            None => {
                let mut builder = EventLoop::builder();
                #[cfg(target_os = "windows")]
                {
                    use winit::platform::windows::EventLoopBuilderExtWindows;
                    builder.with_any_thread(true);
                }
                builder
                    .build()
                    .map_err(|e| ArgusError::new(crate::codes::WINDOW, e.to_string()))?
            }
        };
        let mut host = WindowHost {
            event_loop: Some(event_loop),
            app: App {
                title: title.to_string(),
                width,
                height,
                window: None,
                id: None,
                events: Vec::new(),
                cursor: (0.0, 0.0),
                closed: false,
                visible,
                destroyed: false,
            },
        };
        // The first pump creates the window (`resumed` or `about_to_wait`).
        for _ in 0..50 {
            host.pump();
            if host.app.window.is_some() || host.app.closed {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if host.app.window.is_none() {
            return Err(ArgusError::new(
                crate::codes::WINDOW,
                "the window did not open",
            ));
        }
        Ok(host)
    }

    /// Process pending window events without blocking. False once closed.
    pub fn pump(&mut self) -> bool {
        if let Some(l) = self.event_loop.as_mut() {
            let _ = l.pump_app_events(Some(Duration::ZERO), &mut self.app);
        }
        !self.app.closed
    }

    /// The window, once open.
    pub fn window(&self) -> Option<Arc<Window>> {
        self.app.window.clone()
    }

    /// Events since the last take.
    pub fn take_events(&mut self) -> Vec<DeviceEvent> {
        std::mem::take(&mut self.app.events)
    }

    /// True once the user closed the window.
    pub fn closed(&self) -> bool {
        self.app.closed
    }

    /// Current size in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.app.width, self.app.height)
    }

    /// True once the OS has destroyed the window.
    pub fn destroyed(&self) -> bool {
        self.app.destroyed
    }

    /// Ask the window to redraw (the renderer presents on its own; this
    /// keeps the OS happy about a live window).
    pub fn request_redraw(&self) {
        if let Some(w) = &self.app.window {
            w.request_redraw();
        }
    }
}

/// winit destroys a window by posting it a message when the last handle
/// goes, so the loop has to run after that. A loop dropped first left the
/// window on screen with nothing answering it, and Windows called the process
/// hung (found by the phase-1 gate: the close button froze the game). So the
/// host lets go of the window, pumps until it is gone, and parks the loop for
/// the next window. Drop whatever else holds the window, the renderer's
/// surface, before the host.
impl Drop for WindowHost {
    fn drop(&mut self) {
        let Some(mut l) = self.event_loop.take() else {
            return;
        };
        if let Some(w) = self.app.window.take() {
            let last = Arc::strong_count(&w) == 1;
            drop(w);
            for _ in 0..100 {
                if !last || self.app.destroyed {
                    break;
                }
                let _ = l.pump_app_events(Some(Duration::ZERO), &mut self.app);
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        SPARE.with(|s| *s.borrow_mut() = Some(l));
    }
}
