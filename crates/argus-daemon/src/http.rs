//! A small HTTP/1.1 client for `mark.pull` (phase 2 item 7): GET over plain
//! TCP from the server that hosts a playtest build (klab), where browsers
//! post their reports. There is no TLS: marks are pulled over the local
//! network or the tailnet, not through the public link the players use.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// `http://host[:port]/path` split into host, port, and path.
fn split(url: &str) -> Result<(String, u16, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("{url}: only http:// URLs (the server's LAN or tailnet address)"))?;
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, "/".to_string()),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().map_err(|_| format!("{url}: bad port"))?),
        None => (hostport, 80),
    };
    if host.is_empty() {
        return Err(format!("{url}: no host"));
    }
    Ok((host.to_string(), port, path))
}

/// Decode a chunked body.
fn unchunk(mut b: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let i = b
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("a chunk without its size")?;
        let size = std::str::from_utf8(&b[..i])
            .ok()
            .and_then(|s| usize::from_str_radix(s.split(';').next()?.trim(), 16).ok())
            .ok_or("a bad chunk size")?;
        b = &b[i + 2..];
        if size == 0 {
            return Ok(out);
        }
        if b.len() < size {
            return Err("a chunk cut short".into());
        }
        out.extend_from_slice(&b[..size]);
        b = b.get(size + 2..).unwrap_or(&[]);
    }
}

/// GET a URL; the body of a 200 response, or why not.
pub fn get(url: &str, timeout: Duration) -> Result<Vec<u8>, String> {
    let (host, port, path) = split(url)?;
    let addr = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| format!("{host}: {e}"))?
        .next()
        .ok_or_else(|| format!("{host}: no address"))?;
    let mut s = TcpStream::connect_timeout(&addr, timeout).map_err(|e| format!("{url}: {e}"))?;
    let _ = s.set_read_timeout(Some(timeout));
    let _ = s.set_write_timeout(Some(timeout));
    // One write: a server may read the request in one go.
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nUser-Agent: argus\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    );
    s.write_all(req.as_bytes())
        .map_err(|e| format!("{url}: {e}"))?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).map_err(|e| format!("{url}: {e}"))?;
    let end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| format!("{url}: not an HTTP response"))?;
    let head = String::from_utf8_lossy(&raw[..end]).to_string();
    let body = &raw[end + 4..];
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse::<u16>().ok())
        .unwrap_or(0);
    if status != 200 {
        return Err(format!("{url}: the server answered {status}"));
    }
    let lower = head.to_ascii_lowercase();
    if lower.contains("transfer-encoding: chunked") {
        return unchunk(body).map_err(|e| format!("{url}: {e}"));
    }
    Ok(body.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_chunks() {
        assert_eq!(
            split("http://192.168.1.176:8080/marks").unwrap(),
            ("192.168.1.176".into(), 8080, "/marks".into())
        );
        assert_eq!(
            split("http://klab").unwrap(),
            ("klab".into(), 80, "/".into())
        );
        assert!(split("https://x").is_err());
        assert_eq!(
            unchunk(b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n").unwrap(),
            b"Wikipedia"
        );
    }
}
