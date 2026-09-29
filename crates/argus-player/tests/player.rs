//! The player natively: Hollow opens without the daemon, runs, and is
//! reproducible. The same sequence runs in the browser
//! (tools/web/check.mjs); `--nocapture` prints the digests the
//! two compare.

use argus_player::Player;
use serde_json::json;
use std::path::Path;

fn hollow() -> Player {
    Player::open(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hollow"),
        None,
        None,
    )
    .unwrap()
}

/// The world's summary at a checkpoint. The Luau heap's size is left out:
/// it depends on when the collector ran, and on whether the scripts were
/// compiled here or shipped as bytecode.
fn world(p: &Player) -> String {
    let mut s = p.status();
    s.as_object_mut().unwrap().remove("script_bytes");
    s.to_string()
}

/// Hollow's run scenario: hold right, jump the step, run into the hall.
fn run(p: &mut Player) -> Vec<String> {
    let mut out = vec![world(p)];
    p.inject("move", Some(&json!([1, 0])), Some(450), false)
        .unwrap();
    p.step(118).unwrap();
    out.push(world(p));
    p.inject("jump", Some(&json!(true)), Some(18), false)
        .unwrap();
    p.step(330).unwrap();
    out.push(world(p));
    p.step(600).unwrap();
    out.push(world(p));
    out
}

#[test]
fn hollow_runs_without_the_daemon_and_twice_the_same() {
    let a = run(&mut hollow());
    let b = run(&mut hollow());
    for line in &a {
        println!("native {line}");
    }
    assert_eq!(a, b, "the same run, twice");
    let end: serde_json::Value = serde_json::from_str(&a[2]).unwrap();
    assert_eq!(end["tick"], 448);
    let mut p = hollow();
    run(&mut p);
    let [x, _] = p.position("@player").unwrap();
    assert!(x > 0.0, "the hero moved: {x}");
}

#[test]
fn a_pack_round_trips_a_project() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hollow");
    let files = argus_player::unpack(&argus_player::pack(&root).unwrap()).unwrap();
    assert!(files.iter().any(|(p, _)| p == "argus.toml"));
    assert!(files
        .iter()
        .any(|(p, _)| p == "scenes/hollow/level.chunks/ground/1_0.bin"));
    assert!(!files
        .iter()
        .any(|(p, _)| p.starts_with(".argus") || p.starts_with("tests/")));
    let mut sorted: Vec<&String> = files.iter().map(|(p, _)| p).collect();
    sorted.sort();
    assert_eq!(sorted, files.iter().map(|(p, _)| p).collect::<Vec<_>>());
}
