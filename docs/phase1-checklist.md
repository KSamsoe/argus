# Phase 1 checklist

Section 18 of the design document lists phase 1 as one sentence and a gate. This file breaks the sentence into pieces, attaches every constraint from elsewhere in the document that each piece has to meet, and records status, the way `phase0-checklist.md` did for phase 0. Piece 0 is not in the sentence: it is the phase-0 gate's output, and everything in phase 1 that runs gameplay in Luau sits on it.

Phase 1 sentence: *wgpu sprite renderer with instancing and atlases; windowed and offscreen modes; keyboard, mouse, gamepad input with recording; audio mixer with event log; asset importer with sidecars and watch; tilemaps with chunks and auto-tiling; kinematic character controller; camera component; sprite animation state machines with frame events; tweens; emitter pools; materials with hot reload; semantic frames and annotated screenshots; obs.diff, obs.trace, obs.why.*

Gate: *Swarm and Delve are playable by a human in a window. The agent builds a three-room slice of Hollow with tiles, a kinematic player, one-way platforms, and a following camera, screenshots it, describes what's on screen from the semantic frame, and fixes a visual bug it found that way. The player has a run, jump, and land animation with a footstep event that fires a sound.*

Carried in from phase 0 by decision: `obs.trace` (item 13 here), `code` in `run.toml` (phase 2), `argus docs --for-agent` and `argus mcp` (phase 2), tile pathfinding under `t.expect:path` (with the tilemap and navigation grid; navigation itself is phase 3), replay and clip export on test failure (phase 2).

## 0. The Luau cost model and the broadphase

**Status: done** (2026-09-18) in `argus-ecs` (`spatial.rs`, `World::spawn_with`), `argus-sim` (the `broadphase.rebuild` tick step, `SystemCtx::within/nearest/spawn_with`), `argus-luau` (native `vector` for `vec2`, `q:spawn(names)`, `q:spawn_many`, `q:within`, `q:nearest`), `argus-index` (`Source::near`), and the daemon (lazy rebuild before a `within` query between ticks). Traces in each crate's `SPEC-TRACE.md`; the before/after table is in `crates/argus-luau/SPEC-TRACE.md`. On the stress scene the tick fell from 280 ms to 49 ms and the hit pass from 176 ms to 0.14 ms; what remains is the binding's per-call cost in per-entity loops, which is the Rust promotion's job. The phase-0 gate found that Luau at 50,000 entities cost 265 ms a tick, attributed by `prof.alloc` to a table allocation per `vec2` field access (200k allocations a tick in Swarm's chase system, 750k in its hit pass) and, at spawn, to one binding call per component per entity. Constraints:

- `vec2` fields read and write as Luau's native `vector` value: no allocation, `v.x`/`v.y`, arithmetic in the VM. Tables `{x=, y=}` stay accepted on write (§7 sandboxed, §19 "Luau performance ceiling").
- Spawning many entities is one call: `q:spawn(components)` places the entity in its final archetype at once, and `q:spawn_many(n, components, fn)` loops in Rust and hands each entity to a Luau callback.
- The shared broadphase of §12: a spatial hash over the live world, rebuilt each tick, that `world.query`'s `within` predicate hits instead of scanning (§17: `world.query` over 100k entities under 2 ms), and that Luau reaches as `q:within(pos, r, components...)` and `q:nearest(pos, components...)`. Physics tiers in item 6 build on it.
- The promotion path §19 names ("promote a Luau system to Rust without changing anything around it") stays open: declared write sets are unchanged.
- Measured on the phase-0 gate's stress scene before and after; the numbers go in `docs/phase0-gate.md`'s successor table and the `argus-luau` trace.

## 1. wgpu sprite renderer, windowed and offscreen

**Status: done** (2026-09-18) in `crates/argus-render` (wgpu 24, winit 0.30) and the daemon (`render` feature, on by default): `Sprite` and `Camera` components, one instanced pipeline with a draw per atlas run and culling in the vertex shader, offscreen readback to PNG and a winit window presented every frame, interpolation by the clock's `alpha`, atlases from `assets/*.png` with a `.atlas.toml` sidecar, described-shape placeholders. Protocol: `obs.frame {format: "png"}`, `render.window`, `render.status`; `argusd --window [WxH]` and `argus daemon run --window`. Trace: `crates/argus-render/SPEC-TRACE.md`. Measured: 200,000 sprites over two atlases and four z values in 8 draw calls, 23 ms to build and submit on the development laptop; the frame build is the CPU cost and is the item's known limitation. Both example games now draw (swarm: ring, fans, player; delve: rooms, monsters, stairs) and were captured with `argus obs frame`. Constraints: §2 (the core hosts the renderer; rendering is decoupled from the fixed-rate sim and interpolates between the last two sim states, §6); §17 Swarm 200,000 instanced sprites per frame, one draw call per texture atlas, GPU-side culling; offscreen mode is what tests and CI render into (§9 "identically in CI with the software render adapter"); `present` stage systems read only; the render ring is what marks and clips are cut from (§10, phase 2); placeholder sprites as described shapes (§14) render without an asset.

