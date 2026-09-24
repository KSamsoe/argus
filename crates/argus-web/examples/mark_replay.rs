//! Replay a mark taken in the browser, on the desktop, and say whether the
//! world is the same: `cargo run -p argus-web --example mark_replay -- <project> <mark.json>`.
use serde_json::Value;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (project, file) = match (a.get(1), a.get(2)) {
        (Some(p), Some(f)) => (p, f),
        _ => {
            eprintln!("usage: mark_replay <project> <mark.json>");
            std::process::exit(2)
        }
    };
    let mark: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    let mut p = argus_web::Player::open(
        std::path::Path::new(project),
        mark["scene"].as_str(),
        mark["seed"].as_u64(),
    )
    .unwrap_or_else(|e| panic!("{}", e.message));
    let n = p
        .replay_log(&mark["log"])
        .unwrap_or_else(|e| panic!("{}", e.message));
    let tick = mark["tick"].as_u64().unwrap();
    p.step(tick).unwrap();
    let here = p.mark(mark["note"].as_str().unwrap_or(""), "native");
    let same = here["digest"] == mark["digest"] && here["events_digest"] == mark["events_digest"];
    println!(
        "{}",
        serde_json::json!({
            "replayed": n, "tick": tick, "same": same,
            "web": [mark["digest"], mark["events_digest"], mark["code"]],
            "native": [here["digest"], here["events_digest"]],
            "hero": p.position("@player"),
        })
    );
    std::process::exit(if same { 0 } else { 1 });
}
