# argus-game: spec trace

The release executable (§15, phase 2 item 7): what a player double-clicks. The runtime it runs, the packs, and the recorder are `argus-player`'s, and their rows are in `crates/argus-player/SPEC-TRACE.md`; these are the executable's own. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §15 Shipping

| Spec | Status | Where | Test |
|---|---|---|---|
| A player who has never heard of a daemon can double-click it. | Done | `argus build` puts it beside `data/` under the game's name; it mounts the packs, opens the game with `argus_player::Player`, and plays it in a 1280x720 window with the keyboard, the mouse, and gamepads (`argus_player::desktop`, shared with the daemon's window), sound through the mixer's device, and the continuous clock paced by elapsed time (at most five ticks a frame, the window's vsync pacing the loop). A release build has no console window; a failure to start or a crash is shown in a message box. | the release build of Hollow, headless (the window was not opened for Kevin: see docs/phase2-checklist.md item 7) |
| No socket, no `.argus/` directory, and no way to attach. | Done | The crate links `argus-player` and the runtime crates, never the daemon, the index, or the CLI; the game's files are in memory under a root that is not on disk. | `cargo tree -p argus-game`; the executable's strings; a run leaves nothing beside it |
| A player pressing a report key produces a replay file. | Done | F8, or `[marks] key` in `argus.toml`: the game pauses, the bottom line asks for a note (the bar the daemon's F8 prompt uses, drawn by the UI since phase 3 item 1), Enter saves `reports/<unix time>-report.argr` in the save location, Escape goes back; closing the window keeps a half-typed note. The bottom line then says where the report went. | `argus-player/tests/recorder.rs`; `argus-daemon/tests/reports.rs` |
| Hitting a crash produces a replay file. | Done | A panic in a frame is caught: the report is saved as `-crash.argr` with the panic's message and place as its note, and the player is told where. | the report path above; a crash itself was not provoked in a release build |
| `argus build` checks what it built. | Done | `--headless <ticks>` runs the game with no window or sound device and prints the world's digest; `argus build` compares it with the project's. | the builds of Hollow |

## Phase 3, item 3

| Spec | Status | Where | Test |
|---|---|---|---|
| (§6) Outpost's 1x/3x/10x in a release build. | Done | A frame owes `dt x hz x speed` ticks (at most five frames' worth at that speed), and the speed is the one a game asked for with `q:speed(x)` (the scheduler sets it at the end of the tick that asked). | The daemon's and Outpost's tests; a window by hand |

## Phase 3, item 8: mods

| Spec | Status | Where | Test |
|---|---|---|---|
| (§15) "A release build can load mod packs ... from a user directory, in a declared load order." | Done | The executable reads the `mods/` folder of the save location (`--mods <dir>` for another, `--no-mods` for none), plans and mounts its mods over the game, and says on the bottom line which mods did not load and why. `--headless` prints the mods report. | Hollow's release run headless: both packs by id, `brighter` after `lanterns`; a load order putting `brighter` first leaves it out; `--no-mods` |

## Phase 5, item 1: reports reach the team

| Spec | Status | Where | Test |
|---|---|---|---|
| (§15) A report "the team can open": a desktop player's goes to them as the page's does. | Done | With `[reports] url`, a saved report is posted in the background (`argus_player::reports`), the waiting ones when the game starts, and a crash's for up to three seconds before the game stops; the bottom line says whether it went. Without, it stays in the save location as before. | `argus-player` `tests/release_reports.rs` |

## Phase 5, item 2: the player's screen reader, and settings kept

| Spec | Status | Where | Test |
|---|---|---|---|
| (§12) The semantic frame "can drive a player's narrator": the player's own screen reader. | Done | While a screen reader is reading, each frame's `ui` and `hud` go to it as an AccessKit tree (`argus-render` `access`, the window's adapter); a press it asks for is the pointer on the button and `ui_click`, recorded input like a mouse's. The window opens hidden until the adapter is attached, as AccessKit needs. | `argus-player` `tests/release_access.rs` (Hollow's menu read, the focus following, Controls pressed); Kevin's hand check with Narrator or NVDA |
| Settings that last between launches. | Done | `argus_player::settings::Store`: `settings.toml` in the save location, read and put in before the first tick and the recorder, written when the player changes one. | `argus-player` `tests/release_settings.rs` |

## Phase 5, item 9: the game on Steam

| Spec | Status | Where | Test |
|---|---|---|---|
| (§15) The Steam implementation where the game is on Steam, the null one everywhere else. | Done | Built with the `steam` feature (`argus build` passes it for `modules = ["steam"]`), the game starts Steam for `platform.toml`'s app before its window, so Steam's overlay draws into the graphics device; without Steam it runs on the null platform and the bottom line says so. Each frame polls the platform (callbacks, the overlay as an input). Steam's pictures of the pad buttons the action map binds become the sprites `glyph/<family>/<input>`. Saves go to the disk and Steam Cloud, and a load takes the newer. | argus-steam `steam_live`; argus-player `platform` |

## Known limitations, stated so they are decisions

- The window opens at 1280x720 and can be resized. It fills the screen (borderless) while the game's `UiSettings.fullscreen` says so, read each frame (phase 3 item 1); a game's settings screen is its own UI (every reference game's pause menu has one). Settings last between launches (phase 5 item 2).
- The mouse is the UI's pointer: the window's cursor goes onto the canvas through the renderer's fit.
- A report goes to the team when the project names a `[reports] url` (phase 5 item 1); without one, the player finds it in the save location.
- Saves (phase 3 item 0): what a game asks for with `q:save` goes to the save location's `saves/<name>.save`, and `q:load` reads it back; a failure is said on the bottom line, a success is the game's to mention. `--headless N` writes the saves its ticks asked for and lists them.
