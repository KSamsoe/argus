# argus-web: spec trace

Every statement in the design document (v0.3) that constrains the web spike (phase 2 item 1: §15 targets, §6 determinism, §10 marks), with where it lives and the test that pins it. Findings and numbers are in `docs/web-spike.md`. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §15 The web target

| Spec | Status | Where | Test |
|---|---|---|---|
| The web target is wasm + WebGPU with a WebGL2 fallback, from the start. | Partial (the spike) | `Player` opens a project's scene the way the daemon does, without the daemon; `WebPlayer` (wasm only) mounts a pack, steps on `requestAnimationFrame` time on the continuous clock (at most five ticks a frame), feeds keys, and draws through `Renderer::web`. `tools/web/build.sh` builds the module and the site. The release build is item 7. | `tools/web/check.mjs` (headless Chrome, WebGPU and forced WebGL2) |
| Nothing in the player runtime listens on any socket. | Done | The web player has no daemon and no protocol at all. | |
| Assets are packed. | Partial | A pack is every file of the project but dot-files and `tests/`, in path order: `ARGUSPK1`, a count, then each path and its bytes. Content addressing is item 7's. | `tests/player.rs::a_pack_round_trips_a_project` |
| Luau runs in the release runtime. | Done | Luau (C++) compiles to wasm with wasi-sdk and wasm's exnref exception handling; the C library's system calls are answered inside the module (`wasi.rs`). `tools/web/env.sh` has the flags and why. | the check runs Hollow's scripts in Chrome |

## §6 Determinism across the desktop and the browser

| Spec | Status | Where | Test |
|---|---|---|---|
| Cross-platform bit-determinism is a non-goal; the spike measures whether it holds. | Done (measured) | `Player::digest` hashes every field of every live entity with floats by their bits, and the event digest hashes every event's frame, name, and fields. Hollow's run scenario gives the same digests natively, in Chrome, and in wasm under Node, at every checkpoint, from a release and a size-tuned build. | `tests/player.rs::hollow_runs_without_the_daemon_and_twice_the_same` (`--nocapture` prints the digests), `tools/web/check.mjs` |

## §10 Marks from the browser

| Spec | Status | Where | Test |
|---|---|---|---|
| A mark is a fully reproducible bug report. | Partial (the spike) | `WebPlayer::mark(note)`: scene, seed, code revision (`ARGUS_CODE` at build time), tick, the input log, the world and event digests, and the note. `Player::replay_log` schedules the log from load, and the `mark_replay` example replays a mark natively and compares digests. Marks as the daemon keeps them are item 2; posting them from the browser is item 7. | `tools/web/check.mjs` takes a mark after real key presses and replays it natively (`same: true`) |

## Known limitations, stated so they are decisions

- `Player::open` copies the daemon's `open_scene`. Item 7 shares one assembly.
- Natively `argus-web` is a library only: as a Windows DLL it would export more symbols than a DLL may hold. `build.sh` asks for the cdylib for wasm.
- Only Chrome was checked. A browser without wasm's exnref exception handling refuses the module.
- The page opens audio on the first key or click, as browsers require.
