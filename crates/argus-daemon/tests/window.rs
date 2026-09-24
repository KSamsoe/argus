//! The window through the daemon, on hidden windows with no audio, so
//! nothing appears on screen or plays: its close button (found by the
//! phase-1 gate: the close button left the window on screen, unanswered,
//! and the game had to be killed), and F8 with a typed note (phase 2 item
//! 2). A button or a key sends the window a message; these send the same
//! messages.
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
const WM_KEYDOWN: u32 = 0x0100;
const WM_KEYUP: u32 = 0x0101;
const WM_CHAR: u32 = 0x0102;

/// A key press and release as the keyboard sends them: virtual key, scan
/// code, and the character it types, if any.
fn tap(h: isize, vk: usize, scan: isize, ch: Option<char>) {
    let down = 1 | (scan << 16);
    let up = down | (1 << 30) | (1 << 31);
    unsafe {
        assert_ne!(PostMessageW(h, WM_KEYDOWN, vk, down), 0);
        if let Some(c) = ch {
            PostMessageW(h, WM_CHAR, c as usize, down);
        }
        PostMessageW(h, WM_KEYUP, vk, up);
    }
}

/// Type lowercase letters and spaces (a US layout's scan codes).
fn type_text(h: isize, s: &str) {
    const ROWS: [(&str, isize); 3] = [("qwertyuiop", 0x10), ("asdfghjkl", 0x1E), ("zxcvbnm", 0x2C)];
    for c in s.chars() {
        let (vk, scan) = if c == ' ' {
            (0x20, 0x39)
        } else {
            let (row, first) = ROWS
                .iter()
                .find(|(r, _)| r.contains(c))
                .unwrap_or_else(|| panic!("no key for {c:?} here"));
            let at = row.find(c).unwrap() as isize;
            (c.to_ascii_uppercase() as usize, first + at)
        };
        tap(h, vk, scan, Some(c));
    }
}

const F8: (usize, isize) = (0x77, 0x42);
const ENTER: (usize, isize) = (0x0D, 0x1C);
const ESCAPE: (usize, isize) = (0x1B, 0x01);

/// Run the daemon's frame loop (poll, present) until `done` says so.
fn frames_until(d: &mut Daemon, what: &str, mut done: impl FnMut(&mut Daemon) -> bool) {
    for _ in 0..300 {
        d.poll_devices();
        d.present();
        if done(d) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the daemon never got to: {what}");
}

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

/// One test, both parts in order: winit allows one event loop a process,
/// and it lives on the thread that made it, while tests run on threads of
/// their own.
#[test]
fn the_window() {
    f8_drops_a_mark_and_the_note_typed_after_it_is_kept();
    a_project_can_mark_with_another_key();
    the_close_button_closes_the_window_and_only_the_persons_window_stops_the_daemon();
}

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

fn f8_drops_a_mark_and_the_note_typed_after_it_is_kept() {
    let t = temp("window-marks");
    project(t.path());
    let mut d = open(t.path());
    let title = format!("argus mark test {}", std::process::id());
    let Some(h) = open_hidden(&mut d, &title) else {
        return;
    };
    let bar = |d: &mut Daemon| ok(d, "render.status", json!({}))["bar"].clone();
    ok(&mut d, "sim.play", json!({}));
    ok(&mut d, "sim.step", json!({ "n": 90 }));
    assert!(bar(&mut d).is_null() || bar(&mut d).as_str().unwrap().contains("F8"));

    // F8: a mark at this frame, the game paused, the prompt on the bar.
    tap(h, F8.0, F8.1, None);
    frames_until(&mut d, "a mark", |d| {
        ok(d, "mark.list", json!({}))["open"] == 1
    });
    let st = ok(&mut d, "sim.status", json!({}));
    assert_eq!(st["paused"], true, "{st}");
    let m = ok(&mut d, "mark.get", json!({ "id": 1 }));
    assert_eq!(m["frame"], 90);
    assert_eq!(m["author"], "human");

    // What is typed goes to the note, not the game; Enter keeps it.
    type_text(h, "dash feels late");
    frames_until(&mut d, "the note on the bar", |d| {
        bar(d)
            .as_str()
            .is_some_and(|b| b.contains("NOTE: dash feels late_"))
    });
    assert_eq!(
        ok(&mut d, "input.devices", json!({}))["keys"],
        json!([]),
        "the game heard no keys"
    );
    tap(h, ENTER.0, ENTER.1, None);
    frames_until(&mut d, "the note kept", |d| {
        ok(d, "mark.get", json!({ "id": 1 }))["note"] == "dash feels late"
    });
    assert_eq!(
        ok(&mut d, "sim.status", json!({}))["paused"],
        false,
        "play goes on"
    );
    assert!(bar(&mut d)
        .as_str()
        .unwrap()
        .contains("MARK 1 SAVED: dash feels late"));
    let noted = ok(&mut d, "obs.events", json!({ "name": "mark.noted" }));
    assert_eq!(noted["total"], 1, "{noted}");

    // Escape leaves a mark without a note.
    tap(h, F8.0, F8.1, None);
    frames_until(&mut d, "a second mark", |d| {
        ok(d, "mark.list", json!({}))["open"] == 2
    });
    type_text(h, "lost");
    tap(h, ESCAPE.0, ESCAPE.1, None);
    frames_until(&mut d, "the prompt gone", |d| {
        bar(d)
            .as_str()
            .is_some_and(|b| b.starts_with("MARK 2 SAVED"))
    });
    assert_eq!(ok(&mut d, "mark.get", json!({ "id": 2 }))["note"], "");

    // Closing the window with a note half typed keeps what was typed.
    tap(h, F8.0, F8.1, None);
    frames_until(&mut d, "a third mark", |d| {
        ok(d, "mark.list", json!({}))["open"] == 3
    });
    type_text(h, "the ledge");
    frames_until(&mut d, "the typing", |d| {
        bar(d)
            .as_str()
            .is_some_and(|b| b.contains("NOTE: the ledge_"))
    });
    press_close(&mut d, h);
    assert_eq!(
        ok(&mut d, "mark.get", json!({ "id": 3 }))["note"],
        "the ledge"
    );
}

fn a_project_can_mark_with_another_key() {
    let t = temp("window-mark-key");
    project(t.path());
    let cfg = read(t.path(), "argus.toml")
        + "
[marks]
key = \"Backquote\"
";
    write(t.path(), "argus.toml", &cfg);
    let mut d = open(t.path());
    let title = format!("argus mark key test {}", std::process::id());
    let Some(h) = open_hidden(&mut d, &title) else {
        return;
    };
    ok(&mut d, "sim.play", json!({}));
    tap(h, F8.0, F8.1, None);
    for _ in 0..20 {
        d.poll_devices();
        d.present();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        ok(&mut d, "mark.list", json!({}))["open"],
        0,
        "F8 is the game's here"
    );
    tap(h, 0xC0, 0x29, Some('`'));
    frames_until(&mut d, "a mark from the backquote key", |d| {
        ok(d, "mark.list", json!({}))["open"] == 1
    });
    tap(h, ESCAPE.0, ESCAPE.1, None);
    frames_until(&mut d, "the prompt gone", |d| {
        ok(d, "sim.status", json!({}))["paused"] == false
    });
    assert_eq!(
        ok(&mut d, "mark.get", json!({ "id": 1 }))["note"],
        "",
        "the key typed no note"
    );
    press_close(&mut d, h);
}
