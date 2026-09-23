# Argus Engine

A general-purpose 2D game engine whose primary user interface is a protocol. The editor, the CLI, the test runner, and the coding agent are all clients of the same daemon. The full design is in [docs/argus-engine-design-v0.3.html](docs/argus-engine-design-v0.3.html); the build plan is its section 18, and progress against phase 0 is tracked in [docs/phase0-checklist.md](docs/phase0-checklist.md).

## What exists today

Phase 0, piece 1: the ECS core, in [crates/argus-ecs](crates/argus-ecs).

- Entities are 64-bit ids (index plus generation). Authored entities carry a stable string id like `player` or `door.east`; runtime entities never do.
- Components are schemas plus columns. A schema comes from `#[derive(Component)]` on a `#[repr(C)]` Rust struct or from a runtime builder (the path the Luau binding will use). Both produce the same column layout and the same field-by-name access, so the protocol cannot tell them apart.
- Every field has a type, a default, an optional range, a doc string, and a `tune` flag. Bad writes come back as structured errors with a code, a sentence, a hint, and a `file:line` reference.
- Storage is structure-of-arrays per archetype. Tags cost nothing. Snapshots are memcpys of the columns; a snapshot at 100k entities takes about a millisecond.
- Queries fetch typed (`&T`, `&mut T`) or dynamic columns and can carry a declared read/write set. Touching an undeclared component is an error, which is what makes "who writes this" answerable later.
- A `Parent` component gives an entity a local transform. `propagate` computes world transforms in depth order and reports cycles instead of hanging.
- At frame end, entity references to dead entities are set to null and an event is emitted for each.

The crate's [SPEC-TRACE.md](crates/argus-ecs/SPEC-TRACE.md) maps each sentence of the design document that touches the ECS to the code and the test that covers it, and lists what is deliberately deferred to later pieces.

Phase 0, piece 2: the scheduler, in [crates/argus-sim](crates/argus-sim).

- Systems run in the fixed stage order `input → sim → late → hierarchy → present`, with `after` ordering inside a stage and a deterministic tie-break. `present` is read-only and a system that declares writes there is refused.
- Every system declares reads and writes; its context binds queries and structural changes to that declaration, and `who_writes` answers "what could have changed this component" in run order.
- Two clocks behind one model. A continuous clock runs `speed` ticks per render frame (0 pauses, fractional speeds accumulate). A discrete clock ticks only on `advance`. A system can bind to either, so one scene can mix them.
- Randomness comes from named PCG32 streams seeded from the run seed and the stream name, so adding a system never perturbs another's sequence. Same seed, same run, bit for bit.
- Every system is timed every tick. `prof_systems` gives mean and p95 in stage order, `prof_worst` the slowest ticks with a per-system breakdown and entity counts by archetype.
- A panic or error inside a system is caught at the boundary, the system is disabled, and the fault is an event with the declaration's `file:line`. The rest of the tick runs and the world stays inspectable.
- `play` snapshots the world and sim state, `stop` restores them. Snapshots include clock and RNG state, so replaying from one reproduces the same rolls.

Its trace is [crates/argus-sim/SPEC-TRACE.md](crates/argus-sim/SPEC-TRACE.md).

Phase 0, piece 3: the Luau binding, in [crates/argus-luau](crates/argus-luau).

- Scripts declare components (`component "HitStop" { frames = field.u8 { range = {0, 30} }, ... }`), def table schemas (`def "recipes" { ... }`), and systems (`system "sim.hitstop" { stage, after, reads, writes, run = function(q) ... end }`). The example systems from the design document run as written.
- Script systems get the same contract as Rust ones: declared reads and writes are checked at load time (names must exist) and at run time (proxies refuse undeclared writes), every field write goes through the schema's range check, and errors carry the schema or script `file:line`.
- The VM is sandboxed with a memory limit and an instruction budget per system run. Globals are read-only, so no state can hide in them. A runaway loop is stopped, the system disabled, and the fault reported as an event.
- Handles (`q`, entities, components) are valid only during the `run` that made them; using one on a later frame is a clear error.
- Reload re-evaluates changed files, swaps system bodies in place, keeps world state, tick, and RNG stream state, removes systems of deleted files, and reports syntax and declaration errors as `file:line` data.

Its trace is [crates/argus-luau/SPEC-TRACE.md](crates/argus-luau/SPEC-TRACE.md).

Phase 0, piece 5: authored content, in [crates/argus-scene](crates/argus-scene). (Piece 4, the daemon, needs this to open a scene, so it came first.)

