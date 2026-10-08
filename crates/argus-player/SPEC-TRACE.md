# argus-player: spec trace

Every statement in the design document (v0.3) that constrains a game as players get it, as phase 2 item 7 scopes it (§15 "Shipping", §6 run identity and replays, §10 marks, §18 phase 2 and its gate), with where it lives and the test that pins it. This crate is the runtime a release build, the web page, and the daemon share; `argus-game` is the release executable, `argus-cli`'s `argus build` makes a release, and the daemon turns reports into marks. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §15 The player runtime

| Spec | Status | Where | Test |
|---|---|---|---|
| A release build is the same core. | Done | `assemble`: one function builds a scene's world (input, physics, camera, post, animators, tweens, particles, scripts, the scene, system groups, assets, the mixer), and the daemon's `session.open` and `scene.load`, the web page's `Player`, and `argus-game` all call it. Building it found the copies had drifted: the web player never applied a scene's `systems.enable`/`disable`. | the daemon's tests; `tests/player.rs`; `tests/packs.rs::hollow_from_its_packs_plays_as_hollow_from_its_folder` |
| With the protocol server compiled out, the editor and index absent. | Done | `argus-game` depends on `argus-player` and the runtime crates only; the daemon, the index, the CLI, the socket library, and SQLite are not in its dependency tree, and none of the protocol's method names are in the executable. There is no editor yet (phase 4). | checked on the release build of Hollow: `cargo tree -p argus-game` and the executable's strings (docs/phase2-checklist.md item 7) |
| Luau precompiled to bytecode. | Done | `argus_luau::compile` (the options loading source uses, so a script runs the same and keeps its line numbers); a pack carries bytecode; the host loads source or bytecode by its first byte. | `argus-luau/tests/bytecode.rs`; `tests/packs.rs` |
| Assets packed. | Done | `pack::Pack`: a manifest (path, address, size) and blobs; `pack::build` makes a project's. | `tests/packs.rs` |
| No socket, no `.argus/` directory, and no way to attach. | Done | The executable reads its packs into memory (`argus_os::fs::mount` on the desktop too): every path under the game's root is memory, writes included, so the importer's cache and anything else a loader writes never reach the disk. A run of the release build leaves nothing beside it but its own files. | `argus-os/tests/mount.rs`; the release build of Hollow |
| The dev-only surface is a Cargo feature, not a runtime flag, so it cannot be turned on in the field. | Done (stronger) | The dev surface is a crate (`argus-daemon`) the release executable does not link at all, so no flag or feature of the shipped binary reaches it. `argus-player`'s own features (`render`, `audio`, `desktop`) choose runtime parts, never a protocol. | as above |
| `argus build --release --target windows` produces a single directory: executable, one asset pack per language, and a save location per platform convention. | Done | `argus build` (argus-cli): `<game>.exe` and `data/` with `game.pack` and `lang-<code>.pack` per `lang/<code>/`; `build.json` says what was made. The save location is the platform's, not beside the executable, which may be read-only: `save::save_dir` (`%APPDATA%\<game>`, `~/Library/Application Support/<game>`, `$XDG_DATA_HOME/<game>` or `~/.local/share/<game>`). Reports go under it. | `save::tests`; `argus-cli`'s build on Hollow |
| The recorder stays, in a bounded form: the last thirty seconds of actions and a keyframe. | Done | `recorder::Recorder`: a keyframe every 30 s of sim time (30 turns on a discrete clock), the two newest kept, the input journal and value log before the older one forgotten (`InputState::forget`). A report reaches back between 30 and 60 seconds. | `tests/recorder.rs::the_recorder_stays_bounded_and_a_report_replays_to_the_same_world` |
| (Phase 5 item 1.) A desktop player's report reaches the team as the page's does. | Done | `reports::send_waiting`: the reports in the save location's `reports/` go to the project's playtest server (`[reports] url` in `argus.toml`, an `http://` address) as `POST <url>marks`, the page's request, oldest first, and move to `reports/sent/`; one the server did not take waits. `argus-game` sends a report when it is saved and the waiting ones when the game starts, in the background, and says on its bottom line what became of them; a crash's report gets three seconds before the game stops. The HTTP client moved here from the daemon (`http::post` beside `get`). Hollow names klab's server. | `tests/release_reports.rs`; a real Hollow report posted to `tools/web/serve.mjs` by hand |
| So that a player pressing a report key or hitting a crash produces a replay file the team can open. | Done | `Player::report` writes a replay file (`replay`), the note in its identity. `argus-game`: F8 (or `[marks] key`) pauses and asks for a note on the window's bottom line, Enter saves `reports/<time>-report.argr` in the save location; a panic saves `-crash.argr` (no world at the end, since the tick was cut short) and tells the player where. The web page's F8 posts the same file to the server it came from. | `tests/recorder.rs`; `argus-daemon/tests/reports.rs`; `tools/web/check.mjs` step 4 |
| A player bug report becomes a mark. | Done | `mark.import` takes a report and it is a mark like any other (`mark.open` goes back to its keyframe and replays to its frame, and compares the world it saved); `mark.pull` fetches the reports a playtest server collected. | `argus-daemon/tests/reports.rs::a_report_is_a_mark_that_reproduces_and_pull_fetches_each_once` |
| Save migrations stay, because they are how a patched game loads an old save. | Done (phase 3 item 0) | `savegame.rs`: a save file (`ARGS`) is the game's identity and a keyframe without the input state; `SaveFile::read_into` reads it through the script host's migrations (compiled from bytecode as from source) and refuses a save written by a newer version. The player answers `q:save` with save files of the tick that asked (`Player::take_saves`, from a tick-end hook) and `q:load` with names (`Player::take_loads`); `Player::load` opens the save's scene fresh, puts its state back, and leaves the game as it was on an error. A release build keeps saves in its save location's `saves/`, the web page in IndexedDB. | `saves::the_game_saves_at_the_tick_it_asks_and_a_save_loads_back` |

