//! Replay a mark or a report taken in the browser, on the desktop, and say
//! whether the world is the same:
//! `cargo run -p argus-web --example mark_replay -- <project> <file>`.
//!
//! A report (a replay file the page's recorder wrote, phase 2 item 7) goes
//! back to its keyframe and replays its commands; a spike mark (JSON)
//! replays its input log from the load.
use serde_json::Value;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (project, file) = match (a.get(1), a.get(2)) {
        (Some(p), Some(f)) => (p, f),
        _ => {
            eprintln!("usage: mark_replay <project> <mark.json or report.argr>");
            std::process::exit(2)
        }
    };
    let bytes = std::fs::read(file).unwrap();
    if bytes.starts_with(b"ARGR") {
        let meta = argus_player::replay::decode(&bytes)
            .unwrap_or_else(|e| panic!("{}", e.message))
            .meta;
        let mut p = argus_web::Player::open(
            std::path::Path::new(project),
            meta["scene"].as_str(),
            meta["seed"].as_u64(),
        )
        .unwrap_or_else(|e| panic!("{}", e.message));
        let out = p
            .play_report(&bytes)
            .unwrap_or_else(|e| panic!("{}", e.message));
        let same = out["same"] == true;
        println!(
            "{}",
            serde_json::json!({
                "report": true, "same": same, "from": out["from"], "tick": out["tick"],
                "web": [out["saved"], meta["code"]], "native": [out["digest"]],
                "note": meta["note"], "hero": p.position("@player"),
            })
        );
        std::process::exit(if same { 0 } else { 1 });
    }
    let mark: Value = serde_json::from_slice(&bytes).unwrap();
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