- A project is `argus.toml` plus the directories from the design: `prefabs/`, `defs/`, `scenes/<name>/`, and so on. One authored entity per TOML file, filename is the id. The entity file from the design document loads as written.
- Prefabs are files too (`prefabs/door/locked.toml`, itself built from `door/base`). An entity file holds only its overrides. Inheritance is per field, removal is an explicit `-Door = true`, children live under `[children.light.Light]` and are addressed as `@door.east/light`. Resolving an entity gives every field with where it came from: its own file, a named prefab, or the schema default.
- Scenes are flat or subdivided into rooms. The scene file holds the room graph; rooms within a radius of the player stay loaded, the rest stream out, and references across the boundary go null and come back when the room does. Transitions are events.
- Data tables in `defs/` are rows keyed by id, validated against schemas declared in script, and referenced as `item:key.brass`. Every reference in every file is checked when a scene opens.
- Renames go through `asset.move`, which rewrites every reference and appends to `.argus/moves.log`. A rename done outside the tool produces an error naming the old path, and the hint tells you where it went if the log knows.
- Large data (a tilemap) is a small text index plus a binary chunk with a hash. Readers query rectangles; nobody reads a 40,000-line array.
- Edits write back only the changed key and leave comments and spacing alone. A scene with any problem lists all of them with `file:line` and refuses to load, so a run always starts from a whole world.

Its trace is [crates/argus-scene/SPEC-TRACE.md](crates/argus-scene/SPEC-TRACE.md).

Phase 0, piece 4: the daemon, in [crates/argus-daemon](crates/argus-daemon), with the `argusd` binary.

- One process per project, one world. Clients speak JSON-RPC 2.0, one object per line, over a local socket (a named pipe on Windows) or stdio. Server-initiated notifications carry events and commits to subscribers.
- Forty-eight methods in the namespaces the plan lists for phase 0: `session`, `world`, `scene`, `edit`, `sim`, `script`, plus `obs.events` and `prof.*`. They are described once, in a table the daemon serves as `session.capabilities` and `session.docs`, and the CLI will generate its help from.
- Every read obeys the same contract: a default limit of 50, a total that is always present, a cursor for the rest, and rows as compact arrays. `world.summary` is the one-screen orientation call and fits in about 2 KB.
- Edits are transactions with an author and a note. They apply at commit, all or none. Authored entities are written to their files; runtime entities are live-only. Every commit records its inverse, so undo and redo work per author, and the change feed says who changed what and why. A lock fences entities or paths, and a write into one fails naming the owner and their note.
- Play snapshots the world and stops restores it. The continuous clock ticks in real time on the daemon thread; `sim.run_until` fast-forwards headless until an event, a time, or a tick count.
- Every error reaches the client with its code, sentence, hint, and `file:line` unchanged.

Its trace is [crates/argus-daemon/SPEC-TRACE.md](crates/argus-daemon/SPEC-TRACE.md).

Phase 0, piece 6: the index, in [crates/argus-index](crates/argus-index).

- `.argus/index.db` is a SQLite cache of everything in the project's files: entities by scene, components and fields by entity, references between entities, prefabs, defs, and files, the prefab and def catalogs, component schemas, systems, and a full-text search over doc strings. It is never committed, and a schema change rebuilds it.
- The daemon keeps it current: on open, after every commit, reload, and move, and by a scan every ten seconds while idle so a file edited by hand is picked up.
- Reads about a scene that is not loaded come from the index with exactly the shapes the live world gives. That is guaranteed by construction: one query evaluator runs over either source, and one set of shape builders produces `world.get` and `world.summary` for both. A `source` field says which answered.
- New reads it makes possible: everything that references an entity, prefab, def, or file; every instance of a prefab across all scenes; and searching doc strings in plain words.

Its trace is [crates/argus-index/SPEC-TRACE.md](crates/argus-index/SPEC-TRACE.md).

Phase 0, piece 7: action maps and the input log, in [crates/argus-input](crates/argus-input).

- `input.toml` declares actions (`move`, `fire`, `crouch`) as buttons, axes, or two-axis sticks, with bindings per device and player slots with their devices. Bindings are checked and reported now and read when devices arrive in phase 1.
- The map becomes one component, `Actions`, with a field per action and went-down and went-up edge fields for buttons. Every tick, the `input.actions` system writes each slot's values onto every entity that carries a `PlayerSlot`. Gameplay reads a component like any other and never sees a key.
- With no devices, input is injected: hold an action for some frames, tap it, hold it until told otherwise, or schedule a script of injects. Toggle actions flip on each press.
- Every value change is logged by tick, or by turn in a discrete scene. `input.record` writes that log next to a small file naming the scene, seed, and clock. `input.replay` feeds a log back in, and the same scene, seed, and code give the same run, which the tests check bit for bit.

