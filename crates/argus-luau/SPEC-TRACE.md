# argus-luau: spec trace

Every statement in the design document (v0.3) that constrains the Luau binding, with where it lives and the test that pins it. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §2 Architecture

| Spec | Status | Where | Test |
|---|---|---|---|
| Luau systems run sandboxed inside the daemon with instruction budgets. | Done | `ScriptHost::new`: `Lua::sandbox(true)`, `set_memory_limit`, `set_interrupt` counting VM interrupts against `ScriptConfig::interrupt_budget`, reset per system run. `io`, `os`, and global writes are unavailable. | `limits_reload::an_unbounded_loop_hits_the_budget_and_is_disabled`, `limits_reload::memory_limit_is_enforced`, `declarations::globals_are_frozen_and_the_sandbox_holds` |
| A script fault must not take the daemon down; the world stays inspectable. | Done | Script errors are `ArgusError`s returned from the system body; the scheduler disables the system and emits `SystemFault` with the script's `file:line`. | `systems::runtime_errors_point_at_the_script_line_and_disable_the_system` |

## §3 Data model

| Spec | Status | Where | Test |
|---|---|---|---|
| Schemas declared from Luau via a schema file: `component "HitStop" { doc, frames = field.u8 { default, range, doc }, scale = field.f32 { ..., tune = true } }`. | Done | `decl.rs` installs `component`, `field.<type>`; specs become `SchemaBuilder` calls with `source = file:line`. Field order follows declaration order (a per-field counter, since Lua tables are unordered). | `declarations::component_declaration_becomes_an_introspectable_schema` |
| Every field has a type, default, optional range, doc string; `tune` flag. | Done | `parse_field`; all fifteen `FieldType`s are exposed as `field.<name>`. | `declarations::field_types_defaults_and_tags` |
| Hot fields in Rust, cold in Luau; the protocol does not distinguish. | Done | Script components register through the same `World::register_schema`; Rust and script systems can read either. | `systems::rust_and_script_systems_share_one_schedule_and_one_contract` |
| System declaration: `system "sim.hitstop" { stage, after, reads, writes, run = function(q) for e, hs, ts in q:each("HitStop", "TimeScale") do ... end end }`. | Done | `decl.rs` `system`; `bind.rs` `QueryHandle::each` yields an entity proxy plus one component proxy per name; the spec's example runs verbatim. | `systems::the_spec_example_system_runs` |
| (Found when Kevin played Delve.) `on_load = true` in a system declaration. | Done | `decl.rs` reads it into `SystemDesc::on_load`: the system also runs once when the scene loads, before the first tick. Delve's `sim.floor` uses it, so the first floor is on screen before the first key. | `examples/delve/tests/stairs.luau` (the floor exists at turn 0), `argus-sim/tests/load.rs` |
| Tags are zero-size components. | Done | `component "Enemy" {}`. | `declarations::field_types_defaults_and_tags` |
| Relations are components holding entity references. | Done | `field.entity`; proxies accept entity handles or `@id` / `e:n` strings; null reads as `nil`. | `systems::spawn_despawn_insert_emit_entities_and_rng` |

## §4 Project and scene storage

| Spec | Status | Where | Test |
|---|---|---|---|
| `components/` Luau component schemas, one per file; `systems/` Luau systems, one per file. | Done | `ScriptHost::load_project` scans `components/`, `defs/`, `systems/` recursively in sorted order. A file may declare more than one thing; the convention is not enforced. | `declarations::a_project_directory_loads_in_order` |
| Data tables in `defs/` with declared schemas. | Done | `def "recipes" { key = "id", prefix = "recipe", ... }` declares the row schema (`DefDecl`), reusing `ComponentSchema`; `prefix` is the `item` in `item:key.brass` and defaults to the table name. `argus-scene` loads the rows against it and serves `world.defs` queries. | `declarations::def_schema_declaration` |

## §5 Protocol

