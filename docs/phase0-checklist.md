# Phase 0 checklist

Section 18 of the design document lists phase 0 as one sentence. This file breaks that sentence into pieces, attaches every constraint from elsewhere in the document that each piece has to meet, and records status. Update it as pieces land so nothing gets dropped between pieces.

Phase 0 rule from the plan: no renderer, no audio, no input devices. Actions are injected only.

## 1. ECS core with SoA columns, archetypes, and hierarchy

**Status: done** in `crates/argus-ecs`. Sentence-level trace in `crates/argus-ecs/SPEC-TRACE.md`.

## 2. Scheduler with stages, declared write sets, and both clock drivers

**Status: done** in `crates/argus-sim`. Sentence-level trace in `crates/argus-sim/SPEC-TRACE.md`. Every constraint below is covered there; the two that are only partly this crate's are the §15 lint (needs the Luau binding to inspect script source) and the tactics-style clock hand-off (§19, decided by a prototype after phase 0). Constraints that were met:

- Stages in fixed order `input → sim → late → hierarchy → present` (§3). `sim` is the only stage that may mutate gameplay state; `present` runs at render rate and is read-only.
- Each system declares `stage`, `after`, `reads`, `writes` (§3 example). Use `argus_ecs::Access`; violations are already errors.
- `hierarchy` stage runs `argus_ecs::propagate` before `present` (§3).
- Two clock drivers behind one abstraction (§6): continuous (fixed rate, 60 Hz default; render decoupled and interpolated) and discrete (advances only on `sim.advance {by: "turn"}` or `clock.advance()`; `present` still runs). Every scene declares which drives `sim`.
- A system group can bind to either clock; a game can mix them (§6). The open question on tactics-style hand-off is decided after phase 0 with a prototype (§19).
- Speed is a property of the continuous clock: `sim.speed {x: 3}` runs three ticks per render frame; `{x: 0}` is pause; headless fast-forward at whatever rate the CPU allows (§6).
- Wall-clock time is never visible to gameplay code; `sim.time` is tick count times timestep on the driving clock (§6). Nothing in `sim` may read a device, the wall clock, or a local-player singleton (§15); this is a lint.
- Randomness from named streams: PCG32 seeded from `hash(run_seed, stream_name)` so adding or reordering a system does not perturb other systems' sequences (§6). Stream states are part of the snapshot.
- The scheduler times every system every tick, cheaply, always on: the data behind `prof.*` (§5).
- A panic in a Rust system is caught at the system boundary, the system is disabled, and the fault is reported as an event (§2).
- "Editing" is the world with sim stages disabled; "playing" is with them enabled; entering play snapshots, stopping restores (§2).
- End of frame calls `World::end_frame` so dangling refs are nulled (§3).
- §17 numbers that land here: Swarm sim tick p95 < 6 ms; Delve turn resolution with 500 monsters < 2 ms; Outpost headless fast-forward > 2,000 ticks/s.

## 3. Luau binding with component and def schemas

**Status: not started.** Constraints:

- `component "Name" { doc = ..., field = field.u8 { default, range, tune, doc } }` schema files in `components/`, one per file (§3, §4). Build with `argus_ecs::SchemaBuilder`; `source` is `file:line`.
- `system "sim.name" { stage, after, reads, writes, run = function(q) ... end }` in `systems/`, name = path (§3, §4). `q:each("A", "B")` iterates; use `DynColumn` and `Access::from_names`.
- Def schemas for `defs/` tables (§4).
- Sandboxed, instruction-budgeted (§2). Script globals frozen after module load (§7). A system may only touch declared components and may not hold references across frames; detected statically at load where possible, at run time otherwise (§7).
- `script.reload` recompiles changed files, rebinds, continues from the current frame; `script.errors` returns file:line and the compiler message, never a crash (§7). Hot reload target < 100 ms (§17).
- Component `migrate` functions per schema version are phase 3 (§6), but the schema loader should accept the field now.
- Open question (§19): whether the Luau heap is in the snapshot or per-system state is forbidden. Leaning stricter.