Its trace is [crates/argus-input/SPEC-TRACE.md](crates/argus-input/SPEC-TRACE.md).

Phase 0, piece 8: the rest of profiling, in [crates/argus-sim](crates/argus-sim) and the daemon.

- `prof.alloc` counts heap allocations and bytes per system per tick. A small wrapper around the system allocator keeps thread-local counters; the scheduler reads them before and after every system. `argusd` installs the wrapper, and the report says so, or says it is not counted in a build without it. The Luau VM allocates through the same allocator, so a script system that builds a table per entity is visible as a number with its name on it.
- `prof.frame` and `prof.worst` carry the same counts per system, so a slow tick shows whether it was slow because it allocated.
- `prof.capture {ticks: 300}` records the next 300 ticks with every system span on one time base and writes a trace file under `.argus/captures/`. The format is the Chrome trace event JSON that Tracy imports with its bundled tool and that Perfetto opens as is. A `prof.capture` event names the file when it is written, so a client can wait for it like any other event.
- The cost is measured on the scheduler's perf gate: the 50k-entity swarm runs at about 48,000 headless ticks per second with counting on, 51,700 without.

Phase 0, piece 9: the command line, in [crates/argus-cli](crates/argus-cli), binary `argus`.

- Every daemon method is a subcommand and nothing else is: `argus world query`, `argus world.query`, and `argus input log get` all reach the method of that name. There is no command table in the crate; the daemon's method table is the list, and the test checks it both ways.
- Help at every level is generated from that same table, so `argus world get --help` prints the doc, params, and returns that `session.docs` serves, and the two cannot drift.
- Parameters are `key=value` words: JSON when they parse as JSON, strings otherwise, with `[Enemy, @player]` and `{of: @player, r: 6}` accepted as written, dotted keys for nesting, and one bare word for the method's first parameter, so `argus world get @player` and `argus sim step 5` read as they should. `--params` takes a whole JSON object.
- Row results print as aligned tables, everything else as pretty JSON, and `--json` gives the raw result for a script or an agent. An error prints its code, sentence, hint, and `file:line` and exits 1.
- The project is the nearest `argus.toml`, and its daemon is found through `.argus/daemon.json`. A call with no daemon answering starts one in the background and waits for it; `argus daemon stop` asks it to exit through the protocol like everything else.

Its trace is [crates/argus-cli/SPEC-TRACE.md](crates/argus-cli/SPEC-TRACE.md).

Phase 0, piece 10: the scenario test runner, in [crates/argus-test](crates/argus-test), reached as `argus test`.

- A test is a scenario in `tests/*.luau`: `scenario "first wave clears" { scene = "arena", seed = 7, run = function(t) ... end }`. Inside, `t.input:hold` and `t.input:tap` inject actions, `t:run_until` runs to an event, a sim time, or a frame count, `t:advance` takes turns, and `t.expect` asserts on counts, fields, events, sums, series trends, stage timings, and reachability.
- Everything `t` does is a protocol call, so the runner is a client of the daemon like the editor and the CLI. A run loads the scene fresh for every seed, and the scene that was open comes back afterwards.
- State goldens: `t.expect:snapshot("wave1_end", { select, where })` stores the selected rows as JSON under `tests/goldens/` on the first run and compares on every later one. A mismatch is a field-level diff naming the entity, the field, the golden value, and the new one. `--update` rewrites.
- Seed sweeps: `seeds = range(1, 200)` in the scenario, or `argus test --seeds 1..50` over any of them, shake out order-dependent bugs. Each seed is its own run in the report.
- Perf assertions read the profiler and compare a percentile against the bound with a margin, so they can gate without flaking. A short scenario takes about ten milliseconds.
- The report is one line per scenario, one per failing seed and expectation, and a summary; `--json` gives the whole thing. A scenario that throws is one failed seed with its `file:line`, never a crash.

