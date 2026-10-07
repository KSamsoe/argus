//! Section 15 (phase 5 item 1): a desktop player's reports reach the team.
//! The reports waiting in a save location go to the project's playtest
//! server as the web page's do (`POST <url>marks`), oldest first, and move
//! to `sent/`; with the server away they wait. A project names its server
//! in `argus.toml`, by an address plain HTTP reaches.

use argus_player::reports;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A temp directory removed on drop, used as its path.
struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for Temp {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn temp(name: &str) -> Temp {
    let dir = std::env::temp_dir().join(format!("argus-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Temp(dir)
}

/// Each request a server took: its request line and its body.
type Taken = Vec<(String, Vec<u8>)>;

/// A server that takes `n` requests.
fn server(n: usize) -> (String, std::thread::JoinHandle<Taken>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", l.local_addr().unwrap());
    let h = std::thread::spawn(move || {
        let mut got = Vec::new();
        for _ in 0..n {
            let (mut s, _) = l.accept().unwrap();
            let mut raw = Vec::new();
            let mut buf = [0u8; 4096];
            let body_at = loop {
                let k = s.read(&mut buf).unwrap();
                raw.extend_from_slice(&buf[..k]);
                if let Some(i) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    break i + 4;
                }
            };
            let head = String::from_utf8_lossy(&raw[..body_at]).to_string();
            let len: usize = head
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse().unwrap())
                })
                .unwrap_or(0);
            while raw.len() < body_at + len {
                let k = s.read(&mut buf).unwrap();
                raw.extend_from_slice(&buf[..k]);
            }
            got.push((
                head.lines().next().unwrap().to_string(),
                raw[body_at..].to_vec(),
            ));
            s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
        }
        got
    });
    (url, h)
}

#[test]
fn waiting_reports_go_to_the_server_and_wait_while_it_is_away() {
    let t = temp("release-reports");
    let dir = t.join("reports");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("100-report.argr"), b"ARGR first").unwrap();
    std::fs::write(dir.join("200-crash.argr"), b"ARGR second").unwrap();
    std::fs::write(dir.join("notes.txt"), b"not a report").unwrap();

    // The server away: nothing goes, nothing moves.
    let away = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}/", l.local_addr().unwrap())
    };
    let r = reports::send_waiting(&dir, &away, Duration::from_secs(2));
    assert!(r.sent.is_empty() && r.failed.is_some(), "{r:?}");
    assert_eq!(reports::waiting(&dir).len(), 2);

    // Back: both go, oldest first, and are kept in sent/.
    let (url, got) = server(2);
    let r = reports::send_waiting(&dir, &url, Duration::from_secs(5));
    assert!(r.failed.is_none(), "{r:?}");
    assert_eq!(r.sent.len(), 2);
    let got = got.join().unwrap();
    assert_eq!(
        got[0],
        ("POST /marks HTTP/1.1".to_string(), b"ARGR first".to_vec())
    );
    assert_eq!(got[1].1, b"ARGR second");
    assert!(reports::waiting(&dir).is_empty());
    assert!(dir.join("sent/100-report.argr").is_file());
    assert!(dir.join("notes.txt").is_file(), "only reports");
}

#[test]
fn a_project_names_its_server_by_an_address_plain_http_reaches() {
    let t = temp("release-reports-config");
    let cfg = |reports: &str| {
        std::fs::write(
            t.join("argus.toml"),
            format!("name = \"g\"\nclock = \"continuous@60\"\n\n[reports]\n{reports}\n"),
        )
        .unwrap();
        argus_scene::ProjectConfig::load(&t)
    };
    assert_eq!(
        cfg("url = \"http://100.70.114.55:8765\"")
            .unwrap()
            .reports_url
            .as_deref(),
        Some("http://100.70.114.55:8765/")
    );
    let e = cfg("url = \"https://klab.example:8443/\"").unwrap_err();
    assert!(e.message.contains("http://"), "{}", e.message);
}
