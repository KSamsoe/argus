# Working on the engine

For changing Argus itself, from a checkout of this repository: building it, running its daemon from source, and where everything is. Making games needs none of this; see the [manual](manual/README.md).

## Building

Rust stable (1.77 or newer for `offset_of!`) plus a C++ compiler, because the Luau VM is built from source by the `mlua` crate. On this machine the Rust toolchain is the GNU target and the compiler is WinLibs MinGW-w64 GCC (installed with `winget install BrechtSanders.WinLibs.POSIX.UCRT`); its `mingw64/bin` must be on `PATH` when building. The built binary does not need it: `.cargo/config.toml` links the C and C++ runtimes in, so `argus.exe` starts from any shell.

```bash
cargo test -p argus-ecs
```

Release-only performance gates from section 17 of the design:

```bash
cargo test -p argus-ecs --release --test perf -- --ignored --nocapture
```

Measured on the development laptop (100k entities, release build): sim pass over 100k in 0.13 ms, filtered query in 0.02 ms, snapshot in 1.1 ms, hierarchy of 2,000 children in 0.5 ms. Scheduler, with allocation counting on: a 50k-entity swarm with three systems runs at about 48,000 ticks per second headless; a 500-monster discrete turn resolves in 0.05 ms.

```bash
cargo run -p argus-ecs --release --example perf
```

The same gates for the scheduler:

```bash
cargo test -p argus-sim --release --test perf -- --ignored --nocapture
```

And for authored content (10,000 entities across 200 rooms open and resolve in about 240 ms with no index; one room streams in and out in under a millisecond):

```bash
cargo test -p argus-scene --release --test perf -- --ignored --nocapture
```

## Running the daemon

```bash
cargo run -p argus-daemon --bin argusd -- --project path/to/game
```

That opens the project's default scene and listens on the project's local socket, recording the socket name in `.argus/daemon.json`. Usually there is no need: the first `argus` command in a project starts the daemon itself.

```bash
cargo run -q -p argus-cli --bin argus -- --project path/to/game world summary
```

```bash
argus world query select=[id,Health.cur] where.has=[Enemy] order=-Health.cur
```

With `--stdio` the daemon reads requests from stdin instead, one JSON-RPC object per line, which is how a client embeds it:

```bash
printf '{"jsonrpc":"2.0","id":1,"method":"world.summary"}\n' | cargo run -q -p argus-daemon --bin argusd -- --project path/to/game --stdio
```

## Layout