Its trace is [crates/argus-test/SPEC-TRACE.md](crates/argus-test/SPEC-TRACE.md).

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
  argus-assets/       the importer: sidecars, placeholder shapes, validation warnings, the .argus/assets cache, the watcher (phase 1, item 4)
  argus-tiles/        tilemaps: chunked layers, auto-tiling rules, collision from tile properties, the Tilemap component (phase 1, item 5)
  argus-physics/      overlap contacts and the kinematic character controller: slopes, one-way and moving platforms, coyote time (phase 1, item 6)
  argus-camera/       camera entities: follow with dead zone, lookahead, smoothing, and room bounds; pan and zoom; trauma shake; cuts (phase 1, item 7)
  argus-anim/         animator state machines from TOML: field-comparison transitions, frame events, crossfades, tunable timing (phase 1, item 8)
  argus-tween/        tweens: one field of an entity eased from a value to a value, with delay, repeats, yoyo, and a completion event (phase 1, item 9)
  argus-particles/    emitter pools: particle effects from TOML with curves over life, pooled outside the ECS, drawn instanced (phase 1, item 10)
  argus-material/     materials: a WGSL shader plus typed parameters, checked with line numbers, and the library (unlit, lit, palette, dissolve, outline) (phase 1, item 11)
examples/
  swarm/                          bullet heaven, continuous clock (phase-0 gate)
  delve/                          grid roguelike, discrete clock (phase-0 gate)
  hollow/                         three-room platformer slice (phase-1 gate)
docs/
  argus-engine-design-v0.3.html   the design document
  phase0-checklist.md             every phase-0 item and its status
  phase0-gate.md                  the phase-0 gate run
  phase1-checklist.md             every phase-1 item and its status
  phase1-gate.md                  the phase-1 gate run
