# argus-web: spec trace

Every statement in the design document (v0.3) that constrains the web spike (phase 2 item 1: §15 targets, §6 determinism, §10 marks), with where it lives and the test that pins it. Findings and numbers are in `docs/web-spike.md`. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §15 The web target

| Spec | Status | Where | Test |
|---|---|---|---|
| The web target is wasm + WebGPU with a WebGL2 fallback, from the start. | Done | `Player` opens a project's scene the way the daemon does, without the daemon; `WebPlayer` (wasm only) mounts a pack, steps on `requestAnimationFrame` time on the continuous clock (at most five ticks a frame), feeds keys, and draws through `Renderer::web`. `tools/web/build.sh` builds the module and the site. The release build is item 7. (finished in phase 2 item 7: this trace's release page row; argus-player's web target row) | `tools/web/check.mjs` (headless Chrome, WebGPU and forced WebGL2) |
| Nothing in the player runtime listens on any socket. | Done | The web player has no daemon and no protocol at all. | |
| Assets are packed. | Done | A pack is every file of the project but dot-files and `tests/`, in path order: `ARGUSPK1`, a count, then each path and its bytes. Content addressing is item 7's. (finished in phase 2 item 7: argus-player's trace, the content-addressed packs row) | `tests/player.rs::a_pack_round_trips_a_project` |
| Luau runs in the release runtime. | Done | Luau (C++) compiles to wasm with wasi-sdk and wasm's exnref exception handling; the C library's system calls are answered inside the module (`wasi.rs`). `tools/web/env.sh` has the flags and why. | the check runs Hollow's scripts in Chrome |
| (Phase 2 item 6.) The post stack in the browser. | Done | The web player installs `argus-post` as the daemon does, and `Renderer::web` runs the same passes through WebGPU or WebGL2. | `tools/web/check.mjs` step 4: the hero runs into the hall's pit in Chrome, stands still through the stop, and a mark taken two ticks into it replays on the desktop to the same world and events (flash, shake, and ticks left are world state); its screenshot shows the red flash |

## §6 Determinism across the desktop and the browser

| Spec | Status | Where | Test |
|---|---|---|---|
| Cross-platform bit-determinism is a non-goal; the spike measures whether it holds. | Done (measured) | `Player::digest` is `World::digest` (moved into `argus-ecs` by item 2, so the daemon's marks compare the same one): every field of every live entity with floats by their bits. The event digest hashes every event's frame, name, and fields. Hollow's run scenario gives the same digests natively, in Chrome, and in wasm under Node, at every checkpoint, from a release and a size-tuned build. | `tests/player.rs::hollow_runs_without_the_daemon_and_twice_the_same` (`--nocapture` prints the digests), `tools/web/check.mjs` |

## §10 Marks from the browser

| Spec | Status | Where | Test |
|---|---|---|---|
| A mark is a fully reproducible bug report. | Partial (the spike) | `WebPlayer::mark(note)`: scene, seed, code revision (`ARGUS_CODE` at build time), tick, the input log, the world and event digests, and the note. `Player::replay_log` schedules the log from load, and the `mark_replay` example replays a mark natively and compares digests. The daemon takes one into its queue with `mark.import` (item 2) and opens it by replaying from the load; posting marks from the browser is item 7. | `tools/web/check.mjs` takes a mark after real key presses and replays it natively (`same: true`); `argus-daemon/tests/marks.rs::a_web_mark_reaches_the_same_world_in_the_daemon` |

## Phase 2, item 7: a release build's page

| Spec | Status | Where | Test |
|---|---|---|---|
| (§15) The web target from `argus build`. | Done | `WebPlayer.from_packs` takes a release build's packs (and a language); the page (`tools/web/main.js`) loads the ones `data/packs.json` lists, and falls back to the spike's single pack. The player keeps the bounded recorder from the start. | `tools/web/check.mjs` on a built site |
| (§15) A player bug report becomes a mark, from the browser. | Done | F8 on the page pauses and asks what happened; Send posts `WebPlayer.report(note)` (a replay file) to `marks` on the server the page came from (`tools/web/serve.mjs` stores it), and offers it as a file where there is no server. `argus mark pull` imports them. `examples/mark_replay` replays a report natively. | `tools/web/check.mjs` step 4: the pit's report, posted, fetched, replayed to the same world |

## Phase 3, item 1: the UI in the page

| Spec | Status | Where | Test |
|---|---|---|---|
| The web page draws the UI the same way as the desktop. | Done | The same renderer draws it; the page hands the mouse to `WebPlayer::pointer` (CSS pixels to the canvas) and `mouse`; `semantic()` returns the semantic frame, `fullscreen()` whether the game's settings ask for fullscreen, which the page asks the browser for (a browser grants it just after a key press or a click). | `tools/web/check.mjs`: Escape opens Hollow's pause menu, the real mouse focuses Restart the run and a click presses it (the hero back at the cave mouth), on WebGPU and with WebGL2 forced |

## Phase 3, item 3

| Spec | Status | Where | Test |
|---|---|---|---|
| (§6) A game's speed controls on the page. | Done | `WebPlayer::frame` owes ticks at the clock's speed, which `q:speed(x)` sets at the end of the tick that asked (the scheduler's). | The page's check (Hollow, at 1x); Outpost by hand |

## Known limitations, stated so they are decisions

- `Player::open` copies the daemon's `open_scene`. Item 7 shares one assembly.
- Natively `argus-web` is a library only: as a Windows DLL it would export more symbols than a DLL may hold. `build.sh` asks for the cdylib for wasm.
- Only Chrome was checked. A browser without wasm's exnref exception handling refuses the module.
- The page opens audio on the first key or click, as browsers require.
- Saves (phase 3 item 0, decided with Kevin): `WebPlayer::take_saves`, `take_loads`, and `load`; the page keeps saves in IndexedDB (a database per game, `argus:<game>`, a save per name), so a reload keeps them. Checked in headless Chrome on Outpost: the day's autosave stored, loaded back to its tick, still there after a reload, and a damaged one refused with the game going on.
