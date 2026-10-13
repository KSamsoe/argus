//! Phase 2 item 7 through the protocol: a player's report (the replay file
//! a release build or the web page saves when the report key is pressed)
//! becomes a mark with `mark.import`, and `mark.open` reproduces the world
//! the player saw; `mark.pull` fetches the reports browsers posted to a
//! playtest server, each once (§15 "a player bug report becomes a mark").

mod common;

use common::*;
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let name = e.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let (a, b) = (e.path(), to.join(&name));
        if a.is_dir() {
            copy(&a, &b);
        } else {
            std::fs::copy(&a, &b).unwrap();
        }
    }
}

/// A report from forty seconds of Hollow in a player.
fn report(root: &Path, note: &str) -> (Vec<u8>, String) {
    let mut p = argus_player::Player::open(root, None, None).unwrap();
    p.record();
    for s in 0..40u64 {
        let dir = if (s / 4) % 2 == 0 { 1 } else { -1 };
        p.inject("move", Some(&json!([dir, 0])), Some(50), false)
            .unwrap();
        if s % 3 == 0 {
            p.inject("jump", Some(&json!(true)), Some(8), false)
                .unwrap();
        }
        p.step(60).unwrap();
    }
    let digest = format!("{:016x}", p.digest());
    (p.report(note, "report", "test").unwrap(), digest)
}

/// Serve `/marks` and `/marks/<id>` from memory, for `n` requests.
fn server(files: Vec<(String, Vec<u8>)>, n: usize) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for s in l.incoming().take(n) {
            let mut s = s.unwrap();
            let mut req = Vec::new();
            let mut buf = [0u8; 512];
            while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                let k = s.read(&mut buf).unwrap();
                if k == 0 {
                    break;
                }
                req.extend_from_slice(&buf[..k]);
            }
            let req = String::from_utf8_lossy(&req).to_string();
            let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
            let body: Vec<u8> = if path == "/marks" {
                let rows: Vec<_> = files
                    .iter()
                    .map(|(id, b)| json!({ "id": id, "bytes": b.len() }))
                    .collect();
                json!({ "marks": rows }).to_string().into_bytes()
            } else {
                let id = path.trim_start_matches("/marks/");
                files.iter().find(|(i, _)| i == id).unwrap().1.clone()
            };
            // Chunked, as Node sends a body it streams.
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\ncontent-type: application/octet-stream\r\ntransfer-encoding: chunked\r\n\r\n{:x}\r\n",
                body.len()
            );
            let _ = s.write_all(&body);
            let _ = s.write_all(b"\r\n0\r\n\r\n");
        }
    });
    url
}

#[test]
fn a_report_is_a_mark_that_reproduces_and_pull_fetches_each_once() {
    let t = temp("reports");
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hollow"),
        t.path(),
    );
    let (bytes, digest) = report(t.path(), "the jump felt late");
    let file = t.path().join("report.argr");
    std::fs::write(&file, &bytes).unwrap();

    let mut d = open(t.path());
    let m = ok(&mut d, "mark.import", json!({ "path": "report.argr" }));
    assert_eq!(m["id"], 1, "{m}");
    let got = ok(&mut d, "mark.get", json!({ "id": 1 }));
    assert_eq!(got["source"], "report", "{got}");
    assert_eq!(got["note"], "the jump felt late");
    assert_eq!(got["frame"], 2400);
    let o = ok(&mut d, "mark.open", json!({ "id": 1 }));
    assert_eq!(o["digest"]["same"], true, "{o}");
    assert_eq!(o["digest"]["now"], digest.as_str());
    // Issue 15: the same report again is the same mark, and replay.open
    // takes the report by its own name and compares its digest.
    let again = ok(&mut d, "mark.import", json!({ "path": "report.argr" }));
    assert_eq!(
        (again["id"].clone(), again["already"].clone()),
        (json!(1), json!(true)),
        "{again}"
    );
    let r = ok(&mut d, "replay.open", json!({ "path": "report.argr" }));
    assert_eq!(r["digest"]["saved"], digest.as_str(), "{}", r["digest"]);
    assert_eq!(r["digest"]["same"], true, "{}", r["digest"]);

    // Two reports on a playtest server: pulled once each. The server answers
    // the first pull's list and two files, then the second pull's list.
    let (b2, _) = report(t.path(), "fell through the plank");
    let url = server(vec![("100-aa".into(), bytes), ("200-bb".into(), b2)], 4);
    let r = ok(&mut d, "mark.pull", json!({ "url": url }));
    assert_eq!(r["pulled"].as_array().unwrap().len(), 2, "{r}");
    assert_eq!(r["already"], 0);
    let list = ok(&mut d, "mark.list", json!({}));
    let notes: Vec<&str> = list["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["note"].as_str())
        .collect();
    assert!(notes.contains(&"fell through the plank"), "{list}");
    let r = ok(&mut d, "mark.pull", json!({ "url": url }));
    assert_eq!(r["pulled"].as_array().unwrap().len(), 0, "{r}");
    assert_eq!(r["already"], 2);
}