## 2. Keyboard, mouse, gamepad input with recording

**Status: done** (2026-09-22) in `crates/argus-input` (`devices.rs`, `InputState::apply_devices`) and the daemon (window key and mouse events, gilrs gamepads, `input.devices`, `--play`). Device values enter the same schedule injects use, so the log is actions by tick (or commands by turn) and a device-driven log replays with no devices; on a discrete scene a key press takes a turn while playing. Trace: `crates/argus-input/SPEC-TRACE.md`. Not exercised by hand: pressing physical keys and a physical gamepad; the tests drive the same entry point with synthetic events, and the live window opens and reports its devices. Constraints: §12 bindings per device and per player slot already declared in `input.toml` are read now; two gamepads are two players; the input log records actions not device events (§6), so a replay survives a rebind; rebinding at runtime is a UI over the same table; accessibility hold/toggle options (§12); `input.record` from phase 0 captures the same log from real devices; injected and real input merge per slot.

## 3. Audio mixer with event log

**Status: done** (2026-09-22) in `crates/argus-audio` (config, tones, mixer, cpal output) and the daemon (`audio.status`, `audio.play`, `audio.stop`, `audio.bus`, `audio.clip`, `audio.device`; requests answered in the event log; the device opens with the window), plus `q:play` in the Luau binding. Buses in a routing tree, ducking as data, spatialization from an `AudioListener` or the camera, variation sets with seeded jitter, a first layer model for music driven by a `Music` component, described tones as placeholders. Reading the meters on Swarm's first wave found the mix clipping at +18 dB; a per-sound voice cap and a master limiter fixed it (-2.5 dB peak, limiter idle). Tempo and bar-synchronised transitions stay in phase 3. Trace: `crates/argus-audio/SPEC-TRACE.md`. Not heard by a person: the device opened and closed on the laptop's speakers, and the clips are WAV files nobody has listened to yet. Constraints: §13 named buses (music, sfx, ui, ambience) with a routing tree; ducking rules as data ("dialogue ducks music by 6 dB over 200 ms"); 2D spatialization by distance from the listener entity; variation sets with pitch jitter; adaptive music as layers with tempo, beat/bar transitions, and an intensity parameter a system writes; §8 audio observed as events ("played sfx/hit_heavy at frame 812, gain 0.8, on entity e:4011"); the human hears it, the agent reads it. Buses, ducking, and the adaptive music system are listed again in phase 3, so phase 1 is the mixer, events, variation, spatialization, and a first music layer model.

## 4. Asset importer with sidecars and watch

**Status: done** (2026-09-22) in `crates/argus-assets`, with the renderer loading sprites from its catalog (frames and pivots) and the mixer loading sidecar sounds; the daemon scans on open, polls twice a second, logs `assets.scanned`, `asset.imported`, `asset.warning`, `asset.failed`, `asset.removed`, hot-reloads sprites and sounds, and answers `asset.list`, `asset.get`, `asset.import`. A described-shape sidecar is drawn into the cache as a real texture, so the art that replaces it later is the same asset under the same name. Both example heroes are such placeholders. Native Aseprite and Tiled readers stay in phase 5 as planned; the sidecar holds their structure. Trace: `crates/argus-assets/SPEC-TRACE.md`. Constraints: §14 `assets/` with sidecar TOML the agent can write (frames and pivots, clip lists and rates, gain and variation sets); the importer watches the tree, caches in `.argus/`, and reports every import as an event with warnings; described-shape placeholders; Aseprite and Tiled native import is phase 5, the sidecar shape must already carry their structure; §4 `.argus/` is a cache, never committed.

## 5. Tilemaps with chunks and auto-tiling

