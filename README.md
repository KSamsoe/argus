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

## Building

Rust stable (1.77 or newer for `offset_of!`) plus a C++ compiler, because the Luau VM is built from source by the `mlua` crate. On this machine the Rust toolchain is the GNU target and the compiler is WinLibs MinGW-w64 GCC (installed with `winget install BrechtSanders.WinLibs.POSIX.UCRT`); its `mingw64/bin` must be on `PATH` when building.

```bash
cargo test -p argus-ecs
```

Release-only performance gates from section 17 of the design:

```bash
cargo test -p argus-ecs --release --test perf -- --ignored --nocapture
```

Measured on the development laptop (100k entities, release build): sim pass over 100k in 0.13 ms, filtered query in 0.02 ms, snapshot in 1.1 ms, hierarchy of 2,000 children in 0.5 ms. Scheduler: a 50k-entity swarm with three systems runs at about 45,000 ticks per second headless; a 500-monster discrete turn resolves in 0.02 ms.

```bash
cargo run -p argus-ecs --release --example perf
```

The same gates for the scheduler:

```bash
cargo test -p argus-sim --release --test perf -- --ignored --nocapture
```

## Layout

```
crates/
  argus-ecs/          the ECS core (this piece)
  argus-ecs-derive/   #[derive(Component)]
  argus-sim/          scheduler, clocks, RNG streams, profiling, fault isolation (piece 2)
  argus-luau/         Luau VM host: script components, defs, systems, sandbox, reload (piece 3)
docs/
  argus-engine-design-v0.3.html   the design document
  phase0-checklist.md             every phase-0 item and its status
```

## Next pieces of phase 0

JSON-RPC daemon, TOML scenes and prefab overrides, SQLite index, action maps, the rest of profiling (allocation counts and Tracy capture), CLI, scenario test runner. See the checklist for the exact spec each one has to satisfy.
