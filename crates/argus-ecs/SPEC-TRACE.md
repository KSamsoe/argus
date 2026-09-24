# argus-ecs: spec trace

Every statement in the Argus Engine design document (v0.3, `docs/argus-engine-design-v0.3.html`) that constrains the ECS core, with where it lives and the test that pins it. Section numbers are the document's.

Status words:

- **Done**: implemented here and tested.
- **Partial**: implemented here as far as the ECS can take it; the rest is named.
- **Deferred**: belongs to a later phase-0 piece or a later phase. Listed so it is not dropped; the ECS hook it will use is named.

## §2 Architecture

| Spec | Status | Where | Test |
|---|---|---|---|
| The daemon holds exactly one world and a stack of snapshots. | Done | `World`, `Snapshot`; the daemon's keyframe ring (phase 2 item 0) holds them with the sim state beside, and a fork is a second world parked beside the first. | `maintain_snapshot::snapshot_restore_round_trips_everything` |
| (Phase 2 item 0.) A seek across a room transition restores the rooms with the world. | Done | In argus-scene: `Scene::stream_state` and `set_stream_state` hold which rooms are loaded, the current one, what was spawned for each, and the references still pending; a keyframe carries them. | `argus-daemon/tests/seek.rs` (a transition is a barrier keyframe) |
| Entering play takes a snapshot; stopping restores it; the snapshot is a memcpy of SoA columns costing single-digit ms at Swarm's entity counts. | Done | `Column::clone` is one `memcpy`; `World::snapshot`, `World::restore`. Measured 1.1 ms at 102k entities. | `perf::perf_snapshot_100k_under_5ms` |
| Multiple concurrent play sessions are forked worlds from a snapshot inside the same daemon. | Done | `World::fork`, `Snapshot::fork`. | `maintain_snapshot::forks_evolve_independently` |
| A script or plugin fault must not take the daemon down; the world remains intact and inspectable. | Partial | No ECS operation panics on protocol-reachable input: every fallible call returns `ArgusError`; unknown string ids resolve to `""`; hierarchy cycles are reported, not looped. Remaining asserts (typed view size mismatch, row out of range) are programmer errors unreachable through the protocol. Script sandboxing is the Luau piece. | `hierarchy::cycles_are_reported_not_hung`, `components::bad_schemas_are_rejected_with_reasons` |

## §3 Data model