## 4. JSON-RPC daemon: session, world, edit, sim, script

**Status: not started.** Constraints:

- JSON-RPC 2.0 over a local socket (Unix domain socket or Windows named pipe) or stdio when embedded; server-initiated notifications for subscriptions (§2, §5). Hot paths use out-of-band shared memory that the JSON message points at (§5).
- One daemon per project, one world, forks inside the same process (§2). `argusd --headless` (§2).
- Namespaces and methods in phase 0: `session` (`open`, `capabilities`, `docs`), `world` (`summary`, `query`, `get`, `describe`, `who_writes`, `defs`), `edit` (`begin`, `set`, `spawn`, `despawn`, `commit`, `abort`, `lock`, plus `undo`/`redo`), `sim` (`play`, `pause`, `step`, `advance`, `speed`, `run_until`), `script` (`reload`, `systems`, `errors`) (§5, §18). `sim.seek/snapshot/fork/save/load` are phase 2 and 3.
- Read contract (§5): default `limit` 50, `total` always present, cursor for continuation, compact row arrays, any default response about 2 KB. `world.summary` is the one-screen orientation call.
- `world.query` shape: `select` field paths, `where` with `has` and `within {of, r}`, `order`, `limit` (§5). Spatial predicates hit the broadphase in phase 1; phase 0 may scan.
- `world.get {entity, resolved: false|true}` returns overrides or the merged result with per-field provenance (§3).
- Edits are transactions with `author` and `note`; authored-entity edits write files on commit; runtime-entity edits are live-only; every commit records its inverse; undo/redo scoped per author; the change feed is subscribable and answers "who changed this and why" (§5). `edit.lock` fences entities or paths and a write into a locked path fails naming the lock and owner (§5, §10).
- Errors carry code, message, hint, ref (§5). Use `argus_ecs::ArgusError` unchanged; code block 4xxx is reserved for it.
- Access: local socket needs no auth; remote access via `argus token` with scope and expiry over TLS is later (§5). Nothing in the player runtime listens on a socket (§15).
- Identical result shapes whether served from the live world or the index (§4).

## 5. TOML scenes, rooms, defs, and prefab overrides

**Status: not started.** Constraints:

- Project layout from §4: `argus.toml`, `components/`, `systems/`, `defs/`, `prefabs/`, `scenes/<name>/scene.toml` plus one authored entity per TOML file with filename = string id, `rooms/<room>/room.toml` for subdivided scenes, `assets/`, `tests/`, `plugins/`, `.argus/` gitignored.
- Entity file shape: `prefab = "door/locked"`, `tags = [...]`, `[Component]` tables, entity refs as `@id` (room-qualified `@cavern-04/door.west`), def refs as `item:key.brass` (§4).
- Runtime entities are never written (`World::is_authored`).
- Rooms are directories; the scene file holds the room graph; streaming by graph distance with the index answering queries about unloaded rooms; transitions are an engine event with a game hook (§4). Full streaming at scale is phase 3.
- Data tables in `defs/` with declared schemas, referenced as `table:id`, queryable via `world.defs {table, where}` (§4).
- Path references, not GUIDs. `asset.move` rewrites references and appends to `.argus/moves.log`; an outside rename leaves a clear "missing reference" error with the old path (§4).
- Bulk data: text index file plus binary chunk; `scene.tiles.get {rect}` (§4). Tilemaps proper are phase 1.
- Prefab rules (§3): per-field inheritance, never per component; adding a component is an override; removal is an explicit `-Component` entry; child entities addressed by path `@door.east/light`; `resolved: true` returns provenance (`own`, `door/locked`, `door/base`); a prefab change is a transaction and the feed reports how many instances it touched.
- Open question: TOML versus KDL; leaning TOML (§4, §19).
- §17: cold scene open with a warm index < 300 ms; 10,000 authored entities across 200 rooms.

## 6. SQLite index

