# argus-test: spec trace

Every statement in the design document (v0.3) that constrains the scenario test runner (§9 "Test harness", §6 determinism, §2 clients, §16 `argus test`, §18 item 10), with where it lives and the test that pins it. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §9 Test harness

| Spec | Status | Where | Test |
|---|---|---|---|
| A test is a scenario: load a scene, fix a seed, script some input, run some frames, assert on state and events. | Done | `harness.luau`: `scenario "name" { scene, seed | seeds, run = function(t) ... end }` registers; `run_seed` loads the scene with the seed through `scene.load`, records the event sequence number, builds `t`, and runs. | `runner::a_scenario_runs_headless_and_every_expectation_kind_reports` |
| It runs headless in the daemon. | Done | `test.run` is a daemon method; the harness's only Rust dependency is a `Driver` that answers protocol methods, which the daemon implements on itself. Nothing in this crate touches a world. | same |
| Takes tens of milliseconds for a short scenario. | Done | The fixture scenario (scene load, three injected inputs, a run to an event, ten expectations including a snapshot and two perf reads) takes about 10 ms in a debug build; the test asserts under a second. | same |
| Never flakes, because §6. | Done | Every step is a protocol call over a deterministic sim; the only wall-clock reads are `perf`, which asserts on a percentile with a margin. The golden test runs the scenario twice and compares bit for bit. | same |
| `t.input:hold("move", {1, 0}, frames = 60)`, `t.input:tap("fire")`. | Done | `t.input:hold(action, value, frames)` (third argument a number or `{frames = n}`; without it, held until `t.input:release`), `t.input:tap(action, value?)`: both are `input.inject`. `t.input:release` sends the action's rest value (false, 0, or {0, 0}, by what `input.state` holds); it used to send no value, which is the active one, so it pressed. Field paths reach one axis of a vec2 (`Transform.pos.x`). Both found by the phase-1 gate. | same, `runner::a_release_lets_go_and_a_field_path_reads_one_axis` |
| `t:run_until { event = "wave.cleared", max_frames = 1200 }`; `{ sim_time = days(30), speed = "max" }`; `t:advance { by = "turn", n = 1 }`. | Done | `sim.run_until {event, max_ticks}`, `{time}`, `{ticks}` and `sim.advance`. `speed = "max"` is what headless always is. Also `t:step(n)`, `t:status()`, `t:call(method, params)`, `t:log(msg)`. | same; `runner::a_series_declared_after_the_run_is_sampled_by_rerunning` |
| `t.expect:count({ has = {"Enemy"} }):eq(0)`. | Done | `world.query {select: ["id"], where, limit: 1}` and its `total`. Matchers on every number: `eq ne lt lte gt gte near between`. | same |
| `t.expect:field("@player", "Health.cur"):gte(1)`. | Done | `world.get` and the component field; `eq` on a table compares deeply (vectors, refs). | same |
| `t.expect:events("damage", { target = "@player" }):count():lte(3)`. | Done | `obs.events {name, since: <the scenario's start>}` filtered by a subset match on `fields`; `:count()`, `:exists()`, `:first()`. The scenario's start is the `run_from` a scene load returns: the load's own events (registrations, spawns) are left out, and what `on_load` systems emit, such as Delve's first `floor.generated`, is in. | same, `examples/delve/tests/stairs.luau` |
| `t.expect:snapshot("wave1_end", { select, where })`: golden snapshots of selected state, not images; the harness stores a compact JSON, and a failure prints a field-level diff. | Done | The query's rows (`id` always first) go to `tests/goldens/<scenario slug>/<name>.seed<seed>.json`. Missing: recorded and reported as such. Present: rows matched by id, each selected field compared, and the expectation carries `diff: {changed: [{id, field, golden, actual}], added, removed}` and a message listing them. `update` rewrites. | same |
| `t.expect:perf("sim"):p95_ms():lt(4.0)`; perf assertions on a percentile with a generous default margin. | Done | `prof.systems` for the stage's systems and `prof.worst {n: 600}` for per-tick sums; the p95 of the stage's per-tick total. `"tick"` or `"frame"` reads the whole-tick p95. `lt`/`lte` bounds are multiplied by the scenario's `perf_margin` (default 1.5) and the message shows both. | same |
| `t.expect:path("@player", { has = {"Stairs"} }):exists()`. | Done (over tiles since phase 3 item 2) | When a navigation grid covers the entity, the harness asks `nav.path` (A*) from it to each candidate until one is reached; the message names the steps and the grid. Where no grid covers it, a candidate in a room reachable over the room graph passes (`scene.rooms`, `world.get` reports `room`), and in a flat scene one that exists. | Delve's `tests/stairs.luau` (200 seeds, over its floor); the fallback in `tests/runner.rs` |
| `t.expect:sum("Stockpile.food"):gte(60)`. | Done | `world.query {select: [field], where}` summed. | same |
| `t.expect:series("Stockpile.food", every = days(1)):trend():gte(0)`. | Done | A series samples the sum at declaration and after every chunk of a run, where runs are split into `every`-sized steps while a series is registered. Declared after the run it should watch (as in the example), the seed is run again with the series in place, which the deterministic sim makes exact; the result says `reran`. `:trend()` is the least-squares slope per sample; also `:last() :min() :max() :samples()`. | `runner::a_series_declared_after_the_run_is_sampled_by_rerunning` |
| `seeds = range(1, 200)` and the runner's `--seeds 1..50` seed sweep. | Done | `range(a, b)`, a list, or a single `seed`; none uses the scene's seed policy and reports the seed it got. `test.run {seeds}` overrides with `"7"`, `"1..50"`, or `"1,2,9"`. | `runner::a_scenario_runs_headless_and_every_expectation_kind_reports`, `runner::seed_specs_parse` |
| `days(30)` and friends. | Done | `seconds minutes hours days` as sim-time seconds. A day is the project's (`[time] day`, passed as `Options::day_seconds`; phase 3, decided with Kevin): §9's 30 days at 86,400 s and §17's 2,000 ticks a second would be 21.6 hours a seed against §17's "under a minute"; at Outpost's 60 s it is 54 s. An hour is a 24th of a day. | same; `runner::days_and_hours_are_the_projects` |
| Run one test by name. | Done | `test.run {name}`: exact or substring; `{file}` for one file. | same |
| Compact text report by default, JSON with `--json`. | Done | The report is JSON from the daemon; `argus test` prints one PASS/FAIL line per scenario, one line per failing seed and expectation, and a summary (`argus-cli/src/render.rs::test_report`). | `argus-cli/tests/cli.rs::the_cli_talks_to_a_daemon_over_its_socket` |
| Run a failing test and dump a replay file the human can open (in the editor, phase 4; with `sim.seek` until then); export a clip of the failing window. | Done (phase 2 item 3) | A run's first failed expectation, or its error, calls the driver's `replay.save` right then (`save_failure` in `harness.luau`), so the replay is of the moment it failed, not of where the run ended: `.argus/replays/<scenario>/seed<n>/` holds the replay file (the keyframe five seconds back, the input since, the world then), the frame, a clip of the two seconds before it and its contact sheet, and `replay.json` with the failure's message and the scenario, file, and seed. The first five failures of a run get clips; a sweep that fails everywhere keeps every replay without spending a minute drawing. The seed's row carries the summary, and `argus test` prints `replay at frame N: argus replay open path=...` under the failure. `replay.open` reproduces it in the daemon, paused at the frame, with the lead-in in the log and the traces, so `sim.seek` and a window work on it. `replays: false` (`--no-replays`) saves none. | `runner::a_failing_run_saves_a_replay_of_the_moment_it_failed`, `argus-cli/tests/cli.rs` |
| Image goldens as an opt-in with a perceptual threshold. | Deferred | Phase 1 with the renderer. | |
| Everything runs identically in CI with the software render adapter. | Done | The runs themselves use no renderer; `argus test` exits 0 only when every run passed. A failing run's pictures are drawn with the software rasteriser with `software: true` (`argus test --software`), and a machine with no adapter at all still gets the replay file, its pictures skipped with the reason. | `runner::a_failing_run_saves_a_replay_of_the_moment_it_failed` (clips drawn in software) |