| Spec | Status | Where | Test |
|---|---|---|---|
| Archetype ECS: entities are ids; components are plain data stored column-wise per archetype; systems are functions over queries. | Done | `archetype.rs`, `column.rs`, `query.rs`. | `queries::*` |
| An entity id is a 64-bit value (index + generation). | Done | `Entity::from_parts`, `index`, `generation`; slots recycle with a generation bump. | `entities::entity_id_is_index_plus_generation`, `entities::stale_handles_die_and_slots_are_recycled` |
| Authored entities carry a stable string id (`player`, `door.east`), unique within the scene and stable across sessions, saves, and renames. | Partial | `World::spawn_authored`, `World::entity("@player")`, `string_id`; ids survive snapshot/restore. "Across sessions" is the scene-file piece (filename = id). In a room scene an id is `room/id` (`a/player`); `World::entity("@player")` finds the one `*/player` when there is exactly one, and names the candidates when there are several. Found by the phase-1 gate: Hollow's HUD text and its pit script said `@player` and got nothing. | `entities::authored_ids_are_unique_and_resolvable`, `entities::a_bare_id_finds_the_one_room_scoped_entity_it_means`, `maintain_snapshot::snapshot_restore_round_trips_everything` |
| Runtime-spawned entities never get string ids and are never written to scene files. | Partial | `World::spawn`; `is_authored` is what the scene writer will filter on. | `entities::runtime_entities_never_get_string_ids` |
| Transform hierarchy is first-class: `Parent` gives a local transform, propagated in a dedicated stage before `present`. | Partial | `hierarchy::{Transform, GlobalTransform, Parent, propagate}`. Registering `propagate` in the `hierarchy` stage is the scheduler piece. | `hierarchy::roots_copy_and_children_compose`, `hierarchy::rotation_and_scale_compose`, `hierarchy::children_processed_in_depth_order_regardless_of_spawn_order` |
| Swarm's bullets simply don't have the component and pay nothing. | Done | Only archetypes with `Parent` are walked; roots are a column copy. 100k bullets + 2,000 children propagate in 0.5 ms. | `hierarchy::entities_without_parent_are_only_copied_and_others_untouched` |
| A component is a schema plus a column; declared once, introspectable everywhere: from Rust via derive, or from Luau via a schema file. | Done | `argus-ecs-derive`, `SchemaBuilder` (the Luau binding calls it), `World::describe`. | `components::derive_produces_an_introspectable_schema`, `components::runtime_schema_matches_rust_layout_and_is_indistinguishable` |
| Every field has a type, a default, an optional range, and a doc string. | Done | `FieldSchema`. | `components::derive_produces_an_introspectable_schema` |
| The doc string is what `argus describe` returns, and therefore what the agent reads. | Done | `World::describe` returns the schema with docs. | same |
| `tune = true` marks a feel parameter; changes nothing at runtime. | Done | `FieldSchema::tune` is metadata only; the daemon's tuning log (phase 2 item 5) records every change to one and `tune.set` moves only these. | same, `argus-daemon/tests/tuning.rs` |
| Hot fields declared in Rust get a native SoA column with no VM boundary; cold fields can live in Luau schemas; the protocol does not distinguish. | Done | One `Column` type for both; Rust types view it as `&[T]`; both are reachable by `Component.field` path. C layout rules make a Rust struct and a script schema with the same fields byte-identical. | `components::runtime_schema_matches_rust_layout_and_is_indistinguishable`, `components::runtime_schema_layout_follows_c_rules` |
| Systems run in stages `input → sim → late → hierarchy → present`; `sim` is the only stage that may mutate gameplay state; `present` is read-only. | Deferred | Scheduler piece. Uses `Access` and `propagate` from here. | |
| Each system declares the components it reads and writes; the agent uses it to answer "what could have changed Health". | Partial | `Access` with `reads`/`writes`; `Access::from_names` for script declarations. `who_writes` is the scheduler/daemon piece over the same data. | `queries::declared_access_is_enforced` |
| Tags are zero-size components. | Done | Stride-0 `Column` never allocates. | `components::tags_are_zero_size` |
| Relations are components holding entity references; the engine nulls dangling refs at frame end and emits an event, so nothing crashes on a stale handle and the agent can see it happened. | Done | `World::maintain` / `end_frame`, `Event::DanglingRefNulled`. Covers Rust and runtime schemas. | `maintain_snapshot::dangling_references_are_nulled_at_frame_end_with_an_event`, `maintain_snapshot::dynamic_schema_entity_fields_are_maintained_too`, `hierarchy::dead_parent_is_nulled_at_frame_end_and_child_becomes_root` |
| Prefabs and overrides (per-field inheritance, `-Component` removal, child paths, resolved provenance, instance counts on change). | Deferred | Scene/prefab piece of phase 0. The ECS side it needs is present: field-level `get_component`, `insert_default`, `remove`, `Event::ComponentAdded/Removed`. | |
| Decision: archetype ECS with SoA columns, declared read/write sets, authored-versus-runtime as a first-class concept. | Done | as above | |

## §5 Protocol (the parts the ECS defines)

| Spec | Status | Where | Test |
|---|---|---|---|
| Entity refs are written `e:4011`; string ids `@player`. | Done | `Entity` `Display`/`FromStr` (a recycled slot prints `e:4011.2`); `World::entity` accepts both forms. | `entities::entity_id_is_index_plus_generation`, `entities::authored_ids_are_unique_and_resolvable` |
| Field paths are `Component.field` (`Transform.pos`, `Health.cur`). | Done | `Registry::resolve_path`, `World::get_field`, `World::set_field`. | `components::field_paths_read_and_write_with_coercion_and_range_checks` |
| `world.summary`: entity counts by archetype, named entities and their key fields, active systems, current frame and seed. | Partial | `WorldSummary` has counts by archetype, named entities, frame. Key fields, active systems, seed are the daemon/scheduler pieces. | `maintain_snapshot::summary_and_archetype_stats_orient_in_one_screen` |
| Error shape: numeric code, `"WaveSpawner.interval_s must be in range [0.05, 10]; got 0.0"`, a hint, a `file:line` ref. Code 4102 for range. | Done | `ArgusError`, `codes`, `ComponentSchema::check`. Message format matches the spec's example exactly; `ref` is the derive's `file!():line!()` or the script schema's `source`. | `components::field_paths_read_and_write_with_coercion_and_range_checks` |
| `world.describe`, `prof.archetypes` data. | Done | `World::describe`, `World::archetype_stats`. | `maintain_snapshot::summary_and_archetype_stats_orient_in_one_screen` |
| `edit.*` transactions, author tags, change feed, undo/redo, locks. | Deferred | Daemon piece; wraps `World` mutation and `Event`s. | |

## §6 Determinism and time

