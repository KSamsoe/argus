# argus-plugins: spec trace

Every statement in the design document (v0.3) about Rust plugins (§2, §4, §7, §15, §17, §19, and §18's phase 5 sentence), with where it lives and the test that pins it. This crate is the engine's side: building, loading, checking, installing, reloading, and every call a plugin makes into the world. The plugin's side, the SDK, is `argus-plugin` (its own trace). Statuses: **Done**, **Partial**, **Deferred**.

## §7 Scripting and hot reload

| Spec | Status | Where | Test |
|---|---|---|---|
| Hot-path systems "are Rust, either in the engine or in a project plugin crate, and are compiled to a dynamic library the daemon loads." | Done | A plugin is a crate in `plugins/<name>/` that `argus.toml`'s `plugins` lists, a `cdylib`. `build` runs cargo on it (`--release`, its build in `.argus/plugins/target`); `load` copies the library to `.argus/plugins/loaded/` and loads the copy (Windows locks a loaded file, and the next build must write over cargo's), checks the ABI version, and copies the declaration out; `install` checks it against the world and registers its systems. `load_all` does all three for every listed plugin, in `argus.toml`'s order, after the scripts (whose components the plugin reads). | `plugins::a_plugin_s_systems_run_and_a_panic_stops_only_its_own` |
| "For Rust plugins, the daemon snapshots, unloads the old dylib, loads the new one, restores, and continues." | Done | `reload` builds each plugin again; when the library changed it loads and checks the new one first, then snapshots the world and the sim state, swaps every system's body (`replace_system` keeps each one's place in the run order and its random stream), lets the old library go when the last system holding it does, and restores. The new library is loaded before the old one goes (the order differs from the sentence's) so a library that does not load or check keeps the old one running. | `plugins::reload_swaps_the_library_and_the_run_continues` (the digest and the clock are the same across the swap; the old copy is gone; the new code runs; a build that fails keeps the old library) |
| "The agent sees a compile error as a structured `script.errors` response with file:line and the compiler's message, never as a crash." | Done | `build` reads cargo's JSON messages: each error with its primary span is a `Diagnostic` at a project-relative path, with the compiler's one-line message and its whole rendered text. Cargo's own refusals (a manifest it cannot read) are its stderr, at the plugin's `Cargo.toml`. A plugin that does not build keeps its last good library (cargo leaves it in place), at session open as at a reload; one that never built is missing, and an `after` naming its systems is a warning (`Scheduler::tolerate_missing`), not a refused session. | `plugins::a_compile_error_is_its_file_and_line_and_the_compilers_words`; argus-daemon `plugins::a_plugin_that_never_built_leaves_the_game_running_without_it` |
| Two rules "the engine enforces rather than recommends: a system may only touch components it declared, and a system may not hold references across frames. Violations are errors at load time where they can be detected statically and at run time otherwise." | Done | At load (`check`): every component layout the plugin reads columns as is the game's, field for field (size, alignment, each field's name, type, and offset); every system's stage and components exist; no name another system has. A plugin with any of these wrong installs nothing, and each mistake is at its `file:line`. At run time (`host`): every column fetched, row read or written, spawn, and draw is checked against the system's declaration and the column's layout fingerprint; a refusal stops the system with the engine's message. References: the SDK hands out nothing that outlives a run (its lifetimes; see argus-plugin). | `plugins::a_layout_the_game_does_not_have_is_refused_at_load`, `plugins::a_plugin_s_systems_run_and_a_panic_stops_only_its_own` (late.sneaky) |
| (§19) "The scheduler's write-set declarations make it possible to promote a Luau system to Rust without changing anything around it." | Done | A plugin system declares what a Luau system does (name, stage, after, reads, writes, spawns, rng, first, last, on_load, unfrozen, clock), and the scheduler treats the two alike. Swarm's `sim.chase` moved from `systems/chase.luau` to `plugins/swarm-hot` with the same declaration and the same arithmetic. | argus-player `release_swarm_plugin` (the same systems in the same order, and the same world every tick for 600 ticks of the arena and 40 of the stress scene) |

