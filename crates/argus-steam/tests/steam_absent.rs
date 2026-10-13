//! Steam not running: the platform is not started, and Steam's library is
//! never asked to, so it prints nothing of its own. Its `SteamAPI_Init`
//! printed three `[S_API FAIL]` lines on stderr when the client was not
//! there, which a player running a released game from a terminal saw
//! (Todd's Mac test, issue 2). On a machine where Steam is running this
//! has nothing to show, and says so.

use argus_steam::SteamPlatform;

#[test]
fn without_a_steam_client_the_platform_says_so_and_steam_says_nothing() {
    // SAFETY: callable before SteamAPI_Init; it only looks for the client.
    if unsafe { steamworks_sys::SteamAPI_IsSteamRunning() } {
        eprintln!("skipped: Steam is running here");
        return;
    }
    let e = match SteamPlatform::init(480) {
        Ok(_) => panic!("no Steam client, yet the platform started"),
        Err(e) => e,
    };
    assert!(e.message.contains("not running"), "{}", e.message);
}
