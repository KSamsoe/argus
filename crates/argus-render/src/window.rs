//! The windowed target's host: a winit window pumped from the daemon
//! thread, never blocking it. Device events are collected here and read
//! by the input layer (phase 1, item 2); this module only opens, pumps,
//! resizes, and closes.

use argus_ecs::{ArgusError, Result};
use std::sync::Arc;
use std::time::Duration;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
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
    /// The window gained or lost keyboard focus.
    Focus {
        /// Focused now.
        focused: bool,
    },
}

struct App {
    title: String,
    width: u32,
    height: u32,
    window: Option<Arc<Window>>,
    events: Vec<DeviceEvent>,
    cursor: (f32, f32),
    closed: bool,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(self.title.clone())
            .with_inner_size(winit::dpi::LogicalSize::new(self.width, self.height));
        match el.create_window(attrs) {
            Ok(w) => self.window = Some(Arc::new(w)),
            Err(_) => self.closed = true,
        }
    }

    fn window_event(&mut self, _el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::Focused(focused) => {
                self.events.push(DeviceEvent::Focus { focused });
            }
            WindowEvent::CloseRequested => {
                self.closed = true;
                self.events.push(DeviceEvent::CloseRequested);
            }
            WindowEvent::Resized(size) => {
                self.width = size.width;
                self.height = size.height;
                self.events.push(DeviceEvent::Resized {
                    width: size.width,
                    height: size.height,
                });
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if !event.repeat {
                    self.events.push(DeviceEvent::Key {
                        code: format!("{:?}", event.physical_key)
                            .trim_start_matches("Code(")
                            .trim_end_matches(')')
                            .to_string(),
                        pressed: event.state == ElementState::Pressed,
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
    event_loop: EventLoop<()>,
    app: App,
}

impl WindowHost {
    /// Open a window. On Windows this may run on any thread; elsewhere the
    /// thread that opens it must keep pumping it.
    pub fn open(title: &str, width: u32, height: u32) -> Result<WindowHost> {
        let mut builder = EventLoop::builder();
        #[cfg(target_os = "windows")]
        {
            use winit::platform::windows::EventLoopBuilderExtWindows;
            builder.with_any_thread(true);
        }
        let event_loop = builder
            .build()
            .map_err(|e| ArgusError::new(crate::codes::WINDOW, e.to_string()))?;
        let mut host = WindowHost {
            event_loop,
            app: App {
                title: title.to_string(),
                width,
                height,
                window: None,
                events: Vec::new(),
                cursor: (0.0, 0.0),
                closed: false,
            },
        };
        // The first pump delivers `resumed`, which creates the window.
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
        let _ = self
            .event_loop
            .pump_app_events(Some(Duration::ZERO), &mut self.app);
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

    /// Ask the window to redraw (the renderer presents on its own; this
    /// keeps the OS happy about a live window).
    pub fn request_redraw(&self) {
        if let Some(w) = &self.app.window {
            w.request_redraw();
        }
    }
}