**Status: done** (2026-09-22) in `crates/argus-tiles` (a `*.tilemap.toml` index with a folder of sparse chunks per layer, auto-tiling, collision, the `Tilemap` component, visible-tile queries), `argus-assets` (`[autotile.<terrain>]` rules and tile properties in the tileset's sidecar), `argus-render` (`render_with` draws tiles in the sprite pass), `argus-scene` (maps beside entity files are data, not entities), and the daemon (`scene.tiles.create`, `scene.tiles.info`, `scene.tiles.collision`, `scene.tiles.get` over tilemaps, and `edit.tiles` in transactions with per-author undo). The design names only `scene.tiles.get` and routes every write through transactions, so the write is `edit.tiles` rather than a `scene.tiles.set`. Auto-tiling has three modes: one tile, 16 tiles by edges, and 47 tiles by edges and corners. Collision combines the layers marked for it into empty, solid, and one-way cells, which item 6 reads. Trace: `crates/argus-tiles/SPEC-TRACE.md`. Measured in release at 4,096 × 4,096 tiles and 8 layers: a 64 × 64 auto-tiled brush stroke plus save is 4.8 ms worst, a screen's visible tiles 0.18 ms, a reopen 3.8 ms, and a 1,024 × 1,024 fill plus save 85 ms, which is the known cost of a big edit. Checking a map in a real scene through the command line found that the scene loader read the map's index as an entity file and refused the scene; data indexes are now skipped, and entity ids that collide with scene file names are refused. Constraints: §12 layers over a grid, auto-tiling rules in TOML, collision from tile properties, chunked storage (§4 bulk chunks from phase 0); editable through the protocol at the rect level; §17 4,096 × 4,096 tiles and 8 layers without pauses; Hollow and Outpost are the customers.

## 6. Kinematic character controller

**Status: not started.** Constraints: §12 three tiers on one broadphase (item 0): overlap (contacts only), kinematic (against tilemap and other kinematic bodies: slopes, one-way platforms, moving platforms, step-up, coyote time, jump buffering, corner correction as tune fields), dynamic (optional Rapier module, later); collision is an event (§8 L2); "the tier worth getting exactly right".

## 7. Camera component

**Status: not started.** Constraints: §12 follow with dead zones, lookahead, room bounds, smoothing, shake as a curve with decay, all tune fields; cameras are entities; a cut between two is an event; the semantic frame reports the camera rect (§8).

## 8. Sprite animation state machines with frame events

**Status: not started.** Constraints: §13 Animator component from TOML: states reference clips, transitions have conditions on component fields and blend windows, events by frame are ordinary engine events a test can assert; clip rates, anticipation and hold frames are tune fields; Aseprite tags import as clips (with item 4); §17 2,000 animators under 0.3 ms per tick, "transitions are field comparisons, not script".

## 9. Tweens

**Status: not started.** Constraints: §13 a tween is a component (target path, from, to, duration, easing, on-complete event), queryable, reproduced exactly by replay; timelines above tweens are phase 3.

## 10. Emitter pools

**Status: not started.** Constraints: §13 Emitter component (shape, rate or burst, lifetime, curves over lifetime as control-point lists); particles are not entities, each emitter owns an SoA pool updated in Rust and drawn instanced; §17 250,000 live particles under 1 ms; the post stack is phase 2.

## 11. Materials with hot reload

**Status: not started.** Constraints: §13 a material is a WGSL shader plus typed parameters declared in a sidecar; the shipped library (lit, unlit, palette, dissolve, outline); compile errors through `script.errors` with line numbers; hot reload like scripts (§17 under 100 ms).

## 12. Semantic frames and annotated screenshots

**Status: not started.** Constraints: §8 L4 `obs.frame {format: "semantic"}` (camera rect, every visible entity with screen-space bounds, HUD text, active effects, about 3 KB) and L5 `obs.frame {format: "png", annotate: true}` with entity ids and bounds overlaid; the Describe component for game-extensible frames; the same call is a screen-reader feed (§12 accessibility); Delve lives at L4.

## 13. obs.diff, obs.trace, obs.why

**Status: not started.** Constraints: §8 L3 `obs.diff {since: frame}` (appeared, vanished, changed which fields, with counts, about 1 KB); `obs.trace {entity, frames}` one entity's component history over a window (needs a bounded history; the snapshot ring proper is phase 2); `obs.why {event}` walks declared write sets backwards from an event to list the systems that could have caused it, in stage order; the phase-0 daemon trace records that events plus `who_writes` were enough for the gate and that `obs.trace` moves here.

## Gate

Items 1 to 3 done: Swarm and Delve open in a window, draw, take keyboard and gamepad input, and make sound (`argus daemon start --window --play`). The footstep sound waits for the animator's frame events (item 8). The Hollow slice (items 5, 6, 7, 12); the animation with a footstep that fires a sound (items 3, 8).