| Spec | Status | Where | Test |
|---|---|---|---|
| Errors carry a sentence, a hint, and a `file:line` ref. | Done | Range and type errors from script writes carry the schema's ref; runtime errors carry the script line; declaration errors carry the declaring line. Hints from `ArgusError` are appended to `ScriptError.message`. | `systems::range_violations_from_script_carry_the_schema_ref`, `declarations::errors_are_file_line_data_never_a_crash` |
| `script.reload`, `script.systems`, `script.errors`. | Done | `ScriptHost::reload`, `Scheduler::systems` (sources are script `file:line`), `ScriptHost::errors`. | `limits_reload::reload_swaps_code_and_keeps_world_state_and_rng_streams` |

## §6 Determinism

| Spec | Status | Where | Test |
|---|---|---|---|
| Randomness from named streams. | Done | `rng = "name"` in the declaration; `q:rng()` exposes `next`, `below`, `range`, `chance`; undeclared use is a structured error. | `systems::script_systems_are_deterministic_under_the_same_seed` |
| Snapshot includes the Luau heap (open question §19). | Decided | The strict way, as the design leans: the heap is not in keyframes, and a Luau system keeps no state of its own (globals are read-only; a file-level local is the one place left, and it is unsupported). Sequencing state moves to timelines (§13), built in phase 3 item 5 (`argus-timeline`: a timeline playing is a `Timeline` entity a script spawns, and all it is doing is in the world). A system that keeps state anyway replays differently, and `sim.seek` reports it as `diverged`, naming the first event that differs, instead of handing back a wrong world quietly (phase 2 item 0). | `argus-daemon/tests/seek.rs::a_system_that_keeps_state_outside_the_world_shows_as_a_divergence` |

## §7 Scripting and hot reload

| Spec | Status | Where | Test |
|---|---|---|---|
| Game logic is Luau: sandboxed, gradually typed, small. | Done | mlua 0.10 with the `luau` feature; type annotations parse. | `declarations::*` |
| `script.reload` recompiles the changed files, rebinds system functions, and continues from the current frame. | Done | `reload` hashes files, re-evaluates changed ones, `Scheduler::replace_system` keeps the slot, enabled state resets, RNG stream state is kept when the stream name is unchanged; world and tick untouched. Deleted files remove their systems. | `limits_reload::reload_swaps_code_and_keeps_world_state_and_rng_streams` |
| The daemon freezes script globals after module load so nothing can accidentally live there. | Done (stricter) | Luau sandbox makes globals read-only even during load; a global write is a load error and a run-time write is a fault. Module state must be a `local`. | `declarations::globals_are_frozen_and_the_sandbox_holds` |
| The agent sees a compile error as a structured `script.errors` response with `file:line` and the compiler's message, never as a crash. | Done | `ScriptError { file, line, message, phase }`; syntax, load, and declaration phases. | `declarations::errors_are_file_line_data_never_a_crash` |
| A system may only touch components it declared: errors at load time where detectable, at run time otherwise. | Done | Load time: `reads`/`writes` must name registered components (`Access::from_names`). Run time: `q:each` and `e:get` check reads; proxies are read-only unless the component is in `writes`; `e:insert`/`e:remove` check writes. | `declarations::errors_are_file_line_data_never_a_crash`, `systems::undeclared_access_is_an_error_at_run_time` |
| A system may not hold references across frames; violations are run-time errors. | Done | Every proxy holds a link to the running context that is nulled when `run` returns; a stale use is error 4502 with a hint. | `systems::handles_held_across_frames_fail_clearly` |
| (Phase 2 item 6.) A script system can run through a hit-stop. | Done | `unfrozen = true` in a system's declaration: it runs on frozen ticks (a script's own effect that should play through a stop); others skip them. | `systems::an_unfrozen_script_system_runs_through_a_hit_stop` |
| Component `migrate` functions per schema version (§6 save games; phase 3). | Done (phase 3 item 0) | §19's "probably both, table first": `migrate = { [2] = { rename = { old = "new" }, default = { f = v }, drop = { "f" } }, [3] = function(row) ... end }`, a step per version reached, or one `migrate = function(row, from)`. Steps are checked where declared (versions 2 to the component's). `ScriptHost` implements `argus_ecs::Migrate` (`migrate.rs`): a table step runs in Rust; a function gets the row as a table, under a system's instruction budget, and its errors name the step and the line. Entity fields pass through as handles that can be kept, not used. | `declarations::component_declaration_becomes_an_introspectable_schema`, `migrations::steps_by_version_bring_a_row_up_from_any_older_one`, `migrations::one_function_takes_the_row_and_the_version_it_was_saved_at`, `migrations::a_migration_is_checked_where_it_is_declared_and_where_it_runs` |
| (Phase 3 item 0.) A game asks for its own saves; its days are the project's. | Done | `q:save(name)` and `q:load(name)` emit `save.requested` and `load.requested` (default name "auto"), which whatever runs the game answers between ticks. `q.day_length`, `q.day`, and `q.day_time` count in `[time] day` (`ScriptConfig::day_seconds`, from argus.toml through `assemble`). | `argus-daemon/tests/saves.rs`, `argus-player/tests/saves.rs`, `examples/outpost` |
| Changing a component's shape needs a version bump and a migration. | Done | A reload that changes a registered schema's shape is reported with the "bump the version and add a migration" hint; the running schema is untouched. | `limits_reload::reload_of_a_changed_component_shape_is_a_reported_conflict` |

