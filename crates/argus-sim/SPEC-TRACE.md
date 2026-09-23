# argus-sim: spec trace

Every statement in the design document (v0.3) that constrains the scheduler, clocks, randomness, profiling, and fault isolation, with where it lives and the test that pins it. Statuses as in `argus-ecs/SPEC-TRACE.md`: **Done**, **Partial**, **Deferred**.

## §2 Architecture

| Spec | Status | Where | Test |
|---|---|---|---|
| "Editing" is the world with the simulation stages disabled; "playing" is with them enabled. Entering play takes a snapshot; stopping restores it. | Done | `Scheduler::play` snapshots world + sim state on the first call from edit mode; `stop` restores both; frames in edit mode run only `present`. | `clocks::play_snapshots_and_stop_restores`, `clocks::continuous_clock_runs_speed_ticks_per_frame` |
| A script or plugin fault must not take the daemon down. A panic in a system is caught at the system boundary, the system is disabled, and the fault is reported through the protocol as an event. The world remains intact and inspectable. | Done | `Scheduler::run_system` wraps every system in `catch_unwind`; on panic or `Err` the system is disabled and `Event::SystemFault` plus `Event::SystemDisabled` are emitted with the message and the declaration's `file:line`. Later systems in the same tick still run. | `faults::a_panicking_system_is_disabled_and_reported_and_others_keep_running`, `faults::an_erroring_system_is_disabled_with_the_structured_error` |
| Multiple concurrent play sessions are forked worlds inside the same daemon. | Partial | `Scheduler` is per world; `SimSnapshot` (world + sim state) is what a fork starts from. The daemon holds one scheduler per fork. | `prof_snapshot::full_snapshot_carries_world_and_sim_state` |

## §3 Systems and stages

| Spec | Status | Where | Test |
|---|---|---|---|
| Systems run in a fixed order of stages: `input → sim → late → hierarchy → present`. | Done | `Stage::ALL`; `run_tick` runs input, sim, late, then `argus_ecs::propagate`, then hierarchy systems, then `World::end_frame`; `present` runs per render frame. | `stages::stages_run_in_fixed_order_and_after_orders_within_a_stage` |
| `sim` runs on the game clock and is the only stage that may mutate gameplay state; `present` runs at render rate and is read-only. | Done | Tick stages run on clock ticks; `present` runs in `frame` regardless of play state. A `present` system that declares writes is rejected at registration (code 4403), and its context therefore refuses every write. `late` and `input` may write (cleanup and action delivery). | `stages::present_stage_is_read_only`, `clocks::discrete_clock_only_advances_on_command` |
| Each system declares the components it reads and writes. The scheduler uses this for parallelism, but the agent uses it to answer "what could have changed Health?" with a single query. | Partial | `SystemDesc::access`; `SystemCtx` binds every query and structural change to it (code 4301 otherwise). `Scheduler::who_writes` / `who_reads` answer in stage and run order. Parallel stage scheduling is phase 5. | `stages::context_enforces_declared_access_for_queries_and_structural_changes`, `stages::who_writes_answers_in_stage_order` |
| System declaration shape: `stage`, `after`, `reads`, `writes`, `run`. | Done | `SystemDesc` builder mirrors the Luau declaration one to one; `Access::from_names` for the script path. | `stages::*` |
| `after` orders systems within a stage. | Done | Kahn topological sort per stage with lowest-registration-index tie-break, so order is deterministic. Unknown name (4402), cross-stage `after` and cycles (4404) are structured errors. | `stages::ordering_errors_are_structured` |
| Transform hierarchy is propagated in a dedicated stage before `present`. | Done | `argus_ecs::propagate` is the first step of the hierarchy stage every tick and is timed as `hierarchy.propagate`. | `stages::hierarchy_propagates_every_tick_and_maintenance_runs_at_frame_end` |
| Dangling refs are nulled at frame end. | Done | `World::end_frame` at the end of every tick. | same |

## §5 Protocol (`sim.*`, `prof.*`, `script.systems`)