## §6, §2

| Spec | Status | Where | Test |
|---|---|---|---|
| Same scene, seed, input log, and code: bit-identical. | Done | The golden comparison and the series rerun both rely on it and both pass. | both runner tests |
| The runner is a client of the same daemon. | Done | `Driver` is the whole surface; the daemon reloads the scene that was open when the run ends, so a test run is a visitor, not an owner. | `daemon.rs::test_run` |
| A test edits the world, not the project. | Done | During `test.run` an edit is live only: a set on an authored entity changes the world and writes no override, a despawn deletes no file, and a spawn with an id (which would be a file) is refused with a hint. Found in phase 2 item 1: Hollow's planks test moved the hero with `edit.commit`, which wrote the new position into `player.toml`, and the phase 1 gate commit carried it. | `argus-daemon/tests/test_run.rs::a_scenario_edits_the_world_and_leaves_the_files` (fails with the guard off) |

## Errors, stated so they are decisions

- A scenario that throws is one failed seed with the error and its `tests/x.luau:N`, found by the traceback when the message has no position (a protocol error raised inside the harness). Other seeds and scenarios still run.
- A file that does not load is a `load_errors` entry, the run reports `ok: false`, and the other files still run.
- The runner itself failing (no `tests/`, a broken harness) is error 4709 `TEST_RUNNER`, distinct from a failing test.

