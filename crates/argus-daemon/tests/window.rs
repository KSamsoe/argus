//! The window's close button through the daemon, on hidden windows with no
//! audio, so nothing appears on screen or plays. The close button sends the
//! window `WM_CLOSE`; this sends the same message. Found by the phase-1 gate:
//! the close button left the window on screen, unanswered, and the game had
//! to be killed.
#![cfg(windows)]

mod common;

use argus_daemon::Daemon;
use common::*;
use serde_json::json;
use std::time::Duration;

#[link(name = "user32")]
extern "system" {
    fn FindWindowW(class: *const u16, title: *const u16) -> isize;
    fn PostMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> i32;
    fn IsWindow(hwnd: isize) -> i32;
}

const WM_CLOSE: u32 = 0x0010;

/// Open a hidden window with this title and find it; `None` without a GPU.
fn open_hidden(d: &mut Daemon, title: &str) -> Option<isize> {
    let params = json!({
        "open": true, "visible": false, "audio": false,
        "title": title, "width": 320, "height": 180,
    });
    match d.call(0, "render.window", &params) {
        Ok(_) => {}
        Err(e) if e.code == argus_daemon::codes::NO_ADAPTER => {
            eprintln!("skipping: {e}");
            return None;
        }
        Err(e) => panic!("the window did not open: {e}"),
    }
    let wide: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    let h = unsafe { FindWindowW(std::ptr::null(), wide.as_ptr()) };
    assert_ne!(h, 0, "no window titled {title}");
    Some(h)
}

/// Press the close button and run the daemon's frame polling until it
/// notices.
fn press_close(d: &mut Daemon, h: isize) {
    assert_ne!(unsafe { PostMessageW(h, WM_CLOSE, 0, 0) }, 0);
    for _ in 0..200 {
        d.poll_devices();
        if !d.has_window() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the daemon never saw the close button");
}

#[test]
fn the_close_button_closes_the_window_and_only_the_persons_window_stops_the_daemon() {
    let t = temp("window");
    project(t.path());
    let mut d = open(t.path());
    let title = format!("argus close test {}", std::process::id());

    // A window an agent opened: it closes, and the daemon carries on.
    let Some(h) = open_hidden(&mut d, &title) else {
        return;
    };
    press_close(&mut d, h);
    assert_eq!(
        unsafe { IsWindow(h) },
        0,
        "the window stayed after its close button"
    );
    assert!(!d.shutdown, "a window an agent opened only closes");
    let ev = ok(&mut d, "obs.events", json!({ "name": "window.closed" }));
    assert_eq!(ev["total"], 1, "{ev}");
    assert_eq!(ok(&mut d, "render.status", json!({}))["window"], false);

    // The same daemon opens another, and this time it is the person's, as
    // `argus daemon start --window` makes it: closing it stops the daemon.
    let h2 = open_hidden(&mut d, &title).expect("a second window");
    assert_ne!(h, h2);
    d.exit_on_window_close = true;
    press_close(&mut d, h2);
    assert_eq!(unsafe { IsWindow(h2) }, 0);
    assert!(d.shutdown, "closing the person's window stops the daemon");
}