## §8 Observation

| Spec | Status | Where | Test |
|---|---|---|---|
| Gameplay events (`damage`, `wave.cleared`) in the event log. | Done | `q:emit(name, fields)` → `Event::Game`; fields sorted by name for determinism. | `systems::spawn_despawn_insert_emit_entities_and_rng` |

## §15 Shipping

| Spec | Status | Where |
|---|---|---|
| Nothing in the sim stage may read from a device or the wall clock. | Done | The sandbox exposes no `os`, `io`, or clock; `q.time`, `q.tick`, `q.dt` are sim time. |
| Release builds precompile Luau to bytecode. | Deferred | Phase 2 `argus build`. |

## §17 Performance

| Target | Spec | Measured (release) |
|---|---|---|
| Luau hot reload | < 100 ms | asserted in `limits_reload`; single-file reload is a few ms |
| Script system over 50k entities, four field ops each | (no target; the §19 risk) | 40 ms per tick, about 0.2 µs per field access; roughly 300× the same system in Rust (0.13 ms). The spec's answer is to move a hot system to Rust; Swarm's bullet integration is exactly that case. |

## Phase 1, item 0: the cost model the phase-0 gate found

| Spec | Status | Where | Test |
|---|---|---|---|
| `vec2` fields read and write without allocating (the gate's finding: a table per access, 200k allocations a tick in Swarm's chase). | Done | `bind.rs`: a `vec2` read is Luau's native `vector` (`Value::Vector`, x and y with z = 0), so `v.x`, `v + w`, `v * s`, and `vector.magnitude(v)` run in the VM; a write accepts a vector or a table. | `systems::vec2_fields_are_native_vectors_and_spawn_many_and_within_reach_the_broadphase` |
| Spawning many entities is one call. | Done | `q:spawn({"A", "B"})` places the entity in its archetype at defaults in one move; `q:spawn_many(n, {"A", "B"}, function(e, i) ... end)` loops in Rust and hands each entity to the callback through one reused handle. | same |
| Luau reaches the broadphase. | Done | `q:within(pos, r, "A", "B")` iterates nearest first with the distance last; `q:nearest(pos, r, "A", "B")` returns one entity and its distance. Reads are checked against the declaration like `each`. | same |
| The promotion path stays open. | Done | Nothing about declarations changed; Swarm's systems were rewritten in place without touching their `reads`/`writes`. | `examples/swarm` tests |

Measured on the phase-0 gate's stress scene (50,000 enemies alive, release build, the same laptop):

