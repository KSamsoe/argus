//! The daemon on the web (phase 5 item 10, the browser editor spike): §11's
//! "a browser-based editor (the daemon serving a web UI) ... reachable from
//! a tablet during a playtest, which is attractive for review requests",
//! and §5's remote access "through a token issued by `argus token` with a
//! scope (observe, review, edit) and an expiry".
//!
//! `argus daemon start --web` (or `--web tailnet`, or an address) listens
//! for HTTP beside the local socket. The page at `/` is the web client
//! (`index.html`). `/ws` is the protocol over a WebSocket: one JSON-RPC
//! message a text frame, each socket a connection like a local client's,
//! notifications included. `/file/...` serves what the protocol names on
//! disk (review clips, marks, frames) to a holder of a token. A browser has
//! no local socket and no shared memory, so a frame is a picture the page
//! asks for (`obs.frame`) and fetches.
//!
//! Trust, as decided with Kevin (2026-10-08): the daemon listens on this
//! machine (loopback) or its tailnet address (Tailscale's 100.64.0.0/10),
//! whose traffic WireGuard encrypts, and nowhere else; TLS is for a
//! teammate over the internet, later. A token travels in the WebSocket's
//! subprotocol (`t.<token>`) and a file request's `Authorization` header,
//! never a URL's query, and the page takes it from the address's fragment,
//! which a browser never sends. The `Host` header must name this machine
//! (DNS rebinding), and a WebSocket's `Origin` must be the page's. Every
//! call is checked against the token's scope before the daemon hears it;
//! a path a call names must stay inside the project.

pub use argus_relay::hash;
pub mod tokens;

use crate::daemon::ConnId;
use crate::server::Msg;
use serde_json::{json, Value as Json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::time::Duration;
use tokens::Scope;

/// Web connections' ids start here, apart from the local socket's.
pub const WEB_CONN_BASE: ConnId = 1 << 40;

/// The port when none is given.
pub const DEFAULT_PORT: u16 = 7878;

/// The web client.
pub const PAGE: &str = include_str!("index.html");

/// The largest message the daemon reads from a browser.
const MAX_MESSAGE: usize = 1 << 20;

pub use argus_relay::addr::{allowed, tailnet_ip};

/// `--web`'s value as an address: empty or `local` is this machine,
/// `tailnet` its tailnet address, else `ip`, `ip:port`, or `:port`.
pub fn parse_addr(s: &str) -> Result<SocketAddr, String> {
    let s = s.trim();
    let local = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), DEFAULT_PORT);
    let addr = match s {
        "" | "local" | "localhost" => local,
        "tailnet" => SocketAddr::new(
            tailnet_ip().ok_or("this machine has no tailnet address (is Tailscale running?)")?,
            DEFAULT_PORT,
        ),
        _ => {
            if let Some(p) = s.strip_prefix(':') {
                SocketAddr::new(
                    local.ip(),
                    p.parse().map_err(|_| format!("{s:?}: no port"))?,
                )
            } else if let Ok(a) = s.parse::<SocketAddr>() {
                a
            } else if let Ok(ip) = s.parse::<IpAddr>() {
                SocketAddr::new(ip, DEFAULT_PORT)
            } else {
                return Err(format!(
                    "{s:?} is not an address: local, tailnet, an ip, or ip:port"
                ));
            }
        }
    };
    if !allowed(addr.ip()) {
        return Err(format!(
            "{} is not this machine or its tailnet: the daemon serves the web only there (Tailscale encrypts the tailnet; TLS for anywhere else is not built)",
            addr.ip()
        ));
    }
    Ok(addr)
}

/// What the web listener shares with its connections.
struct Ctx {
    addr: SocketAddr,
    root: PathBuf,
    tx: Sender<Msg>,
    next: AtomicU64,
}