## §18 Build plan

Phase 0 item 10, "scenario test runner with state goldens and seed sweeps": this crate, `test.list` and `test.run` in the daemon, and `argus test` in the CLI.

## Phase 3, item 3

| Spec | Status | Where | Test |
|---|---|---|---|
| (§9) The balance scenario: `t.expect:sum("Stockpile.food"):gte(60)` over a 30-day run on 20 seeds. | Done | `sum` is the daemon's now (`world.query` with `sum(F)`), one small answer however many entities; it read every row itself before. Outpost's `tests/outpost_food.luau` is §9's scenario, its series declared before the run (declared after it, as §9 writes it, the harness runs the seed again to sample from the start: thirty more days a seed). | Outpost's `tests/outpost_food.luau` |
| A run is as long as it was asked to be. | Done | `t:run_until {sim_time}` and `{frames}` without a series go on in calls of at most 100,000 ticks until they are there; one call stopped at the daemon's cap, 27.8 of Outpost's days. | Outpost's `tests/outpost_food.luau` (thirty days) |

## Phase 3, item 8: scenarios with mods

| Spec | Status | Where | Test |
|---|---|---|---|
| (§15) The agent tests a mod as the game is tested. | Done | A scenario's `mods = {"a", "b"}` loads those mods from the project's `mods/`, in that order, and fails if any is left out; a scenario without `mods` loads none, whatever ran before it. | Hollow's `tests/mods.luau` |

## Phase 4, item 3: candidates tried without writing them

| Spec | Status | Where | Test |
|---|---|---|---|
| (§10) "Runs the twenty-seed, thirty-day sweep from §9 against each candidate value." | Done | `Options::set`: fields the harness puts in each run's world with `sim.set` just after its scene loads, before the scenario looks at it. The daemon's `test.run {sweep}` runs the scenarios once for each candidate. | argus-daemon `tune_defs::a_sweep_tries_each_candidate_without_writing_it` |

## Known limitations, stated so they are decisions

- The harness is Luau over one Rust function (`call`). That keeps the runner a client by construction and makes the harness readable by the people who write scenarios; it costs a JSON round trip per call, which at tens of calls per scenario is invisible.
- Goldens are keyed by scenario slug, snapshot name, and seed; a scenario renamed leaves its goldens behind under the old slug.
- `perf` reads the profiler ring, so its window is at most the last 600 ticks of the run.
- `path` answers over a navigation grid when one covers the entity, else over the room graph; a scene with neither only checks that a candidate exists.
- A series declared after its run costs one extra run of that seed.
