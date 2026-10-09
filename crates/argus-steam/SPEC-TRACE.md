# argus-steam: spec trace

The design document's (v0.3) Steam implementation of the platform layer (§15: "a platform plugin interface ... with a Steam implementation first"; §18's phase 5 sentence), decided with Kevin on 2026-10-04 to be tested against Spacewar (app 480, Valve's test app) on his logged-in Steam, through the `steamworks` crate. Statuses: **Done**, **Partial**, **Deferred**.

## §15 Shipping

| Spec | Status | Where | Test |
|---|---|---|---|
| A Steam implementation of the platform interface. | Done | `SteamPlatform` over steamworks-rs 0.13 and Valve's Steamworks SDK (steamworks-sys 0.13, which carries the SDK's headers and libraries; downloaded with Kevin's approval, 2026-10-08). `init(app_id)` starts Steam for the game's app (`platform.toml`'s `[steam] app_id`). | `steam_live::spacewar_takes_achievements_presence_cloud_files_and_gives_glyphs` |
| Achievements. | Done | By their Steamworks API names: `unlock` (set and store, saying whether it was new), `achieved`, `clear_achievement`, and `progress` (IndicateAchievementProgress, through the SDK's own call). The first call waits for the account's stats, which Steam fetches after it starts (`UserStatsReceived`), up to three seconds. | `steam_live` (Spacewar's `ACH_WIN_ONE_GAME`, locked, unlocked, cleared again); `steam_live::spacewar_names_its_achievements` |
| Cloud saves. | Done | Steam Cloud's files (Remote Storage): write, read, list, delete; `cloud_enabled` is the account's and the app's setting both. | `steam_live` (a file written, read, listed, and deleted) |
| Rich presence. | Done | `presence(key, value)` sets or clears a key; the platform clears it all when it goes, so friends see nothing after the game. | `steam_live` (set, read back through the friends API, cleared) |
| Input glyphs. | Done | Steam Input's picture of a button (GetGlyphPNGForActionOrigin, medium), for the Xbox, PlayStation, and Switch families, by the action map's names (`south` is the Switch's B); Steam Input starts the first time one is asked for. A generic pad, a key, and the guide button have none. The host registers them as sprites for the UI. | `tests::every_pad_input_has_a_steam_origin_but_a_generic_pads`; `steam_live` (Xbox A and PlayStation Cross, 128 x 128) |
| The overlay. | Done | `open_overlay` (Steam's dialogs: friends, community, players, settings, officialgamegroup, stats, achievements; or an https:// page) and `overlay_open` from the `GameOverlayActivated` callback. Steam starts before the window's graphics device, so its overlay can draw into it. | `steam_live` (the overlay's state reported; not enabled without a window) |
| A player without Steam still plays. | Done | `init` fails when Steam is not running or the account does not own the app; argus-game then runs on the null platform and says so on the bottom line (`start_platform`). | Not tested here: Steam runs on this machine. Every test but `steam_live` runs on the null platform. |

## §17

No row names Steam. The live test runs in 0.4 s, most of it Steam starting.

## Known limitations, stated so they are decisions

- The live test is run by hand with the account owner's say-so, never by CI (it is ignored and does nothing without `ARGUS_STEAM_LIVE=1`). It leaves the account as it found it.
- Until a game has its own Steam app, it runs as Spacewar; its achievements are mapped to Spacewar's five, and Spacewar's rich presence has no localization tokens, so friends see the raw status only in the game's info.
- The overlay was not seen drawn here: the test has no window, and a window is Kevin's to open (his hand check).
- Steam Input's pictures are of the pad's family as the engine knows it; a Steam Deck's own buttons need its family from Steam Input (`DeviceInput::PadFamily`), which a host would feed.
- `steam_api64.dll` (or `libsteam_api.so`, `libsteam_api.dylib`) ships beside the executable, as Valve's SDK terms allow for a game on Steam.