| Measure | Phase 0 | Item 0 |
|---|---|---|
| Tick p95 | 280 ms | 49 ms |
| `sim.hits` p95 (bullets against enemies, enemies against the player) | 176 ms | 0.14 ms |
| `sim.chase` p95 (50,000 vector reads and writes) | 88 ms | 46 ms |
| Allocations per tick | 997,000 | 286,000 |
| `sim.chase` allocations per entity | 4 (tables) | 5 (the binding's call path) |
| Spawning 50,000 enemies | 279 ms | 172 ms |

What remains in `sim.chase` is the binding's call path: each iteration is three calls across the boundary (the iterator, a field read, a field write), and mlua allocates its argument and result lists per call, about a microsecond per entity per system. That is the floor for per-entity Luau loops, and it is what the promotion to Rust removes; the §17 target of 6 ms at 50k stays a Rust target, as §19 says.

## Phase 1, item 3: asking for a sound

| Spec | Status | Where | Test |
|---|---|---|---|
| The game asks for sfx/step and the mixer chooses (§13). | Done | `q:play(name, {entity, gain, pitch, bus, pan, loop})` emits a `sound.play` event with those fields; the daemon's mixer answers with `sound.played`. Event fields now carry vectors as `vec2`. | `argus-daemon/tests/audio.rs::scripts_ask_for_sounds_and_the_log_says_what_played` |

## Phase 1, item 9: tweens from a script

| Spec | Status | Where | Test |
|---|---|---|---|
| A tween is a component; squash-and-stretch and camera nudges are tweens (§13). | Done | `q:tween(e, "Component.field", {from, to, duration, delay, easing, repeats, yoyo, on_complete, keep})` spawns an entity with a `Tween` driving that field of `e` and returns it; a number for `from` or `to` drives a scalar, a vector a vec2. The calling system declares a write of `Tween`. An unknown option is an error naming the options. (The field is `repeats`, since `repeat` is a Lua keyword.) | `argus-daemon/tests/tween.rs` |

## Phase 1, item 6: reacting to contacts

| Spec | Status | Where | Test |
|---|---|---|---|
| Collision is an event (§8 L2); a script reacts to it. | Done | `q:events(name)` returns the game events emitted so far this tick with that name (all of them with no name), oldest first, each a table of its fields plus `name`; entity fields are entity handles. A late-stage system reads the physics step's `body.landed` or `overlap.begin` the tick they happen. | `systems::a_script_reads_the_events_emitted_earlier_in_its_tick`, `argus-daemon/tests/physics.rs` |

## Phase 3, item 1: a pause

| Spec | Status | Where | Test |
|---|---|---|---|
| (§12) Every game's pause menu: the game stands still while the menu runs. | Done | `q:freeze()`: the rest of this tick stands still, but for systems declared `unfrozen` (the UI's input and layout, and a game's menu systems): phase 2 item 6's hit-stop freeze, asked for by a script. Hollow's `sim.pause` and Delve's `sim.menu` use it. | Hollow's `tests/pause.luau`, Delve's `tests/menu.luau` |

## Phase 3, item 2: path queries

| Spec | Status | Where | Test |
|---|---|---|---|
| (§12) Delve's monsters and any game with NPCs use the same path queries. | Done | `q:path(from, to, grid?)` (the waypoints as vectors and the cost in steps, or nil), `q:walkable(pos, grid?)`, `q:distance(field, pos)` (steps to a `NavField`'s entity), `q:downhill(field, pos)` (the next cell's centre toward it). A point is a vector, a table, or an entity. The host is handed the queries (`ScriptHost::set_paths`, the `Paths` trait) so this crate needs no navigation of its own. A system that asks declares it reads `NavGrid` or `NavField`, so `obs.why` stays true. | `argus-daemon` `nav::a_maze_is_searched_through_the_protocol_and_from_scripts`, `nav::a_system_that_asks_for_a_path_must_say_it_reads_the_grid`; Delve's monsters |

## Phase 3, item 3: data tables, speed, and where an error is

| Spec | Status | Where | Test |
|---|---|---|---|
| (§4) "Outpost is mostly defs": its systems read items, recipes, and job kinds from tables. | Done | `q:def("item:meal")` is a row by reference and `q:defs("jobs")` a table's rows in file order, each a read-only table of its fields plus `key` and `ref`, made once and kept (the `DefRows` trait; the assembly hands the host the project's tables, `ScriptHost::set_defs`, again after a reload or a move). | Outpost's systems (`kitchen.luau`, `work.luau`, `found.luau`); its tests |
| (§6) Outpost's 1x/3x/10x speed controls are `sim.speed`. | Done | `q:speed(x)` emits `speed.requested`; the daemon, a release build, and the page set their clock's speed from it after the step. Not 0: nothing would tick to ask for another speed, so a game pauses with `q:freeze()`. | Outpost's `tests/colony.luau` ("speed and pause") |
| (§7) Script errors carry `file:line`. | Done | A range or type error on an engine component (declared in Rust) now names the script line that wrote it, with the component's declaration in the hint; a script component's error still names its declaration, where the range is written (phase 0's rule). Found building Outpost: a `JobKind.work` out of range pointed at `crates/argus-jobs/src/lib.rs`. | `systems::range_violations_on_an_engine_component_point_at_the_script_line`, `systems::range_violations_from_script_carry_the_schema_ref` |

## Phase 3, item 4: words by key

| Spec | Status | Where | Test |
|---|---|---|---|
| (§12) A script's player-facing words go through the string table. | Done | `q:text(key, args?)`: the key's string in the world's language (the base's where it lacks one), `{name}` holes from `args` and `{@id:Component.field}` from the world, `[key]` where no table has it. `q:languages()`: the languages there are, the base first, for a settings screen. The host is handed the tables (`ScriptHost::set_texts`, the `Texts` trait). | `systems::a_script_asks_for_its_words_by_key`; Hollow's HUD and settings, Delve's messages, Outpost's HUD |

