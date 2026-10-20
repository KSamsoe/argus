//! The daemon on the web (phase 5 item 10): the web client's page, the
//! protocol over a WebSocket with a scoped token (§5: "Any remote access
//! ... goes through a token issued by `argus token` with a scope (observe,
//! review, edit) and an expiry"), files for a token's holder, and what is
//! refused: no token, a wrong host or origin, a call the scope does not
//! allow, the daemon's own calls, a path outside the project.

use argus_daemon::{server, Daemon, Transport};
use interprocess::local_socket::prelude::*;
use serde_json::{json, Value as Json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn project(tag: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("web-{tag}"));
    let _ = std::fs::remove_dir_all(&root);
    write(
        &root,
        "argus.toml",
        "name = \"webby\"\nclock = \"continuous@60\"\ndefault_scene = \"s\"\nplugins = []\n",
    );
    write(
        &root,
        "scenes/s/scene.toml",
        "clock = \"continuous@60\"\nseed = 1\n",
    );
    write(
        &root,
        "scenes/s/camera.toml",
        "[Transform]\n\n[Camera]\nheight = 10.0\n",
    );
    write(
        &root,
        "scenes/s/box.toml",
        "[Transform]\npos = [1.0, 2.0]\n\n[Sprite]\nkind = \"rect\"\ncolor = \"#E8A33D\"\nsize = [2.0, 2.0]\n",
    );
    root
}

struct Web {
    root: PathBuf,
    port: u16,
    local: (
        BufReader<interprocess::local_socket::RecvHalf>,
        interprocess::local_socket::SendHalf,
    ),
    next: u64,
}