| Spec | Status | Where | Test |
|---|---|---|---|
| `sim.play`, `sim.pause`, `sim.step`, `sim.advance`, `sim.speed`, `sim.run_until`. | Done | `play`, `pause`, `stop`, `step`, `advance`, `set_speed`, `run_until`, `run_until_event`, `run_until_time`. `sim.seek/snapshot/fork/save/load` are phase 2 and 3, built on `snapshot`/`restore`. | `clocks::*` |
| `script.systems`: list systems. | Done | `Scheduler::systems` returns name, stage, after, reads, writes, clock, rng, enabled, disabled reason, source, doc in run order. | `stages::stages_run_in_fixed_order_and_after_orders_within_a_stage`, `faults::a_panicking_system_is_disabled_and_reported_and_others_keep_running` |
| The scheduler already times every system every tick; `prof.*` just exposes it. `prof.systems {last: 600}` returns per-system mean and p95 in stage order. | Done | `Profiler` ring (600 ticks by default); `prof_systems(last)`. `hierarchy.propagate` appears as its own row. | `prof_snapshot::every_system_is_timed_every_tick` |
| `prof.worst {n}` returns the slowest recent ticks with a per-system breakdown and the entity counts by archetype at that tick. | Done | `prof_worst(n)`; `TickRecord::archetypes`. | same |
| `prof.frame`. | Done | `prof_last`. | same |
| `prof.alloc` counts allocations by system so a Luau system that builds a table per entity is visible. | Done | `alloc::Counting`, a global allocator wrapper with thread-local allocation and byte counters; the scheduler reads them before and after every system and `hierarchy.propagate`, so each `SystemSample` and `TickRecord` carries `allocs` and `bytes`. `prof_alloc(last)` gives mean, max, and total per system in stage order; `alloc::installed()` says whether the binary opted in (argusd does). Luau allocates through the same global allocator (`mlua` hands it to `lua_newstate`), so script systems are counted like Rust ones. | `prof_snapshot::allocations_are_counted_by_system`; `argus-luau/tests/systems.rs::a_script_system_that_builds_a_table_per_entity_is_visible_in_prof_alloc` |
| `prof.capture {ticks: 300}` writes a Tracy-compatible trace for the human. | Done | `capture_start(ticks)` arms a `Capture`; each of the next `ticks` ticks records every system span (start and duration on one time base, allocations, bytes) and the tick's entity count; `capture_take` returns it once complete. `Capture::chrome_trace` writes the Chrome trace event JSON that Tracy's bundled `import-chrome` tool converts to `.tracy` and that Perfetto and `chrome://tracing` open as is. Tracy's native wire format needs a live client connection and is not a file a daemon can drop; the import path is how Tracy itself takes offline traces. | `prof_snapshot::a_capture_collects_the_next_ticks_as_a_chrome_trace` |
| A perf regression is a query result with a system name on it. | Done | `SystemProf { name, mean_ms, p95_ms, max_ms }`; `prof_tick_p95_ms` for whole-tick assertions. | same |

## §6 Determinism and time