```
crates/
  argus-ecs/          the ECS core (this piece)
  argus-ecs-derive/   #[derive(Component)]
  argus-sim/          scheduler, clocks, RNG streams, profiling with allocation counts and captures, fault isolation (pieces 2, 8)
  argus-luau/         Luau VM host: script components, defs, systems, sandbox, reload (piece 3)
  argus-scene/        argus.toml, TOML scenes and rooms, prefab overrides, defs, moves, bulk chunks (piece 5)
  argus-daemon/       argusd: JSON-RPC 2.0 over local socket or stdio; session, world, scene, edit, sim, script, obs, prof (piece 4)
  argus-index/        .argus/index.db: SQLite cache of entities, components, refs, prefabs, defs, schemas, docs; shared query evaluator (piece 6)
  argus-input/        input.toml action maps, the Actions component and input.actions system, injected input, the replayable log (piece 7)
  argus-cli/          argus: every daemon method as a subcommand, help from the daemon's method table, daemon start/stop, argus test (piece 9)
  argus-test/         the scenario harness (tests/*.luau), state goldens, seed sweeps; test.list and test.run (piece 10)
  argus-render/       wgpu instanced sprites from atlases, Sprite and Camera, offscreen PNG and a winit window (phase 1, item 1); the semantic frame, Describe, and annotated pictures (item 12)
  argus-audio/        buses, ducking, spatial voices, variation sets, layered music, tones, limiter, cpal output (phase 1, item 3)
  argus-assets/       the importer: sidecars, placeholder shapes, validation warnings, the .argus/assets cache, the watcher (phase 1, item 4); fonts (phase 3, item 1)
  argus-tiles/        tilemaps: chunked layers, auto-tiling rules, collision from tile properties, the Tilemap component (phase 1, item 5)
  argus-physics/      overlap contacts and the kinematic character controller: slopes, one-way and moving platforms, coyote time (phase 1, item 6)
  argus-camera/       camera entities: follow with dead zone, lookahead, smoothing, and room bounds; pan and zoom; trauma shake; cuts (phase 1, item 7)
  argus-anim/         animator state machines from TOML: field-comparison transitions, frame events, crossfades, tunable timing (phase 1, item 8)
  argus-tween/        tweens: one field of an entity eased from a value to a value, with delay, repeats, yoyo, and a completion event (phase 1, item 9)
  argus-particles/    emitter pools: particle effects from TOML with curves over life, pooled outside the ECS, drawn instanced (phase 1, item 10)
  argus-material/     materials: a WGSL shader plus typed parameters, checked with line numbers, and the library (unlit, lit, palette, dissolve, outline, sdf_text) (phase 1, item 11)
  argus-os/           the host under the runtime: files and clocks, std on the desktop, an in-memory file system from a pack in the browser (phase 2, item 1)
  argus-web/          the web player: packs, a project's world without the daemon, the page's bindings, marks from the browser (phase 2, item 1)
  argus-post/         the post stack's components: screen shake, flash, fade, vignette, aberration, palette, and hit-stop (phase 2, item 6)
  argus-player/       the one runtime: a scene's world assembled for the daemon and every player, keyframes and replay files, packs, the bounded recorder (phase 2, item 7); mods and mod packs (phase 3, item 8)
  argus-game/         the release executable: a game from its packs in a window, no protocol, the report key (phase 2, item 7); the player's mods (phase 3, item 8)
  argus-ui/           the UI tree: flexbox layout on the canvas, pixel and shaped SDF text, panels and nine-slice, focus and presses, themes (phase 3, item 1)
  argus-nav/          navigation: grids from tilemaps and blocking entities, A* for individuals, flow fields for crowds, baked grids (phase 3, item 2)
  argus-jobs/         the job system: tasks posted as entities, claimed by priority and distance, walked to, worked, carried; why a worker is idle (phase 3, item 3)
  argus-dialogue/     dialogue: a def table of lines, choices, conditions, and edits, played through the UI in the world's language (phase 3, item 4)
  argus-timeline/     timelines: a def table of cues against time (tweens, animator states, camera cuts, dialogue lines, events), played by an entity and scrubbed by a seek (phase 3, item 5)
  argus-client/       the protocol from outside: the socket, JSON lines, and the CLI's key=value grammar read and written ("show as command"); no engine (phase 4, item 0)
  argus-editor/       argus-editor: the editor window, a separate process on the protocol alone, built on egui (phase 4)
tools/
  web/                build, serve, and check the web player in headless Chrome (phase 2, item 1)
examples/
  swarm/                          bullet heaven, continuous clock (phase-0 gate)
  delve/                          grid roguelike, discrete clock (phase-0 gate)
  hollow/                         platformer: three hand-made rooms and 197 generated ones streamed around the hero, music for each part (phase 3); in English and French, with a hermit to talk to and an outro; two example mods in mods/
  outpost/                        colony sim: a valley from the seed, jobs, needs, day and night with its ambience, a vignette for a colonist lost; a day is 60 s (phase 3); an inspector on Tab
docs/
  argus-engine-design-v0.3.html   the design document
  phase0-checklist.md             every phase-0 item and its status
  phase0-gate.md                  the phase-0 gate run
  phase1-checklist.md             every phase-1 item and its status
  phase1-gate.md                  the phase-1 gate run
  phase2-checklist.md             every phase-2 item and its status
  phase3-checklist.md             every phase-3 item and its status
  phase3-gate.md                  the phase-3 gate run
  phase4-checklist.md             every phase-4 item and its status
  web-spike.md                    what the web spike found (phase 2, item 1)
  playtest.md                     the playtest server on klab: the link, reports into marks, new builds (phase 2, item 7)
```