```

## Phase 1

Phase 1's items and their constraints are in [docs/phase1-checklist.md](docs/phase1-checklist.md). Done so far:

- **Item 0, the Luau cost model and the broadphase.** `vec2` fields cross into Luau as native vectors, so a position read or write no longer allocates a table. `q:spawn_many` places a batch of entities in their archetype in one call. The engine now keeps the shared spatial hash the design's physics section asks for, rebuilt once per tick; `world.query`'s `within` predicate, Luau's `q:within` and `q:nearest`, and the phase-1 physics tiers all read it. On the 50,000-enemy stress scene the tick went from 280 ms to 49 ms and the hit pass from 176 ms to 0.14 ms; the rest is the binding's per-call cost in per-entity loops, which is what promoting a system to Rust removes.
- **Item 1, the renderer**, in [crates/argus-render](crates/argus-render). A `Sprite` is an atlas frame or a described shape (a rect or a circle, a size, a colour), so gameplay never waits on art; a `Camera` entity frames the view. One instanced pipeline draws a run of sprites per atlas and culls on the GPU; positions are interpolated between the last two sim ticks. It renders offscreen to a PNG for tests, CI, and `argus obs frame`, or into a window that the daemon presents every frame: `argus daemon run --window`. Both example games draw with it. Trace: [crates/argus-render/SPEC-TRACE.md](crates/argus-render/SPEC-TRACE.md).
- **Item 2, real input devices.** The bindings in `input.toml` are read now: keys and mouse buttons from the window, gamepads through gilrs, each slot fed by the devices it lists, so two gamepads are two players. A changed value goes through the same schedule as an injected one, so the log still records actions, never keys, and a run played by hand replays with no hands. On Delve a key press takes a turn. `argus daemon start --window --play` opens a game ready to play.
- **Item 3, the audio mixer**, in [crates/argus-audio](crates/argus-audio). `audio.toml` declares buses in a routing tree, ducking rules, sounds with variation sets, and music tracks made of layers. A script asks for a sound by name (`q:play("sfx/step", {entity = e})`); the mixer picks the variation from the run seed, pans and attenuates it by distance from the listener, and answers in the event log with `sound.played`, so the agent reads what the human hears. A sound can be a described tone, so both example games make noise with no audio files. `audio.status` reports the meters; `audio.clip` writes the last seconds as a WAV. Reading those meters on Swarm's first wave showed the mix clipping when thirty enemies die in one tick, and a per-sound voice cap plus a master limiter came out of it. Trace: [crates/argus-audio/SPEC-TRACE.md](crates/argus-audio/SPEC-TRACE.md).
- **Item 4, the asset importer**, in [crates/argus-assets](crates/argus-assets). An asset is its path under `assets/` without the extension, with a TOML sidecar beside it an agent can write: a sprite sheet's frames, pivots, and clips, or a sound's bus, gain, and variations. A sidecar with no image describes a shape (`kind = "rect"`, `size = [8, 8]`, `color = "#E8703F"`), which the importer draws into its cache, so the PNG a human drops in later is the same asset and no reference changes. Every import is checked and reported as events with warnings ("frame 5 is empty", "texture 30x20 is not power-of-two, padding to 32x32"); the tree is watched and changes reload in the renderer and the mixer while the game runs. Trace: [crates/argus-assets/SPEC-TRACE.md](crates/argus-assets/SPEC-TRACE.md).
- **Item 5, tilemaps**, in [crates/argus-tiles](crates/argus-tiles). A map is a small text index (`room.tilemap.toml`: size, layers, tilesets, a hash) and a folder of binary chunks that load only when read and are written only when changed, so a 4,096 by 4,096 map with 8 layers edits and saves in a few milliseconds. A tileset is a sprite asset, and its sidecar declares auto-tiling rules for each terrain and which tiles are solid or one-way. An agent carves a room with one `edit.tiles` call naming a rect and a terrain, inside a transaction the human can see in the feed and the agent can undo; `scene.tiles.collision` shows the result as rows of `.`, `#`, and `-`. An entity with a `Tilemap` component places the map in the world, and the renderer draws the tiles the camera sees. Trace: [crates/argus-tiles/SPEC-TRACE.md](crates/argus-tiles/SPEC-TRACE.md).
- **Item 6, the kinematic character controller**, in [crates/argus-physics](crates/argus-physics). An entity with a `Collider` reports contacts as `overlap.begin` and `overlap.end`. Add a `KinematicBody` and it moves against the tilemaps and the solid bodies without passing through walls, walks up and down slopes, jumps up through one-way platforms and lands on them, rides moving platforms, and steps up small ledges. Add a `CharacterController` and the entity's actions become a platformer's run and jump; coyote time, jump buffering, corner correction, jump height, and every other feel setting is a tune field a human can adjust while playing. Landings, walls, bonks, and corner corrections are events in the log, and a script reads the tick's events with `q:events`. Trace: [crates/argus-physics/SPEC-TRACE.md](crates/argus-physics/SPEC-TRACE.md).
- **Item 7, the camera**, in [crates/argus-camera](crates/argus-camera). A camera is an entity with a `Camera`. Give it a `target` and it follows: the target moves freely inside a dead zone, the camera looks ahead in the direction the target is moving, catches up smoothly, and keeps its view inside the bounds of the room the target is in, sliding over when the target walks into the next room. Give it pan and zoom actions instead and a player moves it, the free camera a colony game wants. A `CameraShake` beside it shakes on demand: any system emits `camera.shake {amount}` and the trauma rises, then decays through a curve. Every one of those settings is a tune field. The drawing camera is the active one with the highest priority, and switching is a `camera.cut` event. Each camera records the rect it shows, and `obs.frame` says which camera drew. Trace: [crates/argus-camera/SPEC-TRACE.md](crates/argus-camera/SPEC-TRACE.md).
- **Item 8, animation state machines**, in [crates/argus-anim](crates/argus-anim). An animator is a TOML file beside the sprites: states that play the sprite's clips, and transitions written as field comparisons, such as `KinematicBody.grounded and abs(KinematicBody.velocity.x) > 0.5`, or `end` when a one-shot finishes. A transition can be allowed only on certain frames (a cancel window) and can crossfade. A state can fire events on given frames: the frame the foot lands fires `footstep` and asks the mixer for `sfx/step`, and the frame the sword is out fires `hitbox.on` for a script to act on. A state's frame rate, how long its wind-up lasts, and how long it holds can be read from the game's own tune fields, so a human can tune a wind-up while playing. An `Animator` component on an entity runs the machine, and 2,000 of them take under 0.2 ms a tick. Trace: [crates/argus-anim/SPEC-TRACE.md](crates/argus-anim/SPEC-TRACE.md).
- **Item 9, tweens**, in [crates/argus-tween](crates/argus-tween). A tween moves one field of an entity from a value, or from where it is, to a value over a duration, through an easing curve such as `out_back` or `in_out_sine`, and emits an event when it finishes. A script squashes the player on landing with one call, `q:tween(e, "Transform.scale", {from = vector.create(1.4, 0.6, 0), to = vector.create(1, 1, 0), duration = 0.2, easing = "out_back"})`, and nudges the camera the same way through `Camera.offset.y`. Each tween is an entity, so what is animating right now is a query, and a replay plays it back exactly. `world.who_writes` names the tween system for whatever the tweens have touched. Trace: [crates/argus-tween/SPEC-TRACE.md](crates/argus-tween/SPEC-TRACE.md).
- **Item 10, emitter pools**, in [crates/argus-particles](crates/argus-particles). A particle effect is a TOML file: where particles start, how many a second or at once, how long they live, how fast and which way they go, gravity and drag, and curves over their life for speed, size, rotation, and colour. An `Emitter` on an entity plays one. A script throws sparks at a hit with one event, `q:emit("particles.burst", {emitter = q:entity("@sparks"), count = 12, pos = where})`, which is what Swarm now does at every kill. Particles are not entities, so a quarter of a million of them cost the ECS one entity per emitter and take 0.43 ms a tick. Trace: [crates/argus-particles/SPEC-TRACE.md](crates/argus-particles/SPEC-TRACE.md).
- **Item 11, materials with hot reload**, in [crates/argus-material](crates/argus-material). A material is a WGSL shader, `fn material(in: MaterialInput, p: Params) -> vec4<f32>`, with a TOML sidecar that declares its parameters and their types. A sprite names one with `Sprite.material`. A parameter can come from a field of each sprite's entity, so `q:tween(e, "Dissolve.amount", {to = 1, duration = 0.5})` dissolves one enemy and not the rest. The engine ships `unlit`, `lit`, `palette`, `dissolve`, and `outline` in the same format, and a sidecar can change one's defaults. A shader that does not compile is reported with the line and column in its own file, through `script.errors`, while the last good version keeps drawing. An edit reloads in about 20 ms. Trace: [crates/argus-material/SPEC-TRACE.md](crates/argus-material/SPEC-TRACE.md).
- **Item 12, semantic frames and annotated screenshots**, in [crates/argus-render](crates/argus-render). `obs.frame {format: "semantic"}` says what the player sees without a picture, in about a kilobyte: the camera's view, the HUD, each notable thing on screen with its bounds in pixels and what it says about itself, the rest counted by kind, the effects, and a character map of the ground. A game adds a `Describe` to say what matters, like `text = "HP {Health.cur}/{Health.max}"`. The same frame is a screen-reader feed, and `argus obs frame format=semantic` prints it as sentences. With `annotate: true`, the PNG has every box and id drawn on it, so what the agent sees can be matched to what it can query. Trace: [crates/argus-render/SPEC-TRACE.md](crates/argus-render/SPEC-TRACE.md).
- **Item 13, obs.diff, obs.trace, obs.why**, in [crates/argus-daemon](crates/argus-daemon). Three questions the agent asks after something happens. `obs.diff` answers "what changed since I last looked": which things appeared, vanished, or changed which fields, in under a kilobyte. `obs.trace` answers "what happened to this one": an entity's fields over the last minute of frames, when each changed and to what, next to its events. `obs.why` answers "what could have caused this": from an event back through the systems that emitted it and fed it, with the file and line of each. Every event now says which system emitted it. Trace: [crates/argus-daemon/SPEC-TRACE.md](crates/argus-daemon/SPEC-TRACE.md).