## §15 Targets

| Spec | Status | Where | Test |
|---|---|---|---|
| Windows from day one. | Done | `argus build` on Windows; the executable links its C and C++ runtimes statically (carried from phase 1) and loads only Windows' own DLLs. | the release build of Hollow: 15.8 MB, its headless check |
| Linux from day one. | Done | `argus build --release` on klab (Ubuntu 24.04, `libasound2-dev` and `libudev-dev` for sound and gamepads): a 13.6 MB executable in 1 min 20 s, saving to `~/.local/share/<game>`. Its self-check passed, and the world after 300 ticks is the one the Windows release reaches, bit for bit (`b41656c1f507aaf2`). | klab's build of Hollow (docs/playtest.md) |
| macOS from day one. | Deferred | The same code path; `argus build --target macos` runs on a Mac, and there is none here. | |
| The web target (wasm + WebGPU, with a WebGL2 fallback). | Done | `argus build --target web`: the page (`tools/web/index.html`, `main.js`), the wasm module, the packs in `data/` and `data/packs.json`; the page loads them with `WebPlayer.from_packs`. | `tools/web/check.mjs` on the built site: WebGPU and forced WebGL2 |
| A shareable link is the cheapest way to get a human's feel onto a build the agent made. | Done | Hollow's web build runs on klab (`argus-play.service`, `tools/web/serve.mjs`, docs/playtest.md): the page, and the reports its players post, which `mark.pull` fetches over the tailnet or an SSH tunnel. A report posted there was pulled and opened as a mark that reached the same world. The link is `https://klab.tailde2255.ts.net:8443/`, for the tailnet, with a Let's Encrypt certificate through `tailscale serve`, so the page draws with WebGPU; a report posted over it was stored and listed. `tools/web/deploy.sh` puts up a new build. | `tools/web/check.mjs`; the round trip through klab's server |

## §15 Assets and content