| Spec | Status | Where | Test |
|---|---|---|---|
| Given the same scene, seed, input log, and code version, the world at tick N is bit-identical. | Done (for scene + seed + code) | Deterministic ordering, per-stream RNG, no wall clock in the sim. Input log is item 7. | `rng_determinism::same_seed_same_run` |
| Two clock drivers behind one abstraction; every scene declares which one drives `sim`. | Done | `ClockKind`, `Clock`, `SchedulerConfig::driving`. | `clocks::*` |
| Continuous: the sim advances at a fixed rate (60 Hz default) whether or not anything happened; rendering is decoupled. | Done | `Clock::continuous(hz)`; `frame` runs the owed ticks then `present`. Interpolation is the renderer's (phase 1). | `clocks::continuous_clock_runs_speed_ticks_per_frame` |
| Discrete: the sim advances only on `sim.advance {by: "turn"}`; between advances nothing simulates; `present` still runs. | Done | `advance`; `frame` never ticks a discrete-driven sim but always presents. | `clocks::discrete_clock_only_advances_on_command` |
| The two are the same thing to the replay system: input by tick or commands by turn; seek, fork, marks, tests work identically. | Partial | Both clocks are `Clock` with a tick counter and the same `run_tick`; `SimState` carries both. The replay layer is phase 2. | `clocks::a_scene_can_mix_clocks` |
| A game can mix them; the scheduler lets a system group bind to either clock. | Done | `ClockBinding::{Driving, Continuous, Discrete}` per system; `advance` works in a continuous scene and `step` on the driving clock. The tactics-style hand-off (§19) is left for the prototype after phase 0; a continuous-bound system in a discrete-driven scene runs only when its clock is stepped. | `clocks::a_scene_can_mix_clocks` |
| Speed is a property of the continuous clock: `sim.speed {x: 3}` runs three ticks per render frame; `{x: 0}` is pause; headless fast-forward at whatever rate the CPU allows. | Done | `Clock::set_speed` with fractional accumulation; `run_until*` and `step` are the headless path. `speed` on a discrete scene is a `WRONG_CLOCK` error. | `clocks::continuous_clock_runs_speed_ticks_per_frame`, `perf::perf_headless_fast_forward_over_2000_ticks_per_second` |
| Wall-clock time is never visible to gameplay code; `sim.time` is tick count times the timestep on whichever clock is driving. | Done | `SystemCtx::{tick, time, dt}` are the only time the system sees; `Clock::time`. `Instant` appears only in the profiler. | `clocks::context_time_is_sim_time` |
| Randomness comes from named streams: each system declares a stream name and gets a PCG32 seeded from `hash(run_seed, stream_name)`, so adding or reordering a system does not perturb any other system's sequence. | Done | `rng::stream_seed` (FNV-1a + SplitMix64, integer only), `Pcg32` (matches the reference outputs), `SystemDesc::rng`, `SystemCtx::rng`. Undeclared randomness is an error (4406). | `rng_determinism::adding_another_random_system_does_not_perturb_a_stream`, `rng_determinism::pcg32_reference_and_helpers`, `rng_determinism::undeclared_randomness_is_an_error_not_hidden_state` |
| A snapshot includes the RNG states. | Done | `SimState::rng`; `SimSnapshot` = world snapshot + sim state; `restore` resumes the exact sequence. | `rng_determinism::sim_state_restores_mid_sequence_for_replay` |
| Forks get their own tick counter. | Done | Each `Scheduler` owns its clocks; a fork clones `SimState`. | `prof_snapshot::full_snapshot_carries_world_and_sim_state` |
| Run identity: `clock = "continuous@60"`. | Done | `Clock::parse` / `Display`. | `clocks::clock_specs_round_trip` |
| Floating point is f32 in the sim. | Note | Clock arithmetic is f64 (tick × timestep) so long sessions do not drift; component data stays f32. | |

## §7 Scripting

| Spec | Status | Where | Test |
|---|---|---|---|
| A system may only touch components it declared; violations are errors at load time where detectable, at run time otherwise. | Done (run time) | `SystemCtx` checks; a violating system returns `Err`, which disables it with a `SystemFault` carrying code 4301. Static checking of Luau bodies is the binding's. | `faults::an_erroring_system_is_disabled_with_the_structured_error` |
| Swapping code never loses state; reload continues from the current frame. | Partial | `disable` / `enable` / `remove_system` / `add_system` while a world is live are the primitives `script.reload` will use; the scheduler holds no game state. | `faults::disable_and_remove_by_request` |

## §9 Test harness

| Spec | Status | Where |
|---|---|---|
| `t:run_until { event, max_frames }`, `{ sim_time, speed = "max" }`, `t:advance { by = "turn", n }`, `t.expect:perf("sim"):p95_ms()`. | Partial | `run_until_event`, `run_until_time`, `advance`, `prof_tick_p95_ms` are the calls the runner (item 10) wraps. |

## §15 Shipping

| Spec | Status | Where |
|---|---|---|
| Nothing in the sim stage may read from a device, the wall clock, or a local-player singleton. | Partial | The context exposes none of these. The lint over Luau source is the binding's. |

## §17 Performance targets the scheduler owns

Release build, one thread, development laptop. Gates in `tests/perf.rs`.

| Target | Spec | Measured |
|---|---|---|
| Outpost headless fast-forward | > 2,000 ticks/s | 50k-entity swarm with three systems: ~48,000 ticks/s with the counting allocator installed (51,700 without) |
| Delve turn resolution, 500 monsters | < 2 ms | p95 0.05 ms |
| Scheduler overhead, empty tick with 10 systems | (internal) | 5 µs with allocation counting (2 µs without) |
| Swarm sim tick p95 | < 6 ms | 0.02 ms for the three-system swarm above |

