//! `argusd`: the Argus daemon.
//!
//! ```text
//! argusd --project <dir> [--scene <name>] [--seed <n>] [--stdio | --socket [name]] [--headless]
//! ```
//!
//! With `--project` the session opens before the first client connects and
//! the open result (or error) is printed on stderr. `--headless` is the
//! only mode in phase 0 and is accepted for forward compatibility.

use argus_daemon::{serve, socket_name_for, Daemon, Transport};
use serde_json::json;
use std::path::PathBuf;

/// Count allocations per system for `prof.alloc` (§5). The cost is two
/// thread-local increments per allocation.
#[global_allocator]
static ALLOC: argus_sim::alloc::Counting = argus_sim::alloc::Counting;

fn usage() -> ! {
    eprintln!("usage: argusd [--project <dir>] [--scene <name>] [--seed <n>] [--stdio | --socket [name]] [--headless]");
    std::process::exit(2)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut project: Option<PathBuf> = None;
    let mut scene: Option<String> = None;
    let mut seed: Option<u64> = None;
    let mut transport: Option<Transport> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--project" | "-p" => {
                project = Some(PathBuf::from(args.next().unwrap_or_else(|| usage())))
            }
            "--scene" | "-s" => scene = Some(args.next().unwrap_or_else(|| usage())),
            "--seed" => {
                seed = Some(
                    args.next()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or_else(|| usage()),
                )
            }
            "--stdio" => transport = Some(Transport::Stdio),
            "--socket" => {
                let name = args.next();
                transport = Some(Transport::Socket(name.unwrap_or_default()));
            }
            "--headless" => {}
            "--help" | "-h" => usage(),
            _ => usage(),
        }
    }
    let mut daemon = Daemon::new();
    let root = project
        .as_ref()
        .map(|p| p.canonicalize().unwrap_or(p.clone()));
    if let Some(root) = &root {
        let mut params = json!({ "project": root.to_string_lossy() });
        if let Some(s) = &scene {
            params["scene"] = json!(s);
        }
        if let Some(s) = seed {
            params["seed"] = json!(s);
        }
        match daemon.call(0, "session.open", &params) {
            Ok(v) => eprintln!("argusd: opened {}", v["scene"]),
            Err(e) => {
                eprintln!(
                    "argusd: {}",
                    serde_json::to_string(&argus_daemon::json::error_json(&e)).unwrap_or_default()
                );
                std::process::exit(1);
            }
        }
    }
    let transport = match transport {
        Some(Transport::Socket(name)) if name.is_empty() => {
            let Some(root) = &root else {
                eprintln!("argusd: --socket without a name needs --project");
                std::process::exit(2);
            };
            Transport::Socket(socket_name_for(root))
        }
        Some(t) => t,
        None => match &root {
            Some(root) => Transport::Socket(socket_name_for(root)),
            None => Transport::Stdio,
        },
    };
    if let Transport::Socket(name) = &transport {
        eprintln!("argusd: listening on {name}");
    }
    if let Err(e) = serve(daemon, transport, root.as_deref()) {
        eprintln!("argusd: {e}");
        std::process::exit(1);
    }
}
