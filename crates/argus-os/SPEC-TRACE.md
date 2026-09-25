# argus-os: spec trace

Every statement in the design document (v0.3) that this crate answers: the host underneath the runtime, so the same crates run on the desktop and in the browser (§15, phase 2 item 1). Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §15 Shipping, §18 phase 2 (the web target)

| Spec | Status | Where | Test |
|---|---|---|---|
| The runtime runs on the web as well as the desktop, from the same core. | Done | Every file and clock access of the eight runtime crates (sim, luau, scene, input, assets, tiles, render, audio) goes through `argus_os::fs` and `argus_os::time`. On the desktop they are `std::fs` and `std::time`. In wasm, `fs` is an in-memory file system that `mount` fills from a pack, and `time::Instant` is the browser's clock. `read_dir` returns entries sorted by path on both, so a directory walk is the same order everywhere. | the workspace's tests natively; `argus-player/tests/player.rs` and `tools/web/check.mjs` in the browser |
| Nothing in the sim stage reads the wall clock. | Done | Unchanged by this crate: `time` serves profiling and pacing, never the sim. | |

## Phase 2, item 7: packs on the desktop

| Spec | Status | Where | Test |
|---|---|---|---|
| (§15) A release build reads its packs; no `.argus/` directory. | Done | `fs::mount` works on the desktop too: every path under a mounted root is memory, reads and writes alike (the importer's cache stays in memory), and every other path is the disk. In the browser every path is memory, as before. One store behind a mutex serves both. | `tests/mount.rs::a_mounted_pack_is_memory_and_the_rest_is_the_disk` |

## Known limitations, stated so they are decisions

- The browser's file system is read-mostly and lives as long as the page. Writes go to memory; nothing persists. Save games (phase 3 item 0) do not go through it: the page keeps them in IndexedDB. `time::unix_secs` stamps a save natively and in the browser.
- Modification times in the browser are all the Unix epoch, so a hot reload keyed on them never fires there, which is fine for a player.
