# argus-os: spec trace

Every statement in the design document (v0.3) that this crate answers: the host underneath the runtime, so the same crates run on the desktop and in the browser (§15, phase 2 item 1). Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §15 Shipping, §18 phase 2 (the web target)

| Spec | Status | Where | Test |
|---|---|---|---|
| The runtime runs on the web as well as the desktop, from the same core. | Done | Every file and clock access of the eight runtime crates (sim, luau, scene, input, assets, tiles, render, audio) goes through `argus_os::fs` and `argus_os::time`. On the desktop they are `std::fs` and `std::time`. In wasm, `fs` is an in-memory file system that `mount` fills from a pack, and `time::Instant` is the browser's clock. `read_dir` returns entries sorted by path on both, so a directory walk is the same order everywhere. | the workspace's tests natively; `argus-web/tests/player.rs` and `tools/web/check.mjs` in the browser |
| Nothing in the sim stage reads the wall clock. | Done | Unchanged by this crate: `time` serves profiling and pacing, never the sim. | |

## Known limitations, stated so they are decisions

- The browser's file system is read-mostly and lives as long as the page. Writes go to memory; nothing persists. Save games are phase 3.
- Modification times in the browser are all the Unix epoch, so a hot reload keyed on them never fires there, which is fine for a player.