"Profiling, always on at low cost" (§5 namespace table): the gates run with `alloc::Counting` installed, as `argusd` has it. The allocator adds two thread-local increments per allocation and two counter reads per system; the swarm loses about six percent of its headless rate, which is the price of `prof.alloc` being a query rather than a build flag.

## §12 The shared broadphase (phase 1, item 0)

| Spec | Status | Where | Test |
|---|---|---|---|
| The broadphase is rebuilt each tick. | Done | `run_tick` rebuilds it after `end_frame`, timed and allocation-counted as the `broadphase.rebuild` row at the end of the hierarchy stage, so the next tick's systems and any query between ticks see the tick's final positions. `SystemCtx::within`, `SystemCtx::nearest`, and `SystemCtx::spawn_with` (checked against the declared writes) are what the Luau binding wraps. | `prof_snapshot::every_system_is_timed_every_tick` (the row), `argus-luau/tests/systems.rs::vec2_fields_are_native_vectors_and_spawn_many_and_within_reach_the_broadphase` |

## Phase 1, item 9: write sets for data-driven systems

| Spec | Status | Where | Test |
|---|---|---|---|
| Each system declares the components it reads and writes; the agent answers "what could have changed Health?" with one query (§3). | Done | `SystemCtx::declare(component, write)`: a system whose targets come from data (a tween's path, an animator's conditions) declares each as it meets it; after the run the declarations join its access, so `who_writes`, `who_reads`, and `world.describe` stay true, and checked queries allow them from the next run. Declaring the same component again is free. | `stages::a_system_declares_what_its_data_touches_and_who_writes_follows`, `argus-tween/tests/tween.rs`, `argus-daemon/tests/animation.rs` |

## Phase 1, item 6: the physics step's place in the tick

| Spec | Status | Where | Test |
|---|---|---|---|
| The sim stage is the only stage that may mutate gameplay state (§3), so physics runs in sim, after the gameplay that sets velocities. | Done | `SystemDesc::last`: the system runs after every other system of its stage that does not order itself after it; `after` still holds, so a script may run after `physics.move`. `SystemInfo.last` reports it. Kahn's tie-break is now (last, registration index). | `stages::a_last_system_waits_for_the_rest_of_its_stage_and_events_are_seen_the_same_tick` |
| Collision is an event gameplay reacts to (§8 L2, §12). | Done | `SystemCtx::events_now(name)`: the events emitted so far this tick (by frame), so a later system reacts the same tick and never to an old tick's events. | same |
| The shared broadphase (§12). | Done | `SystemCtx::rebuild_broadphase`, which the physics step calls after moving bodies so its contact pass and later systems see where things are now. | `argus-physics/tests/physics.rs` |

## §18 Build plan

Phase 0 item 2, "scheduler with stages, declared write sets, and both clock drivers", plus the `prof.*` data of item 8 that depends on it: this crate.

## §19 Open questions touching this crate

- Mixed clocks: "is a system group binds to a clock enough?" The binding exists; the tactics prototype after phase 0 decides whether an explicit hand-off is also needed.
- Snapshot and the Luau heap: `SimState` carries no script state, compatible with either answer.

## Known limitations, stated so they are decisions

- One scheduler per world; forks need their own scheduler built from the same system list. The daemon piece owns that pairing.
- `present` systems run with the driving clock's tick and time; interpolation alpha is added with the renderer.
- `SystemCtx::query` clones the access set per call (two small vectors). Fine at phase-0 scale; switch to `Arc` if a profile ever shows it.
- The profiler stores `Arc<str>` names and a small vector per tick; at 48k ticks/s this is a few percent of an otherwise empty tick and invisible under real work.
- `prof.alloc` counts what the sim thread allocates while a system runs, which includes the ECS query machinery and Luau's page allocator batching (a thousand small tables show as a handful of 16 KB pages, so `bytes` is the column that makes a script system visible, `allocs` the one that catches a Rust system boxing per entity). Frees are not counted; a leak is a question for the snapshot, not the profiler.
- A capture holds one process and one thread: the sim thread. When systems run in parallel (phase 1) each worker becomes its own `tid` in the same file.
- A capture is written in the Chrome trace event format rather than Tracy's own; `tracy-import-chrome` is the documented way into Tracy for offline traces. A live Tracy client (the `tracy-client` crate) can be added behind a feature without changing the protocol.
- Panic payloads that are neither `&str` nor `String` are reported as "panic with non-string payload".