## §18 Build plan

Phase 0 item 3, "Luau binding with component and def schemas": this crate.

## §19 Open questions and risks touching this crate

- Luau heap in the snapshot: not snapshotted, and per-system state is unsupported; see the §19 row. A seek reports a system that keeps it.
- Luau performance ceiling: per-entity proxies allocate once per `each` call, not per entity, and `vec2` values cross as native vectors, but every field access is still a Rust round trip with mlua's per-call allocations (about a microsecond per entity per system at 50k). Promoting a system to Rust changes nothing around it because the declaration contract is identical.

## Phase 2, item 7: bytecode

| Spec | Status | Where | Test |
|---|---|---|---|
| (§15) Luau precompiled to bytecode in a release build. | Done | `compile(rel, src)`: Luau bytecode with the options loading source uses (optimisation 1, line numbers kept), or the syntax error as a `script.errors` row with its line. `load_project` and `reload` read bytes, and `load_bytes` takes source or bytecode, which Luau tells apart by the first byte. A run-time error in bytecode still names the file and line. | `bytecode::bytecode_runs_as_its_source_does_and_keeps_its_lines`, `bytecode::a_syntax_error_is_caught_when_compiling_with_its_line` |

## Phase 3, item 8: mods' scripts

| Spec | Status | Where | Test |
|---|---|---|---|
| (§15) A mod's Luau systems run "with the same sandbox and instruction budgets as the game's own scripts". | Done | `ScriptHost::load_layers`: each mod's `components/`, `defs/`, and `systems/` load into the same VM after the game's, named from the game's root (`mods/<id>/systems/x.luau`); `reload` walks the layers too. What a mod file declares must be in its namespace; a mod redeclaring a game's system, component, or def table is refused (`load_bytes`). | argus-player `mods::a_mod_that_breaks_a_rule_is_left_out_whole_and_the_game_runs`, `mods::a_mods_runaway_system_is_stopped_by_the_same_budget_as_the_games` |

## Known limitations, stated so they are decisions

- One Luau VM per `ScriptHost`; a forked world needs its own host (or the same host with a different scheduler, since the VM holds no world state).
- `q:each` snapshots the entity list, so entities spawned during the loop are visited next tick.
- Numbers cross the boundary as doubles; integer fields accept integral doubles and reject fractions with a type error naming the field.
- Def schemas are declared here; their rows are loaded by `argus-scene`.
- Static analysis of `run` bodies (which components a function touches) is not attempted; the runtime checks are exhaustive.
