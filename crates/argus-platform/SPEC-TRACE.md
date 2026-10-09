# argus-platform: spec trace

Every statement in the design document (v0.3) about the platform layer (§15, with §6 where it touches it, and §18's phase 5 sentence), with where it lives and the test that pins it. Statuses: **Done**, **Partial**, **Deferred**.

## §15 Shipping

| Spec | Status | Where | Test |
|---|---|---|---|
| "A platform plugin interface — achievements, cloud saves, rich presence, input glyphs, overlay —" | Done | The `Platform` trait: `unlock`, `achieved`, `clear_achievement`, `progress`; `presence`; `cloud_write`, `cloud_read`, `cloud_delete`, `cloud_files`; `open_overlay`, `overlay_open`; `glyph_image`; `update` once a frame; `status`. `platform.toml` (`PlatformConfig`) names the game's achievements (name, doc, hidden, and the platform's own API name), Steam's app id, and whether saves go to the cloud. | `platform::platform_toml_names_the_achievements_and_steams_app`, `platform::a_platform_toml_that_is_not_one_is_refused_with_its_line` |
| "...with a Steam implementation first..." | Done | argus-steam (its own trace). | argus-steam `steam_live` (Spacewar, by hand) |
| "...and a null implementation for everything else, so game code calls one API and tests run without Steam present." | Done | Game code asks with `q:achieve`, `q:presence`, `q:overlay` (argus-luau), events (`achievement.unlock`, `achievement.progress`, `presence.set`, `overlay.open`, `REQUESTS`) that whatever runs the game hands its platform between ticks (`answer`), whichever it is. `NullPlatform` keeps what it is told, its cloud in memory or a folder; it is the platform of the daemon, the web page, the tests, and a release without Steam. An achievement `platform.toml` does not declare is refused before the platform hears of it (`platform.error`). | `platform::requests_reach_the_platform_and_its_answers_come_back`, `platform::two_null_platforms_sharing_a_folder_share_a_cloud`; argus-player `platform`; argus-daemon `platform`; Hollow's `platform` and `platform_deep`, Outpost's `cloud` |
| Cloud saves. | Done | A save goes to the disk and the platform's cloud under one file name (argus-player's `Player::store_save`); a load takes the copy saved later (`savegame::newer`, by the save's `saved_at`; the disk's on a tie). `[cloud] saves = false` keeps them on the disk. | argus-player `platform::a_save_in_the_cloud_loads_on_another_machine`; argus-daemon `platform::a_save_loads_from_the_cloud_when_the_disk_has_none` |
| Input glyphs. | Done | argus-input's `InputGlyph` (labels for every family, from the engine's own tables); a platform with pictures of its buttons supplies them (`glyph_image`), drawn as the sprite `glyph/<family>/<input>`. | argus-input `glyphs`; argus-steam `steam_live` |
| The overlay. | Done | `open_overlay(page)`; `overlay_open()`, which a host polls (argus-player's `Player::poll_platform`) and turns into the action map's `overlay` input, so a game pauses on it and a replay holds it. | argus-player `platform::requests_reach_the_platform_and_the_overlay_is_an_input`; argus-daemon `platform::the_overlay_is_an_input_and_a_seek_replays_across_requests`; Hollow's `platform` |

## §6 Determinism

| Spec | Status | Where | Test |
|---|---|---|---|
| A run replays the same. | Done | Nothing the platform says goes back into the world: requests are outputs, as `sound.play` is, and the answers go to the host's log (`platform.achieved`, `platform.error`), never a system's. The overlay and the device in use are inputs, logged like keys. A run on Steam and the same run on the null platform reach the same world. | argus-daemon `platform::the_overlay_is_an_input_and_a_seek_replays_across_requests` |

## Known limitations, stated so they are decisions

- One platform at a time, and Steam the only one besides the null platform; another store is another implementation of the same trait.
- Stats, leaderboards, and multiplayer are not in the interface: §15 names achievements, cloud saves, rich presence, input glyphs, and the overlay.
- A save is the unit of the cloud: settings and reports stay on the disk.
- The null platform of a release without Steam keeps its cloud in memory: it is the disk alone that keeps the saves.
