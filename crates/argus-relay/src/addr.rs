//! Where a listener may listen: this machine, its tailnet, or (for a relay,
//! asked for by name) everywhere.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

/// The relay's port unless told another.
pub const RELAY_PORT: u16 = 7879;

/// Whether `ip` is this machine or a tailnet's (100.64.0.0/10, or
/// Tailscale's fd7a:115c:a1e0::/48).
pub fn allowed(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            v.is_loopback() || (v.octets()[0] == 100 && (64..128).contains(&v.octets()[1]))
        }
        IpAddr::V6(v) => v.is_loopback() || v.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    }
}

/// This machine's tailnet address, if it has one: the address a UDP socket
/// would send from to reach Tailscale's 100.100.100.100 (nothing is sent).
pub fn tailnet_ip() -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("100.100.100.100:53").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (allowed(ip) && !ip.is_loopback()).then_some(ip)
}

/// `argus relay [where]` as an address: `local` (the default) is this
/// machine, `tailnet` its tailnet address, `public` every address it has
/// (the internet, if the router lets it in), else `ip`, `ip:port`, or
/// `:port`; each may end `:port`. An address outside this machine and its
/// tailnet is refused unless the word is `public`, which says the person
/// chose it.
pub fn relay_addr(s: &str) -> Result<SocketAddr, String> {
    let s = s.trim();
    let (word, port) = match s.rsplit_once(':') {
        Some((w, p)) if matches!(w, "local" | "tailnet" | "public") => (
            w,
            p.parse::<u16>()
                .map_err(|_| format!("{s:?}: {p:?} is not a port"))?,
        ),
        _ => (s, RELAY_PORT),
    };
    let local = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let addr = match word {
        "" | "local" | "localhost" => local,
        "tailnet" => SocketAddr::new(
            tailnet_ip().ok_or("this machine has no tailnet address (is Tailscale running?)")?,
            port,
        ),
        "public" => return Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port)),
        _ => {
            if let Some(p) = s.strip_prefix(':') {
                SocketAddr::new(
                    local.ip(),
                    p.parse().map_err(|_| format!("{s:?}: no port"))?,
                )
            } else if let Ok(a) = s.parse::<SocketAddr>() {
                a
            } else if let Ok(ip) = s.parse::<IpAddr>() {
                SocketAddr::new(ip, RELAY_PORT)
            } else {
                return Err(format!(
                    "{s:?} is not an address: local, tailnet, public, an ip, or ip:port"
                ));
            }
        }
    };
    if !allowed(addr.ip()) {
        return Err(format!(
            "{} is outside this machine and its tailnet: say public to let anyone reach the relay (the manual's Releasing chapter says what that exposes)",
            addr.ip()
        ));
    }
    Ok(addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn where_a_relay_listens() {
        assert_eq!(relay_addr("").unwrap().to_string(), "127.0.0.1:7879");
        assert_eq!(
            relay_addr("local:9000").unwrap().to_string(),
            "127.0.0.1:9000"
        );
        assert_eq!(relay_addr(":9001").unwrap().to_string(), "127.0.0.1:9001");
        assert_eq!(relay_addr("public").unwrap().to_string(), "0.0.0.0:7879");
        assert_eq!(relay_addr("public:80").unwrap().to_string(), "0.0.0.0:80");
        assert_eq!(
            relay_addr("100.70.114.55:7000").unwrap().to_string(),
            "100.70.114.55:7000"
        );
        assert!(relay_addr("0.0.0.0").unwrap_err().contains("say public"));
        assert!(relay_addr("8.8.8.8:1").is_err());
        assert!(relay_addr("public:x").is_err());
    }
}