| Spec | Status | Where | Test |
|---|---|---|---|
| A snapshot is every SoA column plus the RNG states, the event queue, and the Luau heap. | Partial | Columns, entity table, string ids, string table, event queue, frame counter: done. RNG states join with the scheduler piece; the Luau heap with the scripting piece (see §19 open question). | `maintain_snapshot::snapshot_restore_round_trips_everything` |
| A snapshot at 100k entities is a few tens of MB and takes low single-digit ms. | Done | 4.1 MB and 1.1 ms for the example's component set. | `perf::perf_snapshot_100k_under_5ms` |
| `sim.fork` creates a second world from a snapshot inside the same daemon; forks get their own tick counter. | Partial | `fork`; each `World` has its own `frame`. The sim tick is the clock's. | `maintain_snapshot::forks_evolve_independently` |
| Components carry a version (save games). | Partial | `ComponentSchema::version` (`#[argus(version = N)]`). `migrate` is phase 3. | `components::derive_produces_an_introspectable_schema` |
| Floating point is `f32`. | Done | `FieldType::F32`, `Vec2` is two `f32`. | |
| (Phase 2 item 2.) A snapshot on disk, so a mark outlives the daemon. | Done | `World::encode_state` and `decode_state` (`persist.rs`, over the `codec` module's `Writer` and `Reader`): archetypes in order with their rows in order and their columns as bytes, every entity slot with its generation and the free list in order, the string table, authored and short ids. Iteration order is part of determinism, so a read-back world iterates as the saved one did. It reads into a world with the project's components registered: matched by name, copied as bytes when laid out as saved, field by field when changed (new fields take their defaults), dropped when gone; strings are interned into the reading world's table and every string field translated. A damaged file is error 4010 and leaves the world as it was. The events queue is not saved (a keyframe's events are in the log). | `persist::a_world_reads_back_exactly`, `persist::strings_are_translated_into_the_reading_worlds_table`, `persist::changed_components_are_copied_by_field_and_gone_ones_dropped`, `persist::damaged_bytes_are_an_error_and_leave_the_world_alone` |
| (Phase 2 item 2.) Two worlds are the same world. | Done | `World::digest`: every field of every live entity by value, components by name, floats by their bits, strings as text, so two processes, platforms, or string tables agree. The web player (item 1) and marks compare it. | `argus-web/tests/player.rs`, `argus-daemon/tests/marks.rs` |

## §7 Scripting and hot reload

| Spec | Status | Where | Test |
|---|---|---|---|
| A system may only touch components it declared; violations are errors. | Done | `Query::access`, `ArchetypeView::check`; typed fetches are checked once per query, dynamic columns per fetch. Code 4301 with a hint naming the list to fix. | `queries::declared_access_is_enforced`, `queries::archetype_views_give_typed_and_dynamic_access` |
| A system may not hold references across frames. | Partial | Column guards borrow from the query and `&World`; structural changes need `&mut World`, so Rust cannot hold one. Enforcing it for Luau is the binding's job. | compile-time |
| All state lives in components, so swapping code never loses state. | Done | Columns hold no code or pointers; `World` is `Clone`. | |

## §8 Observation

| Spec | Status | Where | Test |
|---|---|---|---|
| L0 summary. | Done | `WorldSummary`. | as above |
| L2 events: spawn, despawn, ... system errors, edits. | Partial | Structural events: `Spawned`, `Despawned`, `ComponentAdded`, `ComponentRemoved`, `DanglingRefNulled`, `HierarchyCycle`, `ComponentRegistered`, each tagged with its frame. `Event::Game { name, fields }` is the shape gameplay events (damage, death, collision, sound) take; `SystemFault` and `SystemDisabled` are emitted by the scheduler (piece 2). Edit events are the daemon's. | `entities::spawn_and_despawn_emit_events_with_ids`, `argus-sim/tests/faults.rs` |
| L3 diff, `obs.trace`, `obs.why`. | Deferred | Phase 1. `obs.why` walks `Access` declarations. | |

## §9 Test harness

Deferred to the scenario-runner piece. State goldens will serialize `get_component` and query rows.

## §15 Shipping

"Nothing in the sim stage may read from a device or the wall clock." The ECS has no clock; `World::frame` is a counter. The lint is the scheduler's.

## §17 Performance targets the ECS core owns

Release build, one thread, development laptop. Gates are `tests/perf.rs`; numbers from `examples/perf.rs`.

| Target | Spec | Measured |
|---|---|---|
| Live entities at 60 Hz sim (Swarm) | 100,000 | 100,000 spawned with 3-4 components each in 60 ms |
| Sim tick, p95 (Swarm) | < 6 ms | one read/write system over 100k: 0.13 ms |
| `world.query` over 100k entities | < 2 ms | filtered typed query: 0.02 ms (spatial predicates are phase 1) |
| Full snapshot at 100k entities | < 5 ms | 1.1 ms |
| Authored entities per scene (Hollow) | 10,000 | hierarchy with 2,000 children propagates in 0.5 ms |
| Runtime-generated rooms queryable the tick they exist (Delve) | immediate | queries see structural changes with no rebuild: `queries::queries_see_structural_changes_made_between_passes` |

