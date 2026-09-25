//! Save games in the player (phase 3 item 0), what a release build and the
//! web page run: the saves a game asks for (`q:save`) come out as save
//! files, each the world of the tick that asked, and a save loads back to
//! that world, which plays on the same.

use argus_player::{savegame, Player};
use std::path::Path;

fn outpost() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/outpost")
}

#[test]
fn the_game_saves_at_the_tick_it_asks_and_a_save_loads_back() {
    let mut p = Player::open(&outpost(), None, None).unwrap();
    // Outpost saves itself as "auto" when a day turns: at tick 3600.
    p.step(3700).unwrap();
    let saves = p.take_saves("test");
    assert_eq!(saves.len(), 1, "one day turned");
    let (name, bytes) = &saves[0];
    assert_eq!(name, "auto");
    let file = savegame::decode(bytes).unwrap();
    assert_eq!(file.tick, 3601, "the world of the tick that asked");
    assert_eq!(file.meta["game"], "outpost");
    assert_eq!(file.meta["code"], "test");
    assert!(p.take_saves("test").is_empty(), "taken");

    // The same run, stepped to 3601 in a second player, is the world saved.
    let mut q = Player::open(&outpost(), None, None).unwrap();
    q.step(3601).unwrap();
    let then = q.digest();
    q.step(600).unwrap();
    let later = q.digest();

    // Load it: back at 3601, and it plays on the same.
    let report = p.load(bytes).unwrap();
    assert_eq!(report["migrated"], serde_json::json!([]));
    assert_eq!(p.tick(), 3601);
    assert_eq!(p.digest(), then);
    p.step(600).unwrap();
    assert_eq!(p.digest(), later);

    // A damaged save is refused and the game goes on as it was.
    let tick = p.tick();
    assert!(p.load(&bytes[..bytes.len() / 2]).is_err());
    assert_eq!(p.tick(), tick);
}
