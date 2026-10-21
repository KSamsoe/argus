//! WebSocket (RFC 6455), as much as the engine needs: frames, the server's
//! side of the handshake (the relay, the daemon's web listener), and the
//! client's (a desktop game, the daemon joining a match). No TLS: behind a
//! public name a proxy in front of the relay ends it.

use crate::hash;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;

/// Opcodes.
pub mod op {
    /// More of a message.
    pub const CONTINUE: u8 = 0;
    /// Text.
    pub const TEXT: u8 = 1;
    /// Bytes.
    pub const BINARY: u8 = 2;
    /// Closing.
    pub const CLOSE: u8 = 8;
    /// Are you there?
    pub const PING: u8 = 9;
    /// Here.
    pub const PONG: u8 = 10;
}

/// One frame: its opcode, whether it ends a message, and its payload.
/// A payload longer than `max` is an error, before it is read.
pub fn read_frame(r: &mut impl Read, max: usize) -> std::io::Result<(u8, bool, Vec<u8>)> {
    let mut h = [0u8; 2];
    r.read_exact(&mut h)?;
    let fin = h[0] & 0x80 != 0;
    let op = h[0] & 0x0f;
    let masked = h[1] & 0x80 != 0;
    let mut len = (h[1] & 0x7f) as u64;
    if len == 126 {
        let mut b = [0u8; 2];
        r.read_exact(&mut b)?;
        len = u16::from_be_bytes(b) as u64;
    } else if len == 127 {
        let mut b = [0u8; 8];
        r.read_exact(&mut b)?;
        len = u64::from_be_bytes(b);
    }
    if len > max as u64 {
        return Err(std::io::Error::other("a message too long"));
    }
    let mut mask = [0u8; 4];
    if masked {
        r.read_exact(&mut mask)?;
    }
    let mut data = vec![0u8; len as usize];
    r.read_exact(&mut data)?;
    if masked {
        for (i, b) in data.iter_mut().enumerate() {
            *b ^= mask[i % 4];
        }
    }
    Ok((op, fin, data))
}