## Where phase 1 stands

Items 0 to 13 are all built, each with its trace. The gate is half done, recorded in [docs/phase1-gate.md](docs/phase1-gate.md). The agent built a three-room slice of Hollow in [examples/hollow](examples/hollow) through the CLI: tiles, a kinematic hero, one-way planks, a following camera, and a run, jump, and land animation whose footsteps make a sound. Comparing its pictures with its semantic frames showed the hall's pit drawn as more floor, because nothing was painted behind it, and one tile transaction fixed it. Building it found five faults, all fixed. The other half is a person playing Swarm and Delve in a window, which has not happened yet.

## Where phase 0 stands

All ten items of the plan's phase-0 sentence are built, each with a trace from the design's sentences to code and tests, and the gate in section 18 is passed: [docs/phase0-gate.md](docs/phase0-gate.md) records an agent building headless cores of Swarm and Delve from a paragraph brief each, using only the CLI, with passing tests and "why did the player die" answered from the event log in both. The two games are in [examples/swarm](examples/swarm) and [examples/delve](examples/delve).

The finding the gate was designed to produce: nothing in either game fought the protocol, but Luau at 50,000 live entities costs about 265 ms a tick, and `prof.alloc` shows it is table allocation per field access in the binding. The stress scene runs and its test passes; the two hot systems are the first candidates for promotion to Rust in phase 1, which the declared write sets make a local change.

```bash
cargo run -q -p argus-cli --bin argus -- --project path/to/game test --seeds 1..20
```
