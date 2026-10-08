//! Phase 5 item 7: a replay file of a game with rigid bodies (the toybox)
//! carries Rapier's own state with its keyframe (version 6), so a player
//! that opens it, in another process or on another day, replays to the
//! world the run reached. (Built with the module: `--features rigid`, or
//! the workspace, where the daemon asks for it.)
#![cfg(feature = "rigid")]

use argus_player::{replay, Player};
use serde_json::json;
use std::path::{Path, PathBuf};

fn toybox() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/toybox")
}

#[test]
fn a_replay_file_replays_the_rigid_bodies_to_the_same_world() {
    let mut p = Player::open(&toybox(), None, None).unwrap();
    p.step(100).unwrap();
    let k = p.sources.capture(&p.world, &p.sched, false);
    assert!(k.rigid.bytes() > 0, "the keyframe holds the bodies' state");
    let bytes = replay::encode(&json!({ "to": 220 }), &k, 0, &[], None, &Vec::new());
    p.step(120).unwrap();
    let want = p.digest();

    let mut q = Player::open(&toybox(), None, None).unwrap();
    let file = replay::decode(&bytes).unwrap();
    let back = file.keyframe(&q.world, &q.particles.borrow()).unwrap();
    q.sources
        .restore(&back.keyframe, &mut q.world, &mut q.sched);
    q.step(120).unwrap();
    assert_eq!(q.tick(), 220);
    assert_eq!(
        q.digest(),
        want,
        "the same world 120 ticks after the keyframe"
    );
}