/// A frame as bytes: a server's unmasked, a client's with `mask`.
pub fn frame(op: u8, data: &[u8], mask: Option<[u8; 4]>) -> Vec<u8> {
    let mut h = vec![0x80 | op];
    let bit = if mask.is_some() { 0x80 } else { 0 };
    match data.len() {
        n if n < 126 => h.push(bit | n as u8),
        n if n <= u16::MAX as usize => {
            h.push(bit | 126);
            h.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            h.push(bit | 127);
            h.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    match mask {
        None => h.extend_from_slice(data),
        Some(m) => {
            h.extend_from_slice(&m);
            h.extend(data.iter().enumerate().map(|(i, b)| b ^ m[i % 4]));
        }
    }
    h
}

/// Write a server's frame.
pub fn write_frame(w: &mut impl Write, op: u8, data: &[u8]) -> std::io::Result<()> {
    w.write_all(&frame(op, data, None))?;
    w.flush()
}

/// An HTTP request's line and headers, as a WebSocket upgrade sends them.
#[derive(Clone, Debug, Default)]
pub struct Request {
    /// `GET`.
    pub method: String,
    /// `/relay`.
    pub path: String,
    /// Header names lowercased.
    pub headers: Vec<(String, String)>,
}

impl Request {
    /// A header by its lowercase name.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Read a request's line and headers (at most 64 lines of 8 KB).
pub fn read_request(r: &mut impl BufRead) -> std::io::Result<Request> {
    let mut line = String::new();
    let mut lines = 0;
    let mut req = Request::default();
    loop {
        line.clear();
        if r.take(8192).read_line(&mut line)? == 0 {
            return Err(std::io::Error::other("the connection closed"));
        }
        let l = line.trim_end();
        if lines == 0 {
            let mut parts = l.split(' ');
            req.method = parts.next().unwrap_or("").to_string();
            req.path = parts.next().unwrap_or("").to_string();
        } else if l.is_empty() {
            return Ok(req);
        } else if let Some((k, v)) = l.split_once(':') {
            req.headers
                .push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
        lines += 1;
        if lines > 64 {
            return Err(std::io::Error::other("too many header lines"));
        }
    }
}

/// The server's answer to an upgrade: the accept key for the client's, or
/// `None` when the request is not a WebSocket upgrade.
pub fn accept(req: &Request) -> Option<String> {
    let upgrade = req
        .header("upgrade")
        .is_some_and(|u| u.eq_ignore_ascii_case("websocket"));
    req.header("sec-websocket-key")
        .filter(|_| upgrade)
        .map(hash::ws_accept)
}

/// A client's random-enough bytes: a mask, a handshake key. Not for
/// secrets: the mask only keeps a proxy from reading frames as HTTP.
pub fn noise() -> [u8; 16] {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut z = t ^ N.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed);
    let mut out = [0u8; 16];
    for c in out.chunks_mut(8) {
        z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut x = z;
        x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        c.copy_from_slice(&(x ^ (x >> 31)).to_le_bytes());
    }
    out
}

/// Where a `ws://host:port/path` address points.
pub fn parse_url(url: &str) -> Result<(String, String), String> {
    let rest = url
        .strip_prefix("ws://")
        .ok_or_else(|| format!("{url} is not a ws:// address (wss:// needs a proxy in front)"))?;
    let (host, path) = match rest.split_once('/') {
        Some((h, p)) => (h.to_string(), format!("/{p}")),
        None => (rest.to_string(), "/".to_string()),
    };
    if host.is_empty() {
        return Err(format!("{url} names no host"));
    }
    Ok((host, path))
}

/// Open a WebSocket to `url` as a client: the stream, and a reader over
/// its other half, after the server agreed.
pub fn connect(url: &str) -> std::io::Result<(TcpStream, BufReader<TcpStream>)> {
    let (host, path) = parse_url(url).map_err(std::io::Error::other)?;
    let addr = if host.contains(':') {
        host.clone()
    } else {
        format!("{host}:80")
    };
    let mut s = TcpStream::connect(addr)?;
    s.set_nodelay(true)?;
    let key = hash::base64(&noise());
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )?;
    s.flush()?;
    let mut reader = BufReader::new(s.try_clone()?);
    let mut status = String::new();
    reader.read_line(&mut status)?;
    if !status.starts_with("HTTP/1.1 101") {
        return Err(std::io::Error::other(format!(
            "{url} answered {}",
            status.trim()
        )));
    }
    let mut want = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(std::io::Error::other("the connection closed"));
        }
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            if k.trim().eq_ignore_ascii_case("sec-websocket-accept") {
                want = Some(v.trim().to_string());
            }
        }
    }
    if want.as_deref() != Some(hash::ws_accept(&key).as_str()) {
        return Err(std::io::Error::other(format!(
            "{url} is not a WebSocket server"
        )));
    }
    Ok((s, reader))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_read_back_masked_or_not() {
        for len in [0, 5, 125, 126, 300, 70_000] {
            let data: Vec<u8> = (0..len).map(|i| (i * 7) as u8).collect();
            for mask in [None, Some([1, 2, 3, 4])] {
                let bytes = frame(op::BINARY, &data, mask);
                let (o, fin, got) = read_frame(&mut &bytes[..], 1 << 20).unwrap();
                assert_eq!((o, fin), (op::BINARY, true));
                assert_eq!(got, data);
            }
        }
        let big = frame(op::BINARY, &[0; 2000], None);
        assert!(read_frame(&mut &big[..], 1000).is_err());
        assert_eq!(
            parse_url("ws://klab:8790/relay").unwrap(),
            ("klab:8790".into(), "/relay".into())
        );
        assert!(parse_url("http://x").is_err());
    }
}
