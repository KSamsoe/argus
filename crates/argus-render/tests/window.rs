//! The window's life, on hidden windows so nothing appears on screen. The
//! close button sends the window `WM_CLOSE`; this sends the same message.
//! Found by the phase-1 gate: after the close button the window stayed on
//! screen, unanswered, and the game had to be killed. And winit allows one
//! event loop per process, so a second window used to fail to open.
#![cfg(windows)]

use argus_render::window::{DeviceEvent, WindowHost};
use argus_render::Renderer;
use std::time::Duration;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

#[link(name = "user32")]
extern "system" {
    fn PostMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> i32;
    fn IsWindow(hwnd: isize) -> i32;
}

const WM_CLOSE: u32 = 0x0010;

fn hwnd(host: &WindowHost) -> isize {
    let w = host.window().expect("open");
    match w.window_handle().expect("handle").as_raw() {
        RawWindowHandle::Win32(h) => h.hwnd.get(),
        other => panic!("not a Win32 window: {other:?}"),
    }
}

/// Send the close button's message and pump until the host sees it.
fn press_close(host: &mut WindowHost) -> Vec<DeviceEvent> {
    assert_ne!(unsafe { PostMessageW(hwnd(host), WM_CLOSE, 0, 0) }, 0);
    let mut seen = Vec::new();
    for _ in 0..200 {
        host.pump();
        seen.extend(host.take_events());
        if host.closed() {
            return seen;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the close request never arrived");
}

/// One test, because the loop is kept per thread, the way the daemon's one
/// thread uses it.
#[test]
fn a_closed_window_is_destroyed_and_another_can_open() {
    // A window with a renderer drawing into it, as the daemon has.
    let mut host = WindowHost::open_with("argus test", 320, 180, false).expect("window");
    let first = hwnd(&host);
    let r = match Renderer::windowed(host.window().expect("open"), false) {
        Ok(r) => Some(r),
        Err(e) if e.code == argus_render::codes::NO_ADAPTER => {
            eprintln!("no adapter: the window is closed without a renderer");
            None
        }
        Err(e) => panic!("the renderer did not start: {e}"),
    };
    let seen = press_close(&mut host);
    assert!(
        seen.iter()
            .any(|e| matches!(e, DeviceEvent::CloseRequested)),
        "{seen:?}"
    );
    assert_eq!(unsafe { IsWindow(first) }, 1, "closing is the host's job");
    // The daemon's order: the renderer's surface first, then the host.
    drop(r);
    drop(host);
    assert_eq!(
        unsafe { IsWindow(first) },
        0,
        "the window outlived its host"
    );

    // A second window on the same loop opens, hears its close button, and
    // goes too.
    let mut host = WindowHost::open_with("argus test 2", 320, 180, false).expect("second window");
    let second = hwnd(&host);
    assert_ne!(first, second);
    press_close(&mut host);
    drop(host);
    assert_eq!(unsafe { IsWindow(second) }, 0);
}