## §2 Isolation

| Spec | Status | Where | Test |
|---|---|---|---|
| "Rust plugin systems are loaded as dynamic libraries; a panic in one is caught at the system boundary, the system is disabled, and the fault is reported through the protocol as an event." | Done | The SDK's trampoline catches a panic in the plugin (one may not unwind into the engine) and returns its message; the host answers the run with an error at the system's declaration (code 6107), and the scheduler switches the system off and emits `system.fault` with its `file:line`, as for any system. A panic inside a query's visit is caught there and raised again once the engine's call has returned and released the columns. The host never lets a panic of its own reach the plugin. | `plugins::a_plugin_s_systems_run_and_a_panic_stops_only_its_own` (late.boom) |

## §6 Determinism, and item 5's batches

| Spec | Status | Where | Test |
|---|---|---|---|
| A plugin system is as deterministic as any. | Done | Its events, spawns, and despawns are kept and applied when its run returns, through the system's own context, so a plugin system behaves the same alone and in a parallel batch (where they wait for the batch); its random draws are the engine's own stream (`Pcg32`), so a draw is the one a Luau system makes. A system declared `.parallel()` is registered as a parallel system; the world is the same on one thread or many. | `plugins::parallel_plugin_systems_share_a_batch_and_reach_the_same_world_on_any_threads` |

## §15 Shipping, §17 targets

| Spec | Status | Where | Test |
|---|---|---|---|
| Plugins are the release build's. | Done | `Source::Prebuilt(dir)`: a release build loads `<exe dir>/plugins/<name>.<dll>` where it is (nothing rebuilds it, and an install folder may be read-only). `argus build` builds each plugin optimised and puts it there (argus-cli). | argus-cli `argus build` of Swarm (the project, the packs, and the executable run headless reach the same world) |
| "Rust plugin reload < 2 s after compile. Compile time is the cost; reload itself must not add to it." | Done | `Reloaded::reload_ms`: from the build's end to the world running on the new library. 35 ms for a test plugin; 38 to 43 ms for Swarm's chase in the daemon's release gate, after a one-line change built in 0.34 to 0.40 s (Swarm's plugin keeps `incremental = true` in its release profile). | `plugins::reload_swaps_the_library_and_the_run_continues`; argus-daemon `perf_rust_plugin_reload_under_2_s_after_compile` |
| (§19 risk) "Rust compile times on the plugin path." | Done (measured) | A plugin is its own small crate depending only on the SDK; the SDK has no dependencies. A cold build of the SDK and Swarm's plugin takes under a second here; cargo's no-op check at a session's open about 30 ms. | the build reports' `ms` |

## The phase 5 gate: what the compiler said

| Spec | Status | Where | Test |
|---|---|---|---|
| (§7) A plugin's compile error is its file, line, and the compiler's words. | Done | The row carries the primary span's label and rustc's notes after the headline (`mismatched types: expected u32, found &str`), not the headline alone. | `plugins::a_compile_error_is_its_file_and_line_and_the_compilers_words` |

## Known limitations, stated so they are decisions

- The page cannot load a library: on the web a plugin is an error row and its systems are missing, and `argus build --target web` refuses a game with plugins. Linking a plugin into the wasm build statically is item 10's question.
- A plugin's own statics are not part of a snapshot and are lost at a reload. Safe Rust cannot keep a reference to the world past a run, but it can keep a counter in a static; that is state in code (§7, P4), and the engine cannot see it.
- Forks keep the library they were made with; `script.reload` swaps the active world's. The old library stays loaded while a fork runs it.
- Mods cannot carry plugins: a mod is data and Luau.
- A plugin is trusted code: it is the project's own source, built by its own cargo, and runs with the engine's rights. Unsafe code in a plugin can break anything.