| Spec | Status | Where | Test |
|---|---|---|---|
| A release pack is built from the import cache, not the source tree. | Done | `pack::build` imports first (a failed import stops the build), then packs each sprite's cached texture and what the importer made of it (`argus_assets::Importer::packed_sprites`: frames, clips, tiles, pivots, the catalog), and leaves the source images and sprite sidecars out; the release build's importer loads sprites from the catalog (`Importer::packed`) and imports only sounds, animators, emitters, and materials, which are small text. | `tests/packs.rs` |
| Compressed textures in atlases. | Done (phase 5 item 1) | `argus_assets::pages`: `pack::build` puts every sprite on a page it shares with others (shelves, tallest first, at most 2048 square, WebGL2's least), each in a cell of its own: its pixels with its edge pixels repeated twice around them, rounded out to four-pixel blocks, so a page samples at a sprite's edge as its own texture did and no block holds two sprites. A sprite goes on a BC7 page when its compressed pixels look the same to a person (`bc7::looks_same`: alpha 0 and 255 exact, other alpha within 8, colour over black and over white within the image goldens' threshold), and on a lossless PNG page otherwise. The encoder (`argus_assets::bc7`) writes BC7's two one-subset modes, which need none of the format's partition tables. The renderer loads a page once for all its sprites, as BC7 where the device samples it and decoded on the CPU where not, and draws a page's sprites as one run. Hollow: 8 sprites on 2 pages, 6 in BC7. `argus build` now draws a frame of the project and of the build in software and refuses a build whose pixels differ (`check.looks`). | `tests/release_pages.rs` (Hollow from its packs draws as from its folder, pixel for pixel), `argus-render` `pages::bc7_blocks_draw_as_they_decode_here`, `argus-assets` `bc7` |
| Audio in the platform's preferred codec. | Done (phase 5 item 1) | Ogg Vorbis. `pack::build` encodes each WAV (libvorbis through `vorbis_rs`, quality 0.5, the stream's serial taken from the WAV's bytes, so a sound packs to the same blob every build) and keeps its path, so `audio.toml` and sidecars name it as before. `argus_audio::sound::decode` tells WAV from Vorbis by their first bytes (`lewton`, pure Rust, so the page decodes it too), and an authored `.ogg` is a sound as a WAV is. A tenth of the size, at 38.5 dB signal to noise on a test chord. Found: a sound file was read from the disk, never through the engine's file system, so a release build, whose files are in memory, had no sounds from files at all; none of the four games has one, which is why nothing failed. | `tests/release_audio.rs` |
| Defs and scenes in a binary form of the same schema. | Done (phase 5 item 1) | `argus_scene::binary`: what the loaders make of the TOML (an entity file's document, a def table's rows), the values still TOML values until a schema types them, each with its line. A scene's or room's entity files become one `entities.bin`, the prefabs one `prefabs/prefabs.bin`, a def table `defs/<table>.bin`. A file that did not parse stays TOML and is reported as it would have been, and a file beside a bundle lays over its entry (a language pack's does). Hollow: 10,078 entity files in 201 bundles, its packs 1,372 files where they were 11,255, and its scene opens in 71 ms where it took 192 (release, in memory). | `argus-scene` `tests/binary.rs` (the same world and tables, the same errors at the same lines), `tests/packs.rs` |
| Packs are content-addressed so a patch ships only changed chunks. | Done | Each file's bytes are named by the first 16 bytes of their SHA-256 (`sha256`, checked against the standard vectors). `argus build --since <previous build>` writes `patch-<n>.pack`: the whole manifest and only the blobs the builds before it lack; the player reads every pack in `data/`, takes the newest manifest, and finds each blob in whichever pack holds it. A patch without its base is refused by name. A blob that does not match its address is refused as damaged. | `tests/packs.rs::a_patch_carries_only_what_changed_and_a_language_lays_over_the_game` |
| Localization packs are separate so a language can be added without a rebuild. | Done | `lang/<code>/` in the project becomes `lang-<code>.pack`, its files laid over the game's with the same paths; the executable reads whatever language packs are in `data/` (`--lang <code>`, `?lang=` on the page), so a new one is a file dropped in. String tables for UI text are the UI tree's (phase 3). | same |

## §6, §10 Replays and marks

| Spec | Status | Where | Test |
|---|---|---|---|
| (Moved here from the daemon.) Keyframes and replay files. | Done | `keyframe` (`Keyframe`, `Sources`) and `replay` (the ARGR file; `encode`/`decode` for bytes, since a browser has no files) moved from the daemon, which re-exports them, so its ring, marks, and test replays share them with the recorder. | the daemon's tests |
| A report reproduces on the desktop from wherever it was taken. | Done | `Player::play_report`: back to the report's keyframe, its commands replayed to its last tick, the world compared with the one it saved. A report taken in Chrome replays natively to the same world. | `tests/recorder.rs`; `tools/web/check.mjs` step 4 (the pit's report, through the server) |

## Phase 3, item 1: the UI in the one assembly

| Spec | Status | Where | Test |
|---|---|---|---|
| (§12) The UI in every scene, the same in the daemon, the executable, and the page. | Done | `assemble.rs` loads `ui.toml` (`Theme::load`), installs `argus-ui` after the input map and the camera and, with an input map, `input.rebind`; `set_fonts` makes each font asset a shaped `SdfFont`. `Player::devices` hands a press to a waiting rebind first. | `packs::hollow_from_its_packs_plays_as_hollow_from_its_folder` (Hollow's pause and menu systems among the scripts) |

## Phase 3, item 2: navigation in the one assembly

| Spec | Status | Where | Test |
|---|---|---|---|
| (§12) Navigation in every scene, the same in the daemon, the executable, and the page. | Done | `assemble.rs` makes the `NavStore` over the session's tilemaps, installs `argus-nav` after the physics (its agents move after the physics step), and hands scripts the path queries (`NavPaths`, the `argus_luau::Paths` of the store). `Assembly.nav`, `Player.nav`. | Delve's tests; `argus-daemon` `nav.rs` |

## Phase 3, item 3: jobs, tables, and speed in the one assembly

| Spec | Status | Where | Test |
|---|---|---|---|
| (§12) The job system in every scene, the same in the daemon, the executable, and the page. | Done | `assemble.rs` installs `argus-jobs` over the session's navigation, after it. | `argus-daemon` `jobs.rs`; Outpost's tests |
| (§4) Scripts read the project's data tables. | Done | `ProjectDefs`, the project's `Defs` as `argus_luau::DefRows`, handed to the host after the tables are read (`q:def`, `q:defs`). | Outpost's tests |
| (§6) A game's speed controls. | Done | The scheduler answers `speed.requested` at the end of the tick that asked (since 2026-09-30; the player set it after the step before), so the player, the daemon, and the page agree with every replay. | Outpost's `tests/colony.luau` (through the daemon); `argus-sim` `clocks::a_speed_a_system_asks_for_takes_effect_at_the_end_of_its_tick`; the executable and the page read the speed each frame |

## Phase 3, item 4: words and conversations in the one assembly

| Spec | Status | Where | Test |
|---|---|---|---|
| (§12) String tables and dialogue in every scene, the same in the daemon, the executable, and the page. | Done | `assemble.rs` reads the string tables into the UI's shared state, installs `argus-dialogue` after the UI, hands scripts `q:text` (`UiTexts`) and the rebind its prompt, adds the engine's `dialogue` def table to the project's (`with_engine_defs`), and builds the graph from its rows. | Hollow's, Delve's, and Outpost's tests |
| (§15) "Localization packs are separate so a language can be added without a rebuild." | Done | `pack.rs`: a string table other than the base language's (`strings/<code>.toml`) goes into `lang-<code>.pack`; every language pack's tables are mounted whichever language the files are laid out in, so a player switches in the game; of two builds' packs of a language, the newest is the language (the first build's was taken before). | `packs::hollow_from_its_packs_plays_as_hollow_from_its_folder`, `packs::a_patch_carries_only_what_changed_and_a_language_lays_over_the_game` |

## Phase 3, item 5: timelines in the one assembly, and overlaps in keyframes

| Spec | Status | Where | Test |
|---|---|---|---|
| (§13) Timelines in every scene, the same in the daemon, the executable, and the page. | Done | `assemble.rs` installs `argus-timeline` after the dialogue (first in the late stage), adds the engine's `timeline` def table to the project's (`with_engine_defs`), and reads the timelines with their fields checked against the world's components (`read_timelines`). | Hollow's and Outpost's tests |
| (§6) Everything the sim needs that the world does not hold is in a keyframe. | Done | `keyframe.rs`: the colliders overlapping at the end of the tick before (`Keyframe.contacts`, `Sources.contacts`) beside the particle pools and rooms. Replay files are version 2 and carry them; a version 1 file reads with none. A save starts with none. | Hollow's outro scrubs; the daemon's ring and seek tests |

## Phase 3, item 6: rooms stream in the one assembly

| Spec | Status | Where | Test |
|---|---|---|---|
| (§4) Rooms stream around the player in the daemon, the executable, and the page alike. | Done | `assemble.rs` shares the scene (`Rc<RefCell<Scene>>`) and installs `rooms.stream` (argus-scene) last in the late stage. `keyframe.rs`: `Sources.scene` takes and puts back the scene's stream state, what unloaded rooms remember included. Replay files are version 3 and saves version 2 for it; older files read with no memory. | Hollow's tests; the daemon's ring, seek, and save tests |
| (§15, §17) "Content-addressed packs; only changed chunks rebuild." | Done | Found at Hollow's 10,000 files: every pack carried every file's path, address, and size, so a one-file patch was mostly manifest (it failed the patch test's size check). Packs are version 3: a patch lists only the files added or changed since the build before and the paths it removed; resolving applies the patches in order on the base's manifest, and a patch without the base, or with a patch between missing, is refused. A version 2 patch, whose manifest is whole, still reads. | `packs::a_patch_carries_only_what_changed_and_a_language_lays_over_the_game` (the patch lists the one sign; under a hundredth of the base) |
| A report stays small as the game grows. | Done | A report is two keyframes and the world at the end, so it follows the live world, which streaming keeps to a handful of rooms: Hollow's is 85 KB with the two deep rooms live at the start (the test's bound went from 64 to 128 KB). | `recorder::the_recorder_stays_bounded_and_a_report_replays_to_the_same_world` |

## Phase 3, item 7

| Spec | Status | Where | Test |
|---|---|---|---|
| (§13) Music on the grid in every build. | Done | The player advances the mixer to the sim's time before it reads `Music`, as the daemon does; `assemble.rs` registers `InRoom` before scripts load and installs `rooms.stream` first in the input stage. | Hollow's and Outpost's tests |

## Phase 3, item 8: mod packs

| Spec | Status | Where | Test |
|---|---|---|---|
| (§15) "A release build can load mod packs ... from a user directory" | Done | `mods.rs`: `find` reads a folder's mod directories (each with a `mod.toml`) and mod packs, `choose` plans them, `mount` puts each at `mods/<id>/` under the release's game in memory. argus-game reads the `mods/` folder of the save location (`--mods <dir>`, `--no-mods`). | `mods::a_mod_pack_from_a_players_folder_is_one_more_layer_over_the_games_packs`; Hollow's release executable run headless with `lanterns.pack` and `brighter.pack` |
| "additional defs, scenes, assets, and Luau systems" | Done | `assemble.rs` hands the layers (the game, then each mod) to every loader: scripts, prefabs, def rows, scenes and rooms, assets, sounds and music, string tables. Conversations and timelines are def tables, so mods carry them too. | `mods::a_mod_adds_under_its_id_and_changes_what_came_before`; Hollow's `tests/mods.luau` |
| "in a declared load order" | Done | `mods::plan`: a folder's `load-order.toml` (`order = [...]`), else every mod by id, each after the mods it requires. A session's `mods` or a scenario's `mods` is its own declared order. A declared mod not found, a mod not declared (when there is a load order), one for another game, or one before what it requires is left out, with why. | `mods::the_later_mod_wins_and_the_order_is_declared`; `mods::a_mod_for_another_game_or_before_what_it_needs_is_left_out` |
| "with the same sandbox and instruction budgets as the game's own scripts" | Done | A mod's scripts load into the game's one script host. A runaway mod system is stopped by the same budget and switched off like a game system's, and the game goes on. | `mods::a_mods_runaway_system_is_stopped_by_the_same_budget_as_the_games` |
| "and the same schema validation" | Done | A mod's rows and entities go through the game's schemas. A mod with any problem is left out whole and the scene assembled again without it; the reason points at the mod's row, with the schema's line as a hint. A mod needing one left out is left out too. | `mods::a_mod_that_breaks_a_rule_is_left_out_whole_and_the_game_runs` |
| "a load-order rule and a namespace prefix on mod content" | Done | The rules are argus-ecs `layers.rs`: everything a mod adds is named with its id first; a name without it changes something a layer before it has (def rows field by field, strings, assets, sounds, music tracks, entities, prefabs), the later layer winning. Systems, components, and def tables are only ever added. `Report.changes` says who changed what, over whom, and where. | the tests above; `mods.list` in Hollow's test |
| "Games that don't want mods set one flag." | Done | `[mods] enabled = false` in `argus.toml`: no folder is read, and asking for mods by name is refused (6004). | `mods::games_that_dont_want_mods_set_one_flag` |
| (§15) Packs are content-addressed; a mod pack is one more layer over the game's. | Done | `pack.rs`: `Kind::Mod(id)`, written as `<id>.pack`. `build_mod` packs `mods/<id>/` with scripts compiled, sprites from the mod's own import cache, and the directory's hash (`argus.cache/mod.hash`), so the pack and its directory are the same mod to a replay. The game's own pack leaves `mods/` out. | `mods::a_mod_pack_from_a_players_folder_is_one_more_layer_over_the_games_packs` |
| A modded run reproduces. | Done | Reports, saves, and marks record `mods: [{id, version, hash}]`; a save loads with the mods the player asked for. | the same test (the mark says which mods) |

## §18 Build plan

| Spec | Status | Where | Test |
|---|---|---|---|
| Phase 2: first `argus build` for desktop and web so a playtest link exists before the editor does. | Done | Windows, Linux, and web builds, packs, patches, languages, the recorder, the report key, reports into marks, and the page served from klab at its HTTPS link. macOS waits for a Mac. | above |
| Gate: a second human opens the web build from a link and drops a mark that reaches the agent the same way. | Partial | Everything the gate needs is in place and checked: the link on klab, the page's F8 posting a report to it, `mark.pull` importing it, `mark.open` reproducing it. The gate itself is a second person playing. | `tools/web/check.mjs`; `argus-daemon/tests/reports.rs`; the report round trip through klab |

## The phase 3 gate

| Spec | Status | Where | Test |
|---|---|---|---|
| The tick-end hook that finds `q:save` requests is cheap in a long step. | Done | `savegame::requested` reads only the tick's events at the queue's tail. | Outpost's `tests/week.luau` |
| What a report or a page can say of a player's memory. | Done | `Player::status` has `script_bytes` (the Luau heap) and `strings` (the string table's size). Used at the gate to find that the player does not leak: three hours of Outpost stayed at 19 to 23 MB once its saves were collected, as the executable collects them every frame. The heap's size is not the world's: it depends on when the collector ran and on whether the scripts were compiled or shipped as bytecode, so the tests that compare runs leave it out. | the gate's measurements; `packs`, `player` |

## Phase 5, item 2: a player's settings

| Spec | Status | Where | Test |
|---|---|---|---|
| Settings that last between launches, outside the world, since a replay must not change with them. | Done | `settings`: the text size, high contrast, fullscreen, and language of the game's `UiSettings`, and the bindings and toggles that differ from the game's, as TOML (`settings.toml` in the save location; the web page keeps the text in the browser). `Store::open` puts them in before the first tick and before the recorder starts, so its first keyframe holds them and a report replays with the player's settings in a player without them; `Store::changed` writes them back when the player changes them. The daemon does not read them. | `tests/release_settings.rs` |
| (§12) The player's screen reader reads the game. | Done | The desktop game builds the frame's AccessKit tree while one is reading and turns its presses into the pointer and a click (`argus-game`). | `tests/release_access.rs`: Hollow's pause menu, by its words, the focus following, and a press opening Controls |

## Known limitations, stated so they are decisions

- A desktop report goes over plain HTTP to the server's tailnet address (`[reports] url`): a player off the tailnet keeps theirs, and it goes when they are on it, or they send the file.
- BC7 pages use two of the format's eight modes. A sprite with a block those cannot hold (three colours in four by four pixels, which much pixel art has) ships lossless.
- A patch ships every page when a sprite is added or resized (the layout moves), and a room's whole bundle when one of its entities changes.
- Packs hold numbers at their full width, so Hollow's are 3.3 MB where the TOML was 2.6; over the wire, compressed, they are smaller than before (2.5 MB).
- The web page's report key is F8; `[marks] key` applies to the desktop executable.
- `argus build` builds the executable from the engine's source tree with cargo (`ARGUS_ENGINE`, default the tree the CLI came from). A packaged engine would ship prebuilt executables per platform.
- A crash that corrupts memory rather than panicking is not caught; a report needs the process alive to write it.
- `mark.pull` speaks plain HTTP: it pulls from the server's tailnet address, or through an SSH tunnel, not the HTTPS link.
- A Linux release needs the glibc of the machine that built it or newer (2.39 for klab's Ubuntu 24.04). A release for older distributions is built on an older one.
- Windows and Linux reach the same world for Hollow (§6 promises the same platform only); a game using platform-dependent float functions might not, and nothing yet checks that across platforms routinely.
- Mods (phase 3 item 8) cannot remove anything the game has (a def row, an entity, a room); an entity a mod replaces is replaced whole, where a def row is changed field by field.
- A mod cannot change `argus.toml`, `input.toml`, `ui.toml`, a scene's `scene.toml`, or a room's `room.toml` it did not make, nor replace or switch off a game's system.
- The web page loads no mods: it has no folder of the player's to read. A page that takes mod packs dropped on it could follow.
- A player edits `load-order.toml` by hand; there is no in-game list of mods to switch on and off.