fn start(tag: &str) -> Web {
    let root = project(tag);
    let name = format!("{}-webtest", server::socket_name_for(&root));
    let (r2, n2) = (root.clone(), name.clone());
    std::thread::spawn(move || {
        let mut d = Daemon::new();
        d.call(
            0,
            "session.open",
            &json!({ "project": r2.canonicalize().unwrap().to_string_lossy() }),
        )
        .unwrap();
        let _ = server::serve_with(
            d,
            Transport::Socket(n2),
            Some(&r2),
            Some("127.0.0.1:0".parse().unwrap()),
        );
    });
    let mut stream = None;
    for _ in 0..100 {
        if let Ok(s) = server::connect(&name) {
            stream = Some(s);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let (recv, send) = stream.expect("the daemon's socket").split();
    // The daemon writes daemon.json just after its socket listens, so a
    // client can connect first (a CI run under load did).
    let mut info = Json::Null;
    for _ in 0..100 {
        if let Some(j) = std::fs::read_to_string(root.join(".argus/daemon.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<Json>(&s).ok())
        {
            info = j;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let web = info["web"]
        .as_str()
        .expect("daemon.json names the web client");
    let port = web
        .trim_end_matches('/')
        .rsplit(':')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    Web {
        root,
        port,
        local: (BufReader::new(recv), send),
        next: 1,
    }
}

impl Web {
    /// A call over the local socket (this machine's client).
    fn local(&mut self, method: &str, params: Json) -> Json {
        let id = self.next;
        self.next += 1;
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.local
            .1
            .write_all(format!("{line}\n").as_bytes())
            .unwrap();
        loop {
            let mut s = String::new();
            self.local.0.read_line(&mut s).unwrap();
            let j: Json = serde_json::from_str(&s).unwrap();
            if j["id"] == id {
                return j;
            }
        }
    }

    fn token(&mut self, scope: &str, hours: f64) -> String {
        let r = self.local(
            "token.issue",
            json!({ "scope": scope, "hours": hours, "label": scope }),
        );
        assert!(
            r["result"]["url"].as_str().unwrap().contains("/#t=argus-"),
            "{r}"
        );
        r["result"]["token"].as_str().unwrap().to_string()
    }

    fn http(&self, req: &str) -> (String, Vec<u8>) {
        let mut s = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        s.write_all(req.as_bytes()).unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).unwrap();
        let split = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        (
            String::from_utf8_lossy(&out[..split]).into_owned(),
            out[split + 4..].to_vec(),
        )
    }

    fn get(&self, path: &str, extra: &str) -> (String, Vec<u8>) {
        self.http(&format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{extra}\r\n",
            self.port
        ))
    }

    /// A WebSocket with a token, or the handshake's answer when refused.
    fn ws(&self, token: &str, extra: &str) -> Result<Ws, String> {
        let mut s = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        write!(
            s,
            "GET /ws HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Protocol: argus, t.{token}\r\n{extra}\r\n",
            self.port
        )
        .unwrap();
        let mut head = Vec::new();
        let mut b = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            s.read_exact(&mut b).unwrap();
            head.push(b[0]);
        }
        let head = String::from_utf8_lossy(&head).into_owned();
        if !head.starts_with("HTTP/1.1 101") {
            return Err(head);
        }
        assert!(
            head.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="),
            "{head}"
        );
        Ok(Ws { s, next: 1 })
    }
}

#[derive(Debug)]
struct Ws {
    s: TcpStream,
    next: u64,
}

impl Ws {
    fn send(&mut self, text: &str) {
        let data = text.as_bytes();
        let mask = [0x12u8, 0x34, 0x56, 0x78];
        let mut f = vec![0x81u8];
        if data.len() < 126 {
            f.push(0x80 | data.len() as u8);
        } else {
            f.push(0x80 | 126);
            f.extend_from_slice(&(data.len() as u16).to_be_bytes());
        }
        f.extend_from_slice(&mask);
        f.extend(data.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        self.s.write_all(&f).unwrap();
    }

    fn recv(&mut self) -> Json {
        let mut h = [0u8; 2];
        self.s.read_exact(&mut h).unwrap();
        let mut len = (h[1] & 0x7f) as usize;
        if len == 126 {
            let mut b = [0u8; 2];
            self.s.read_exact(&mut b).unwrap();
            len = u16::from_be_bytes(b) as usize;
        } else if len == 127 {
            let mut b = [0u8; 8];
            self.s.read_exact(&mut b).unwrap();
            len = u64::from_be_bytes(b) as usize;
        }
        let mut d = vec![0u8; len];
        self.s.read_exact(&mut d).unwrap();
        serde_json::from_slice(&d).unwrap()
    }

    fn call(&mut self, method: &str, params: Json) -> Json {
        let id = self.next;
        self.next += 1;
        self.send(
            &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string(),
        );
        loop {
            let m = self.recv();
            if m["id"] == id {
                return m;
            }
        }
    }
}

#[test]
fn the_page_is_served_and_a_token_opens_the_protocol() {
    let mut w = start("page");
    let (head, body) = w.get("/", "");
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(head.contains("Content-Security-Policy"), "{head}");
    assert!(String::from_utf8_lossy(&body).contains("<title>Argus</title>"));
    let t = w.token("observe", 1.0);
    let mut ws = w.ws(&t, "").unwrap();
    // First what the connection may do, then answers.
    let hello = ws.recv();
    assert_eq!(hello["method"], "web.hello");
    assert_eq!(hello["params"]["scope"], "observe");
    let st = ws.call("world.summary", json!({}));
    assert_eq!(st["result"]["scene"], "s", "{st}");
    // Its calls are the change feed's under its label.
    let list = w.local("token.list", json!({}));
    assert_eq!(list["result"]["rows"][0]["scope"], "observe");
    assert!(
        !list.to_string().contains(&t),
        "the token itself is never listed"
    );
    let kept = std::fs::read_to_string(w.root.join(".argus/tokens.json")).unwrap();
    assert!(!kept.contains(&t), "only its hash is kept");
}

#[test]
fn what_is_refused_is_refused_before_the_daemon_hears_it() {
    let mut w = start("refused");
    let observe = w.token("observe", 1.0);
    let review = w.token("review", 1.0);
    let edit = w.token("edit", 1.0);
    // No token, a wrong one, another host, another page's origin.
    assert!(w
        .ws("argus-0000", "")
        .unwrap_err()
        .starts_with("HTTP/1.1 401"));
    let (head, _) = w.http(&format!(
        "GET / HTTP/1.1\r\nHost: evil.example.com:{}\r\n\r\n",
        w.port
    ));
    assert!(head.starts_with("HTTP/1.1 403"), "{head}");
    let o = w
        .ws(&observe, "Origin: http://evil.example.com\r\n")
        .unwrap_err();
    assert!(o.starts_with("HTTP/1.1 403"), "{o}");
    // Scopes.
    let mut ws = w.ws(&observe, "").unwrap();
    ws.recv();
    let r = ws.call("review.answer", json!({ "id": 1, "answer": "yes" }));
    assert_eq!(r["error"]["code"], 4717, "{r}");
    assert!(
        r["error"]["message"].as_str().unwrap().contains("observe"),
        "{r}"
    );
    let mut ws = w.ws(&review, "").unwrap();
    ws.recv();
    let r = ws.call("edit.begin", json!({ "author": "x" }));
    assert_eq!(
        r["error"]["code"], 4717,
        "a review token does not edit: {r}"
    );
    // The daemon's own, whatever the scope.
    let mut ws = w.ws(&edit, "").unwrap();
    ws.recv();
    for m in [
        "token.issue",
        "session.shutdown",
        "render.stream",
        "session.open",
    ] {
        let r = ws.call(m, json!({}));
        assert_eq!(r["error"]["code"], 4717, "{m}: {r}");
    }
    // A path outside the project.
    let r = ws.call(
        "sim.save",
        json!({ "name": "x", "path": "../outside.save" }),
    );
    assert_eq!(r["error"]["code"], 4717, "{r}");
    assert!(!w.root.join("../outside.save").exists());
    // An edit token edits.
    let r = ws.call("sim.step", json!({ "n": 2 }));
    assert!(r["error"].is_null(), "{r}");
    // Revoked: the next connection is refused.
    let id = w.local("token.list", json!({}))["result"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["label"] == "edit")
        .unwrap()["id"]
        .clone();
    w.local("token.revoke", json!({ "id": id }));
    assert!(w.ws(&edit, "").unwrap_err().starts_with("HTTP/1.1 401"));
    // And the connection already open is told, then closed, within a second.
    let t = std::time::Instant::now();
    let said = ws.recv();
    assert_eq!(said["method"], "web.expired", "{said}");
    assert_eq!(said["params"]["why"], "revoked", "{said}");
    let mut rest = [0u8; 16];
    assert_eq!(ws.s.read(&mut rest).unwrap_or(0), 0, "closed");
    assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
    // Expired: refused too.
    let short = w.token("observe", 0.0002);
    std::thread::sleep(Duration::from_millis(1200));
    assert!(w.ws(&short, "").unwrap_err().starts_with("HTTP/1.1 401"));
}

#[test]
fn a_frame_and_a_clip_come_as_files_for_a_tokens_holder() {
    let mut w = start("files");
    let t = w.token("review", 1.0);
    let mut ws = w.ws(&t, "").unwrap();
    ws.recv();
    let f = ws.call(
        "obs.frame",
        json!({ "format": "png", "width": 160, "height": 90, "software": true, "path": ".argus/frames/web-test.png" }),
    );
    if f["error"]["code"] == 5001 {
        return; // no adapter at all on this machine
    }
    assert!(f["error"].is_null(), "{f}");
    let (head, _) = w.get("/file/frames/web-test.png", "");
    assert!(head.starts_with("HTTP/1.1 401"), "{head}");
    let (head, body) = w.get(
        "/file/frames/web-test.png",
        &format!("Authorization: Bearer {t}\r\n"),
    );
    assert!(
        head.starts_with("HTTP/1.1 200") && head.contains("image/png"),
        "{head}"
    );
    assert_eq!(&body[1..4], b"PNG");
    // Only what the protocol names, and only below .argus/.
    for p in [
        "/file/tokens.json",
        "/file/frames/../tokens.json",
        "/file/frames/%2e%2e/tokens.json",
    ] {
        let (head, _) = w.get(p, &format!("Authorization: Bearer {t}\r\n"));
        assert!(head.starts_with("HTTP/1.1 403"), "{p}: {head}");
    }
}
