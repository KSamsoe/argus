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
| Compressed textures in atlases. | Partial | The cached textures are PNG, one per sprite (Hollow's five are 24 KB). Atlas packing and GPU texture compression wait until a game's texture memory or load time asks for them. | |
| Audio in the platform's preferred codec. | Deferred | WAV as authored; Hollow's sounds are tones and short WAVs. | |
| Defs and scenes in a binary form of the same schema. | Deferred | They ship as TOML text and parse in milliseconds at Hollow's size. | |
| Packs are content-addressed so a patch ships only changed chunks. | Done | Each file's bytes are named by the first 16 bytes of their SHA-256 (`sha256`, checked against the standard vectors). `argus build --since <previous build>` writes `patch-<n>.pack`: the whole manifest and only the blobs the builds before it lack; the player reads every pack in `data/`, takes the newest manifest, and finds each blob in whichever pack holds it. A patch without its base is refused by name. A blob that does not match its address is refused as damaged. | `tests/packs.rs::a_patch_carries_only_what_changed_and_a_language_lays_over_the_game` |
| Localization packs are separate so a language can be added without a rebuild. | Done | `lang/<code>/` in the project becomes `lang-<code>.pack`, its files laid over the game's with the same paths; the executable reads whatever language packs are in `data/` (`--lang <code>`, `?lang=` on the page), so a new one is a file dropped in. String tables for UI text are the UI tree's (phase 3). | same |

## §6, §10 Replays and marks

| Spec | Status | Where | Test |
|---|---|---|---|
| (Moved here from the daemon.) Keyframes and replay files. | Done | `keyframe` (`Keyframe`, `Sources`) and `replay` (the ARGR file; `encode`/`decode` for bytes, since a browser has no files) moved from the daemon, which re-exports them, so its ring, marks, and test replays share them with the recorder. | the daemon's tests |
| A report reproduces on the desktop from wherever it was taken. | Done | `Player::play_report`: back to the report's keyframe, its commands replayed to its last tick, the world compared with the one it saved. A report taken in Chrome replays natively to the same world. | `tests/recorder.rs`; `tools/web/check.mjs` step 4 (the pit's report, through the server) |

## §18 Build plan

| Spec | Status | Where | Test |
|---|---|---|---|
| Phase 2: first `argus build` for desktop and web so a playtest link exists before the editor does. | Done | Windows, Linux, and web builds, packs, patches, languages, the recorder, the report key, reports into marks, and the page served from klab at its HTTPS link. macOS waits for a Mac. | above |
| Gate: a second human opens the web build from a link and drops a mark that reaches the agent the same way. | Partial | Everything the gate needs is in place and checked: the link on klab, the page's F8 posting a report to it, `mark.pull` importing it, `mark.open` reproducing it. The gate itself is a second person playing. | `tools/web/check.mjs`; `argus-daemon/tests/reports.rs`; the report round trip through klab |

## Known limitations, stated so they are decisions

- A desktop report is saved in the player's save location; the player sends the file. Posting it to a server, as the page does, can follow when a playtest ships desktop builds.
- The web page's report key is F8; `[marks] key` applies to the desktop executable.
- `argus build` builds the executable from the engine's source tree with cargo (`ARGUS_ENGINE`, default the tree the CLI came from). A packaged engine would ship prebuilt executables per platform.
- A crash that corrupts memory rather than panicking is not caught; a report needs the process alive to write it.
- `mark.pull` speaks plain HTTP: it pulls from the server's tailnet address, or through an SSH tunnel, not the HTTPS link.
- A Linux release needs the glibc of the machine that built it or newer (2.39 for klab's Ubuntu 24.04). A release for older distributions is built on an older one.
- Windows and Linux reach the same world for Hollow (§6 promises the same platform only); a game using platform-dependent float functions might not, and nothing yet checks that across platforms routinely.
