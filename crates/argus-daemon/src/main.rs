//! `argusd`: the Argus daemon.
//!
//! ```text
//! argusd --project <dir> [--scene <name>] [--seed <n>] [--stdio | --socket [name]] [--headless | --window [WxH]]
//! ```
//!
//! With `--project` the session opens before the first client connects and
//! the open result (or error) is printed on stderr. `--headless` is the
//! only mode in phase 0 and is accepted for forward compatibility.

use argus_daemon::{launch, Launch, Transport};
use std::path::PathBuf;

/// Count allocations per system for `prof.alloc` (§5). The cost is two
/// thread-local increments per allocation.
#[global_allocator]
static ALLOC: argus_sim::alloc::Counting = argus_sim::alloc::Counting;

fn usage() -> ! {
    eprintln!("usage: argusd [--project <dir>] [--scene <name>] [--seed <n>] [--stdio | --socket [name]] [--headless | --window [WxH]]");
    std::process::exit(2)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut project: Option<PathBuf> = None;
    let mut scene: Option<String> = None;
    let mut seed: Option<u64> = None;
    let mut transport: Option<Transport> = None;
    let mut window: Option<(u32, u32)> = None;
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
            "--window" => {
                window = Some(match args.next() {
                    Some(s) if s.contains('x') => {
                        let (w, h) = s.split_once('x').unwrap();
                        (w.parse().unwrap_or(1280), h.parse().unwrap_or(720))
                    }
                    _ => (1280, 720),
                })
            }
            "--help" | "-h" => usage(),
            _ => usage(),
        }
    }
    std::process::exit(launch(Launch {
        project,
        scene,
        seed,
        transport,
        window,
    }));
}