**Status: not started.** Constraints (§4): `.argus/index.db` updated on load and on every file change; entities by scene, components by entity, references between assets, prefabs, and entities, full-text over doc strings; serves `world.*` and `scene.*` queries when the world is not loaded with identical result shapes; cache only, never committed. Delve target: floor generation plus index < 20 ms (§17).

## 7. Action maps with the action log

**Status: not started.** Constraints (§12, §6): `input.toml` declares actions with bindings per device and per player slot; gameplay never sees keys; the input log records actions by tick (continuous) or commands by turn (discrete); `input.inject {action}`, `input.script`, `input.record`, `input.log.get`, `input.bindings` (§5). Phase 0 has no devices: injection only. Inputs arrive only as per-player-slot actions (§15).

## 8. prof.*

**Status: partial.** Data side done: `argus_ecs::World::archetype_stats` supplies `prof.archetypes`; `argus_sim::Scheduler::{prof_systems, prof_worst, prof_last, prof_tick_p95_ms}` supply `prof.systems {last}` (mean and p95 in stage order), `prof.worst {n}` (per-system breakdown plus entity counts by archetype at that tick), and `prof.frame`. Remaining (§5): `prof.alloc` by system (needs a counting global allocator wired to the scheduler's current-system marker) and `prof.capture {ticks}` (Tracy-compatible trace). Exposing all of it as protocol methods is item 4.

## 9. CLI

**Status: not started.** `argus` subcommands mirror protocol methods one-to-one (§5, §16); help generated from the same schema as the protocol and the later MCP tool descriptions (§16). `argus docs --for-agent` (ARGUS.md from the live daemon) and `argus mcp` are phase 2.

## 10. Scenario test runner with state goldens and seed sweeps

**Status: not started.** Constraints (§9):

- `tests/*.luau`: `scenario "name" { scene, seed | seeds = range(a, b), run = function(t) ... end }`.
- `t.input:hold(action, value, frames)`, `t.input:tap(action)`, `t:run_until { event | sim_time, max_frames, speed = "max" }`, `t:advance { by = "turn", n }`.
- Expectations: `count`, `field`, `events(...):count()`, `snapshot(name, {select, where})` state goldens with field-level diffs, `perf(stage):p95_ms()`, `path(...):exists()`, `sum(field)`, `series(field, every):trend()`.
- Headless, tens of milliseconds for a short scenario, never flakes (§6).
- `argus test`: run one by name, `--seeds 1..50`, compact text report, `--json`. Replay dump and clip export on failure are phase 2. Perf assertions on percentiles with a generous margin.
- Image goldens are opt-in and rare (phase 1 renderer).

## Phase 0 gate

From an empty directory and a paragraph brief each, an agent using only the CLI builds headless cores of Swarm (continuous clock, 50k entities, waves) and Delve (discrete clock, generated floors, turn order) with passing tests, and can answer "why did the player die" in both from events and traces. If either game fights the protocol, that finding is the phase's output.

**Inconsistency to resolve:** the gate asks for answers "from events and traces", but `obs.trace` is listed under phase 1. Either `obs.trace` (one entity's component history over a window, §8) moves into phase 0, or the gate is satisfied with events plus queries. Decide before piece 4 fixes the phase-0 method set.

## Deliberately outside phase 0 (so nobody adds them early)

Renderer, audio, input devices, tilemaps, physics, camera, animation, tweens, particles, materials, semantic frames and screenshots, `obs.diff/trace/why` (phase 1); snapshot ring, `sim.seek/fork`, marks, tuning log, proposals, locks UI, `argus mcp`, `argus docs`, builds (phase 2); UI tree, dialogue, timelines, pathfinding, jobs, save migrations, streaming at scale, audio buses, mods (phase 3); editor (phase 4); plugins with dylib reload, parallel scheduling, lighting, rigid bodies, skeletal, platform plugin, release recorder, accessibility, importers, browser editor, cross-platform determinism (phase 5).