## §12 The shared broadphase (phase 1, item 0)

| Spec | Status | Where | Test |
|---|---|---|---|
| Three physics tiers sharing one broadphase: a spatial hash over the live world, rebuilt incrementally each tick. | Done (the hash; the overlap and kinematic tiers are item 6, in `argus-physics`) | Item 6 added `SpatialHash::for_each_within`, a visit that allocates nothing, for the contact pass over every collider each tick. | `spatial.rs`: `SpatialHash` of uniform cells over `(entity, x, y)`; `World::rebuild_broadphase` reads `GlobalTransform.pos` when present, else `Transform.pos`, straight from the columns; `World::within(x, y, r)` visits the cells a circle covers and checks distance exactly, nearest first; `SpatialHash::nearest` searches rings outward with a filter. Rebuilt from scratch each tick (2.3 ms at 100k); incremental maintenance is a later optimisation behind the same calls. | `spatial::within_and_nearest_come_from_the_hash_with_exact_distances`, `perf::perf_broadphase_rebuild_and_within_100k` |
| `world.query` over 100k entities under 2 ms: spatial predicates hit the broadphase hash, not a scan (§17). | Done | `within` at r=30 over 100k uniformly spread entities: 8 µs. The evaluator narrows its candidates through `Source::near` before reading a field. | `perf::perf_broadphase_rebuild_and_within_100k` |
| Spawning many entities is one placement, not one move per component (§19 batch spawn). | Done | `World::spawn_with(&[ComponentId])` builds the default rows and pushes the entity into the target archetype once; `ComponentAdded` events still fire per component. | `spatial::spawn_with_places_the_entity_in_its_archetype_at_defaults` |

## Phase 1, item 8: reading fields every tick

| Spec | Status | Where | Test |
|---|---|---|---|
| Transitions are field comparisons, 2,000 animators under 0.3 ms (§17). | Done | `DynColumn::num_by(row, field, axis)` reads a numeric, bool, or vec2-axis field as a number, and `DynColumn::str_id_by` a string field's id, without building a `Value` (which resolves strings and allocates). The animator reads every condition this way. | `argus-anim/tests/anim.rs::perf_two_thousand_animators` |

## Phase 1, item 13: what the history and diffs read

| Spec | Status | Where | Test |
|---|---|---|---|
| `obs.diff`, `obs.trace`, `obs.why` (§8). | Done | `EventRecord.by`: the system that emitted it, from `World::set_emitter`. `World::with_row` shows a component's row as bytes, so a history keeps rows and a diff compares them without building values; `World::decode_field` reads a field out of kept bytes; `World::live` walks every live entity; `World::named` walks the named ones without collecting them. | `argus-daemon/tests/history.rs`, `argus-sim/tests/observe.rs` |

## Phase 1, item 6: what the physics step needed from the core

| Spec | Status | Where | Test |
|---|---|---|---|
| A system's queries are checked against its declared access (§3). | Done | A query now borrows the running system's access set (`Query::access_ref`) instead of copying it, which was two allocations per query in every Rust system; `Query::access` still takes an owned set. | `argus-daemon/tests/protocol.rs` (physics with no bodies allocates nothing) |
| Schemas describe fields with their defaults (§3, §8 L1). | Done | A Rust `f32` default is described by its shortest decimal (`0.12`, not `0.11999999731779099`). Found by item 6: the index keeps 15 significant digits, so the live and index answers of `world.describe` disagreed on the controller's defaults. | `argus-daemon/tests/index_shapes.rs::live_and_index_reads_have_identical_shapes` |

## §18 Build plan

Phase 0 item 1, "ECS core with SoA columns, archetypes, and hierarchy": this crate. The other phase-0 items are tracked in `docs/phase0-checklist.md`.

## §19 Open questions touching this crate

- Whether the snapshot includes the Luau heap: the ECS snapshot is data-only, so either answer composes with it.
- TOML versus KDL: the ECS is format-agnostic; `Value` is what any loader produces.

## Known limitations, stated so they are decisions

- Column borrow flags are `Cell`s, so `World` is not `Sync`. Parallel stage scheduling (phase 5) replaces runtime flags with static per-stage checks from `Access`.
- The string table is append-only. A game that mints unbounded distinct strings at runtime grows it for the life of the world.
- `propagate` does a hash lookup per child. Fine at Hollow's thousands; revisit if a game parents tens of thousands.
- The registry is per world and cloned on fork. Register every component before forking.
- Typed writes to Rust components may copy uninitialized padding bytes into a row. Rows are never exposed as byte slices, only copied with `ptr::copy`, so this is not observable; snapshot comparisons in tests compare values, not bytes.
- `insert` on an existing component overwrites in place and emits no event. The edit transaction layer will report value changes.