/// Listen on `addr` for the daemon of the project at `root`; the address
/// bound (port 0 picks one).
pub(crate) fn start(
    addr: SocketAddr,
    root: PathBuf,
    tx: Sender<Msg>,
) -> std::io::Result<SocketAddr> {
    if !allowed(addr.ip()) {
        return Err(std::io::Error::other(format!(
            "{} is not this machine or its tailnet",
            addr.ip()
        )));
    }
    let listener = TcpListener::bind(addr)?;
    let bound = listener.local_addr()?;
    let ctx = Arc::new(Ctx {
        addr: bound,
        root,
        tx,
        next: AtomicU64::new(0),
    });
    std::thread::spawn(move || {
        for s in listener.incoming() {
            let Ok(s) = s else { continue };
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = connection(s, &ctx);
            });
        }
    });
    Ok(bound)
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

fn read_request(r: &mut impl BufRead) -> std::io::Result<Request> {
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let mut headers = Vec::new();
    let mut total = line.len();
    loop {
        let mut h = String::new();
        let n = r.read_line(&mut h)?;
        total += n;
        if n == 0 || h == "\r\n" || h == "\n" || total > 16 * 1024 {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    Ok(Request {
        method,
        path,
        headers,
    })
}

fn respond(
    s: &mut TcpStream,
    status: &str,
    kind: &str,
    body: &[u8],
    extra: &str,
) -> std::io::Result<()> {
    write!(
        s,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n{extra}\r\n",
        body.len()
    )?;
    s.write_all(body)?;
    s.flush()
}

/// Whether a `Host` header names this machine: the address listened on,
/// the loopback names, or a tailnet machine name (`laptop`,
/// `laptop.tail1234.ts.net`). A name of someone else's domain is DNS
/// rebinding, and refused.
fn host_ok(host: Option<&str>, addr: SocketAddr) -> bool {
    let Some(h) = host else { return false };
    let name = match h.rsplit_once(':') {
        Some((n, p)) if p.chars().all(|c| c.is_ascii_digit()) && !n.ends_with(':') => n,
        _ => h,
    };
    let name = name.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = name.parse::<IpAddr>() {
        return ip == addr.ip() || ip.is_loopback();
    }
    name.eq_ignore_ascii_case("localhost")
        || !name.contains('.')
        || name.to_ascii_lowercase().ends_with(".ts.net")
}

fn connection(mut s: TcpStream, ctx: &Ctx) -> std::io::Result<()> {
    s.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(s.try_clone()?);
    let req = read_request(&mut reader)?;
    if !host_ok(req.header("host"), ctx.addr) {
        return respond(
            &mut s,
            "403 Forbidden",
            "text/plain",
            b"this daemon answers to its own machine's names only",
            "",
        );
    }
    let path = req.path.split('?').next().unwrap_or("/").to_string();
    match (req.method.as_str(), path.as_str()) {
        ("GET", "/") | ("GET", "/index.html") => respond(
            &mut s,
            "200 OK",
            "text/html; charset=utf-8",
            PAGE.as_bytes(),
            "Content-Security-Policy: default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src 'self' blob: data:; connect-src 'self' ws: wss:\r\nReferrer-Policy: no-referrer\r\n",
        ),
        ("GET", "/ws") => websocket(s, reader, &req, ctx),
        ("GET", p) if p.starts_with("/file/") => file(&mut s, &req, &p["/file/".len()..], ctx),
        _ => respond(&mut s, "404 Not Found", "text/plain", b"nothing here", ""),
    }
}

/// `/file/<what>`: a file of `.argus/` the protocol named (a review's clip,
/// a mark's frame, a frame the page asked for), for a holder of a token.
fn file(s: &mut TcpStream, req: &Request, rel: &str, ctx: &Ctx) -> std::io::Result<()> {
    let token = req
        .header("authorization")
        .and_then(|a| a.strip_prefix("Bearer "))
        .and_then(|t| tokens::check(&ctx.root, t));
    if token.is_none() {
        return respond(
            s,
            "401 Unauthorized",
            "text/plain",
            b"a token, as Authorization: Bearer <token>",
            "",
        );
    }
    let rel = percent_decode(rel);
    let p = Path::new(&rel);
    let inside = p.components().all(|c| matches!(c, Component::Normal(_)));
    let top = p.components().next().and_then(|c| c.as_os_str().to_str());
    if !inside || !matches!(top, Some("reviews" | "marks" | "frames")) {
        return respond(
            s,
            "403 Forbidden",
            "text/plain",
            b"reviews/, marks/, and frames/ of .argus only",
            "",
        );
    }
    let kind = match p.extension().and_then(|e| e.to_str()) {
        Some("png") => "image/png",
        Some("json") => "application/json",
        Some("wav") => "audio/wav",
        Some("txt") | Some("log") => "text/plain; charset=utf-8",
        _ => {
            return respond(
                s,
                "403 Forbidden",
                "text/plain",
                b"pictures, sound, and text only",
                "",
            )
        }
    };
    match std::fs::read(ctx.root.join(".argus").join(p)) {
        Ok(bytes) => respond(s, "200 OK", kind, &bytes, ""),
        Err(_) => respond(s, "404 Not Found", "text/plain", b"no such file", ""),
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok();
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Calls a token of any scope may make: reads, and the connection's own.
const ALWAYS: &[&str] = &[
    "session.identify",
    "session.status",
    "session.capabilities",
    "session.docs",
    "obs.subscribe",
    "obs.unsubscribe",
];

/// What review adds to observe.
const REVIEW: &[&str] = &["review.answer", "mark.drop", "mark.note"];

/// The daemon's own, never a browser's: tokens, another project, stopping
/// the daemon, a window or a shared-memory stream on this machine, the
/// network.
const DENY: &[&str] = &[
    "session.open",
    "session.close",
    "session.shutdown",
    "render.window",
    "render.stream",
    "mark.pull",
    "mark.import",
];

/// Whether a scope allows a method.
pub fn allows(scope: Scope, method: &str) -> bool {
    if DENY.contains(&method) || method.starts_with("token.") {
        return false;
    }
    let read = crate::feed::READS.contains(&method) || ALWAYS.contains(&method);
    match scope {
        Scope::Observe => read,
        Scope::Review => read || REVIEW.contains(&method),
        Scope::Edit => true,
    }
}

/// A path a call names must stay inside the project: no absolute path and
/// no `..`, whatever the key (`path`, `file`, `out`, `dir`).
fn paths_inside(params: &Json) -> bool {
    match params {
        Json::Object(m) => m.iter().all(|(k, v)| {
            let named = matches!(k.as_str(), "path" | "file" | "out" | "dir" | "golden");
            let ok = match (named, v) {
                (true, Json::String(p)) => {
                    let p = Path::new(p);
                    !p.is_absolute()
                        && !p.has_root()
                        && p.components()
                            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
                }
                _ => true,
            };
            ok && paths_inside(v)
        }),
        Json::Array(a) => a.iter().all(paths_inside),
        _ => true,
    }
}

/// Why a browser's call is refused, if it is.
fn refusal(scope: Scope, line: &str) -> Option<(Json, String)> {
    let Ok(req) = serde_json::from_str::<Json>(line) else {
        return None; // the daemon answers a bad line itself
    };
    let id = req.get("id").cloned().unwrap_or(Json::Null);
    let method = req["method"].as_str().unwrap_or("");
    if !allows(scope, method) {
        let why = if DENY.contains(&method) || method.starts_with("token.") {
            if method == "render.stream" {
                format!("{method} is a window's on this machine (shared memory): a browser asks for frames with obs.frame and fetches /file/frames/...")
            } else {
                format!("{method} is the daemon's own: only a client on this machine makes it")
            }
        } else {
            format!("a {} token does not allow {method}", scope.name())
        };
        return Some((id, why));
    }
    if !paths_inside(&req["params"]) {
        return Some((
            id,
            "a path from a browser stays inside the project: relative, without ..".into(),
        ));
    }
    None
}

/// One WebSocket frame: opcode, whether it ends a message, and payload.
fn read_frame(r: &mut impl Read) -> std::io::Result<(u8, bool, Vec<u8>)> {
    argus_relay::ws::read_frame(r, MAX_MESSAGE)
}

use argus_relay::ws::write_frame;

fn websocket(
    mut s: TcpStream,
    mut reader: BufReader<TcpStream>,
    req: &Request,
    ctx: &Ctx,
) -> std::io::Result<()> {
    let upgrade = req
        .header("upgrade")
        .is_some_and(|u| u.eq_ignore_ascii_case("websocket"));
    let Some(key) = req.header("sec-websocket-key").filter(|_| upgrade) else {
        return respond(
            &mut s,
            "400 Bad Request",
            "text/plain",
            b"a WebSocket upgrade",
            "",
        );
    };
    // The page's own origin, or no browser at all (a script).
    if let Some(o) = req.header("origin") {
        let host = o.split("://").nth(1).unwrap_or("");
        if Some(host) != req.header("host") {
            return respond(
                &mut s,
                "403 Forbidden",
                "text/plain",
                b"another page's origin",
                "",
            );
        }
    }
    let offered: Vec<&str> = req
        .header("sec-websocket-protocol")
        .map(|p| p.split(',').map(str::trim).collect())
        .unwrap_or_default();
    let secret = offered.iter().find_map(|p| p.strip_prefix("t."));
    let Some(token) = secret.and_then(|t| tokens::check(&ctx.root, t)) else {
        return respond(
            &mut s,
            "401 Unauthorized",
            "text/plain",
            b"a token (argus token issue), offered as the subprotocol t.<token>",
            "",
        );
    };
    write!(
        s,
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\nSec-WebSocket-Protocol: argus\r\n\r\n",
        hash::ws_accept(key)
    )?;
    s.flush()?;
    s.set_read_timeout(None)?;
    let id = WEB_CONN_BASE + ctx.next.fetch_add(1, Ordering::Relaxed);
    let (out_tx, out_rx) = mpsc::channel::<String>();
    let mut w = s.try_clone()?;
    std::thread::spawn(move || {
        for line in out_rx {
            if write_frame(&mut w, 1, line.as_bytes()).is_err() {
                break;
            }
        }
        let _ = w.shutdown(std::net::Shutdown::Both);
    });
    let _ = ctx.tx.send(Msg::Open(id, out_tx.clone()));
    // Who it is in the change feed, and what it may do, before anything.
    let label = if token.label.is_empty() {
        token.id.clone()
    } else {
        token.label.clone()
    };
    let identify = json!({ "jsonrpc": "2.0", "method": "session.identify", "params": { "author": format!("web:{label}") } });
    let _ = ctx.tx.send(Msg::Line(id, identify.to_string()));
    let hello = json!({ "jsonrpc": "2.0", "method": "web.hello", "params": {
        "scope": token.scope.name(), "label": label, "expires_at": token.expires_at,
    }});
    let _ = out_tx.send(hello.to_string());
    // A token revoked or run out mid-session ends it within a second,
    // whether or not the page is saying anything, so its notifications
    // stop too.
    let done = Arc::new(AtomicBool::new(false));
    {
        let (done, root, out_tx) = (done.clone(), ctx.root.clone(), out_tx.clone());
        let secret = secret.unwrap_or_default().to_string();
        let expires_at = token.expires_at;
        let s = s.try_clone()?;
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_secs(1));
                if done.load(Ordering::Relaxed) || tokens::check(&root, &secret).is_some() {
                    continue;
                }
                let why = if tokens::now() >= expires_at {
                    "expired"
                } else {
                    "revoked"
                };
                let _ = out_tx.send(
                    json!({ "jsonrpc": "2.0", "method": "web.expired", "params": { "why": why } })
                        .to_string(),
                );
                std::thread::sleep(Duration::from_millis(100));
                let _ = s.shutdown(std::net::Shutdown::Both);
                break;
            }
        });
    }
    let mut pending: Vec<u8> = Vec::new();
    while let Ok((op, fin, data)) = read_frame(&mut reader) {
        match op {
            // Text, or the rest of one.
            1 | 0 => {
                pending.extend_from_slice(&data);
                if pending.len() > MAX_MESSAGE {
                    break;
                }
                if !fin {
                    continue;
                }
                let line = String::from_utf8_lossy(&pending).into_owned();
                pending.clear();
                if tokens::now() >= token.expires_at {
                    let _ = out_tx.send(json!({ "jsonrpc": "2.0", "method": "web.expired", "params": { "why": "expired" } }).to_string());
                    break;
                }
                if let Some((rid, why)) = refusal(token.scope, &line) {
                    if !rid.is_null() {
                        let e = json!({ "jsonrpc": "2.0", "id": rid, "error": {
                            "code": crate::codes::FORBIDDEN, "message": why,
                            "data": { "hint": "argus token issue scope=edit makes a token that may change the game", "scope": token.scope.name() },
                        }});
                        let _ = out_tx.send(e.to_string());
                    }
                    continue;
                }
                if ctx.tx.send(Msg::Line(id, line)).is_err() {
                    break;
                }
            }
            // Close.
            8 => break,
            // Ping: pong.
            9 => {
                let mut w = s.try_clone()?;
                let _ = write_frame(&mut w, 10, &data);
            }
            _ => {}
        }
    }
    done.store(true, Ordering::Relaxed);
    let _ = ctx.tx.send(Msg::Closed(id));
    let _ = s.shutdown(std::net::Shutdown::Both);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_daemon_listens_on_this_machine_and_its_tailnet_only() {
        assert!(allowed("127.0.0.1".parse().unwrap()));
        assert!(allowed("::1".parse().unwrap()));
        assert!(allowed("100.93.53.97".parse().unwrap()));
        assert!(allowed("fd7a:115c:a1e0::1".parse().unwrap()));
        assert!(!allowed("192.168.1.20".parse().unwrap()));
        assert!(!allowed("0.0.0.0".parse().unwrap()));
        assert!(
            !allowed("100.128.0.1".parse().unwrap()),
            "outside 100.64/10"
        );
        assert!(parse_addr("192.168.1.20:7878")
            .unwrap_err()
            .contains("tailnet"));
        assert_eq!(parse_addr("").unwrap().to_string(), "127.0.0.1:7878");
        assert_eq!(parse_addr(":9000").unwrap().to_string(), "127.0.0.1:9000");
    }

    #[test]
    fn a_host_must_be_this_machines() {
        let a: SocketAddr = "100.93.53.97:7878".parse().unwrap();
        assert!(host_ok(Some("100.93.53.97:7878"), a));
        assert!(host_ok(Some("localhost:7878"), a));
        assert!(host_ok(Some("laptop.tail1234.ts.net:7878"), a));
        assert!(host_ok(Some("laptop"), a));
        assert!(!host_ok(Some("evil.example.com:7878"), a), "DNS rebinding");
        assert!(!host_ok(None, a));
    }

    #[test]
    fn scopes_allow_what_they_say() {
        assert!(allows(Scope::Observe, "world.query"));
        assert!(!allows(Scope::Observe, "review.answer"));
        assert!(allows(Scope::Review, "review.answer"));
        assert!(!allows(Scope::Review, "edit.set"));
        assert!(allows(Scope::Edit, "edit.set"));
        for own in [
            "token.issue",
            "session.shutdown",
            "render.window",
            "render.stream",
            "session.open",
        ] {
            assert!(!allows(Scope::Edit, own), "{own}");
        }
        assert!(refusal(
            Scope::Edit,
            r#"{"id":1,"method":"sim.save","params":{"path":"C:/Windows/x.save"}}"#
        )
        .is_some());
        assert!(refusal(
            Scope::Edit,
            r#"{"id":1,"method":"sim.save","params":{"path":"../x.save"}}"#
        )
        .is_some());
        assert!(refusal(
            Scope::Edit,
            r#"{"id":1,"method":"sim.save","params":{"path":"saves/x.save"}}"#
        )
        .is_none());
    }
}
