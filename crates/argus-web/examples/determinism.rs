//! The cross-platform determinism spike's native half (phase 5 item 11):
//! play a run file and say the world's and the events' digests every
//! `every` ticks, on whatever platform this was built for.
//!
//! ```text
//! cargo run --release -p argus-web --example determinism -- <run.json> [--packs <site>] [--rows <tick>]
//! ```
//!
//! A run file is `{game, project, scene, seed, ticks, every, log}`, the log
//! an input log's rows (`[tick, slot, action, value]`), made by
//! `tools/determinism/runs.mjs`. The same file plays in the browser
//! (`tools/determinism/web.mjs`), so the two lists compare line by line.
//! `--packs` opens a built site's packs instead of the project's folder,
//! as the page does; `--rows` stops at that tick and prints what the
//! digest reads, a line a field, to find the first field that differs.
use serde_json::{json, Value};
use std::path::Path;
use std::time::Instant;

/// What this was built for.
fn platform(packs: bool) -> Value {
    let env = if cfg!(target_env = "gnu") {
        "gnu"
    } else if cfg!(target_env = "msvc") {
        "msvc"
    } else if cfg!(target_env = "musl") {
        "musl"
    } else {
        ""
    };
    json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "env": env,
        "from": if packs { "packs" } else { "project" },
    })
}

fn fail(msg: &str) -> ! {
    eprintln!("determinism: {msg}");
    std::process::exit(1)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    // `--math [n]`: the math probe (argus_web::mathprobe) instead of a run.
    if a.first().map(String::as_str) == Some("--math") {
        let n = a.get(1).and_then(|n| n.parse().ok()).unwrap_or(20_000);
        let mut out = argus_web::mathprobe::math_probe(n);
        out["platform"] = platform(false);
        println!("{out}");
        return;
    }
    let Some(file) = a.first() else {
        eprintln!("usage: determinism <run.json> [--packs <site>] [--rows <tick>] | --math [n]");
        std::process::exit(2)
    };
    let flag = |name: &str| {
        a.iter()
            .position(|x| x == name)
            .and_then(|i| a.get(i + 1))
            .cloned()
    };
    let run: Value = serde_json::from_slice(
        &std::fs::read(file).unwrap_or_else(|e| fail(&format!("{file}: {e}"))),
    )
    .unwrap_or_else(|e| fail(&format!("{file}: {e}")));
    let scene = run["scene"].as_str();
    let seed = run["seed"].as_u64();
    let t0 = Instant::now();
    let mut p = match flag("--packs") {
        Some(site) => {
            let listed: Value = serde_json::from_slice(
                &std::fs::read(Path::new(&site).join("data/packs.json"))
                    .unwrap_or_else(|e| fail(&format!("{site}: {e}"))),
            )
            .unwrap_or_else(|e| fail(&e.to_string()));
            let packs: Vec<_> = listed["packs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|n| {
                    let bytes = std::fs::read(Path::new(&site).join("data").join(n))
                        .unwrap_or_else(|e| fail(&format!("{n}: {e}")));
                    argus_web::pack::Pack::decode(&bytes).unwrap_or_else(|e| fail(&e.message))
                })
                .collect();
            let (files, _) =
                argus_web::pack::resolve(&packs, None).unwrap_or_else(|e| fail(&e.message));
            argus_os::fs::mount("/game", files);
            argus_web::Player::open(Path::new("/game"), scene, seed)
        }
        None => {
            let project = run["project"]
                .as_str()
                .unwrap_or_else(|| fail("no project"));
            argus_web::Player::open(Path::new(project), scene, seed)
        }
    }
    .unwrap_or_else(|e| fail(&e.message));
    if let Some(e) = p.plugins.errors.first() {
        fail(&format!("a plugin did not load: {e:?}"));
    }
    let opened_ms = t0.elapsed().as_secs_f64() * 1e3;
    // A game with no input (toybox, puppet) has no log to replay, and may
    // have no input map to replay one into.
    let replayed = match run["log"].as_array() {
        Some(rows) if !rows.is_empty() => p
            .replay_log(&run["log"])
            .unwrap_or_else(|e| fail(&e.message)),
        _ => 0,
    };
    let every = run["every"].as_u64().unwrap_or(60).max(1);
    let ticks = run["ticks"].as_u64().unwrap_or(3600);
    let stop = flag("--rows").map(|t| t.parse::<u64>().unwrap_or_else(|_| fail("--rows <tick>")));
    let t1 = Instant::now();
    let mut digests = Vec::new();
    let mut done = 0;
    // What a digest costs: what a lockstep game would pay to check that
    // two machines still agree.
    let mut digest_ms = 0.0;
    while done < stop.unwrap_or(ticks) {
        let n = every.min(stop.unwrap_or(ticks) - done);
        p.step(n).unwrap_or_else(|e| fail(&e.message));
        done += n;
        let t = Instant::now();
        let world = p.digest();
        digest_ms += t.elapsed().as_secs_f64() * 1e3;
        digests.push(json!([
            p.tick(),
            format!("{world:016x}"),
            format!("{:016x}", p.events_digest()),
        ]));
    }
    if stop.is_some() {
        for row in p.digest_rows() {
            println!("{row}");
        }
        return;
    }
    println!(
        "{}",
        json!({
            "game": run["game"],
            "platform": platform(flag("--packs").is_some()),
            "scene": scene, "seed": seed, "replayed": replayed,
            "ticks": ticks, "every": every,
            "open_ms": opened_ms,
            "run_ms": t1.elapsed().as_secs_f64() * 1e3,
            "digest_ms_mean": digest_ms / digests.len().max(1) as f64,
            "entities": p.status()["entities"],
            "digests": digests,
        })
    );
}
