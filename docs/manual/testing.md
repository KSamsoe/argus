# Testing

This chapter is about scenarios: the tests an Argus game keeps in `tests/`. It covers writing them, seeds, state and picture goldens, running them, reading a failure, the replay a failing run leaves behind, and why the same seed always gives the same run.

## What a scenario is

A scenario is a short Luau script that plays the game without a window. It loads a scene with a seed, presses the game's actions, runs some ticks, and then checks what must be true. `argus test` runs every scenario in the project and says which passed.

A scenario talks to the game the way every other client does: through the daemon's protocol. So anything the command line or an agent can ask, a scenario can ask too.

Two things to know from the start:

- **A test never changes your files.** While scenarios run, edits change only the live world. A scenario that moves the hero does not move the hero in the scene file the game ships.
- **The game is put back after.** When the run is over, the daemon loads the scene it had before, and plays on if it was playing.

The smallest one is in `templates/blank/tests/move.luau`:

```lua
-- A scenario: the game run headless with scripted input, and what must be
-- true after. `argus test` runs every file in tests/.
return scenario "the player walks right" {
  scene = "main", seed = 1,
  run = function(t)
    t.input:hold("move", { 1, 0 }, 60)
    t:run_until { frames = 60 }
    -- Six units a second for one second.
    t.expect:field("@player", "Transform.pos.x"):gt(5.5)
    t.expect:field("@player", "Transform.pos.y"):eq(0.0)
  end,
}
```

## Writing scenarios

### The file

Each `.luau` file directly in `tests/` returns one scenario, or a table of them. Files in folders under `tests/` are not read.

```lua
return {
  scenario "the player walks right" {
    scene = "main", seed = 1,
    run = function(t)
      -- ...
    end,
  },
  scenario "the player walks left" {
    scene = "main", seed = 1,
    run = function(t)
      -- ...
    end,
  },
}
```

`scenario "name" { ... }` takes these keys:

| Key | What it is |
|---|---|
| `scene` | Required. The scene to load, a folder name under `scenes/`. |
| `run` | Required. `function(t) ... end`, the scenario itself. |
| `seed` | One seed: `seed = 3`. |
| `seeds` | Several: `range(1, 20)` (both ends included) or a list `{1, 2, 7}`. |
| `mods` | Mod ids to load, in order: `mods = { "lanterns", "brighter" }`. |
| `perf_margin` | How far over a `perf` bound still passes. 1.5 by default. |

With neither `seed` nor `seeds`, the scene's own seed is used (`seed` in its `scene.toml`; `"random"` there means the runner picks one and the report says which).

### Moving time

| Call | What it does |
|---|---|
| `t:step(n)` | Run n ticks (one by default). |
| `t:run_until { frames = n }` | Run n ticks. |
| `t:run_until { event = "player.fell", max_frames = 120 }` | Run until that event happens, at most `max_frames` ticks (100,000 by default). |
| `t:run_until { sim_time = days(7) }` | Run until the sim clock reaches that time. |
| `t:advance { by = "turn", n = 1 }` | On a discrete (turn-based) clock, pass n turns. |
| `t:status()` | The sim's status now: tick, time, clock, seed, and more. |

`t:run_until` returns a table with `hit` and `ticks`. An event that never comes does **not** fail the run by itself, so check it:

```lua
local r = t:run_until { event = "wave.end", max_frames = 600 }
assert(r.hit, "no wave.end")
```

`t:run_until` always runs as fast as the machine allows. (You may see `speed = "max"` in `examples/outpost/tests/outpost_food.luau`: it is accepted and changes nothing.) A key it does not take is an error that names it, so a misspelt `max_frames` is caught rather than quietly running the default 100,000 ticks.

`seconds(n)`, `minutes(n)`, `hours(n)`, and `days(n)` turn game time into sim seconds. A day is the project's (`[time] day` in `argus.toml`), so Outpost's `days(30)` is 30 minutes of sim time.

### Pressing actions

A scenario presses the game's actions from `input.toml`, never raw keys:

| Call | What it does |
|---|---|
| `t.input:hold(action, value, frames)` | Hold an action at a value for n ticks. `{1, 0}` for a stick or axis pair, `true` for a button. Without `frames`, hold until released. |
| `t.input:tap(action, value)` | The action at a value for one tick. |
| `t.input:release(action)` | Back to rest (false, 0, or `{0, 0}`). |

Input is read when a tick runs. On a discrete clock, hold the action across the turn: a press and release with no tick between them is never seen. From `examples/hollow/tests/run.luau`:

```lua
t.input:hold("move", { 1, 0 }, 450)
t:step(118)                          -- up against the step in the cave
t.input:hold("jump", true, 18)
t:step(330)                          -- into the hall, short of its pit
```

### Asking the game anything

`t:call(method, params)` calls any protocol method (`argus docs` lists them all) and gives back its result. If the method fails, the run fails with its message. Delve's fight test asks where the nearest monster is, as any client would (`examples/delve/tests/fight.luau`):

```lua
local r = t:call("world.query", {
  select = { "id", "Transform.pos" },
  where = { has = { "Monster" } },
  order = "dist(@player)", limit = 1,
})
```

To set the stage, change the world with an edit. Hollow's pit test puts the hero over the pit (`examples/hollow/tests/pit.luau`). Remember that this changes only the live world, never the files:

```lua
local tx = t:call("edit.begin", { author = "test:hollow", note = "over the pit" }).tx
t:call("edit.set", { tx = tx, entity = "@player", path = "Transform.pos", value = { 58, 3 } })
t:call("edit.commit", { tx = tx })
t:run_until { event = "player.fell", max_frames = 120 }
```

`t:call("obs.events", ...)` reads the daemon's whole event log, from before this run too. Pass `since`, or use `t.expect:events`, which counts only this run's events.

`t:log(msg)` adds a line to the run's report. Log lines show in the JSON report (`--json`), not in the short text one.

## Expectations

`t.expect` holds the checks. Each one is checked at the moment it is written, so write it after the ticks that should make it true.

| Check | What it gives |
|---|---|
| `t.expect:field("@player", "Health.cur")` | A field's value now. `Transform.pos.x` reads one axis of a vec2. |
| `t.expect:count({ has = {"Enemy"}, within = { of = "@player", r = 3 } })` | How many entities match a query's `where`. |
| `t.expect:sum("Stockpile.food", where)` | A field added up over the matching entities. |
| `t.expect:events("damage", { by = "@player" })` | This run's events with that name (and those fields). Then `:count()`, `:exists()`, or `:first()` (the first event's row, its data under `.fields`). |
| `t.expect:path("@player", { has = {"Stairs"} })` | `:exists()`: a way from the entity to one that matches, over the navigation grid when there is one. |
| `t.expect:perf("sim")` | `:p95_ms()`: the stage's 95th percentile milliseconds over the last 600 ticks. `"tick"` is the whole tick. |
| `t.expect:series("Stockpile.food", { every = days(1) })` | Samples a field from now on. Then `:samples()`, `:last()`, `:min()`, `:max()`, `:trend()` (the slope per sample). |
| `t.expect:snapshot(name, { select, where })` | A state golden (below). |
| `t.expect:image(name)` | A picture golden (below). |

Most checks give a number to compare:

| Comparison | Passes when |
|---|---|
| `:eq(x)`, `:ne(x)` | Equal, or not. `eq` compares tables deeply too. |
| `:lt(x)`, `:lte(x)`, `:gt(x)`, `:gte(x)` | Less, at most, greater, at least. |
| `:near(x, eps)` | Within eps of x. |
| `:between(lo, hi)` | From lo to hi, both included. |
| `:get()` | Never fails: it gives the value itself, to use in Luau. |

Some details that matter:

- **A failed check does not stop the run.** It is recorded, and the run goes on to its end, so one report shows every check that failed. The replay is saved at the first failure.
- **An error does stop it.** `error(msg)`, `assert`, or a failing `t:call` ends the run and names the line in `tests/`. In a helper of your own, `error(msg, 2)` names the caller's line instead.
- **Declare a series before the run it watches.** Declared late, the harness runs that seed again from the start with the series in place. Outpost's food scenario declares it first for this reason, since a rerun would be thirty more days.
- **`perf` has slack.** With the default `perf_margin` of 1.5, `:lt(4.0)` passes up to 6 ms. Timing depends on the machine and how busy it is.

## Seeds

Every random number in a game comes from the run's seed, so one seed is one particular run. A scenario that should hold for any run says so with many seeds. Delve checks 200 generated floors (`examples/delve/tests/stairs.luau`, shortened):

```lua
return scenario "stairs are always reachable on floor 1" {
  scene = "delve", seeds = range(1, 200),
  run = function(t)
    t.expect:count({ has = {"Stairs"} }):eq(1)
    t.expect:path("@player", { has = {"Stairs"} }):exists()
    t.expect:count({ has = {"Monster"}, within = { of = "@player", r = 3 } }):eq(0)
  end,
}
```

Each seed is a fresh load of the scene. `--seeds` overrides every scenario's own seeds for one run, which is how you shake out a bug that only some runs hit:

```sh
argus test stairs --seeds 1..1000
```

## Goldens

A golden is a recorded answer. The first run writes it; later runs compare against it. Goldens live in `tests/goldens/<scenario>/`, named from the scenario's name and the seed, and belong in version control with the tests.

### State goldens

`t.expect:snapshot` keeps the rows of a query. From `examples/delve/tests/fight.luau`:

```lua
t.expect:snapshot("after_the_fight", { select = {"Transform.pos", "Health.cur"}, where = { has = {"Health"} } })
```

It is stored as `tests/goldens/walking_at_the_nearest_monster_attacks_it/after_the_fight.seed4.json`. When the world differs, the failure is a field-level diff:

```text
FAIL  golden walk  (main, seed 2, 534 ms)
    seed 2: snapshot after: differs from tests/goldens/golden_walk/after.seed2.json (1 changed, 0 added, 0 removed)
  @player Transform.pos: golden [0, 3], now [0, 4]
```

Rows are matched by entity id. Added and removed entities are listed with `+` and `-`.

### Picture goldens

`t.expect:image(name)` draws the frame with the software rasteriser at 640 x 360 and compares it with `tests/goldens/<scenario>/<name>.seed<n>.png`. Options: `width`, `height`, `threshold` (how different a pixel must be to count, 0.1) and `max` (the share of pixels that may differ, 0.001). From `examples/hollow/tests/looks.luau`:

```lua
t:step(1)
t.expect:image("cave")
```

On a failure the picture drawn and a difference image are written under `.argus/test-images/`, and the message names both.

Use picture goldens rarely, for rendering work. A picture golden holds on machines of one system only: Windows and Linux rasterise text and sprite edges differently, so a golden recorded on Windows fails on Linux. State goldens hold everywhere.

### Updating goldens

When a change is meant to change the answer, rewrite the goldens:

```sh
argus test look --update
```

Only goldens that no longer match are rewritten; the rest keep their files. Look at the new goldens (`git diff tests/goldens`) before you commit them, since an update accepts whatever the game does now.

## Running scenarios

```sh
argus test                                # every scenario
argus test walks                          # those whose name contains "walks"
argus test name="the pit puts the hero"   # a name with spaces, as a key
argus test --seeds 1..50                  # every scenario on seeds 1 to 50
argus test --file tests/move.luau         # one file
argus test --update                       # rewrite goldens that no longer match
argus test --json                         # the whole report as JSON
argus test list                           # what there is, without running it
```

A name matches when it is the scenario's whole name or part of it, ignoring case. Two more flags: `--software` draws failure clips with the software rasteriser (as CI does), and `--no-replays` saves no replays. `set` and `sweep` try other values for tune fields; they are in [Tuning and performance](tuning-and-performance.md).

The command starts the project's daemon if none is running. It exits with 0 when every run passed and 1 otherwise, so a script or CI can use it. An agent runs the same thing with the `test` tool.

The short report gives one line per scenario and the failures under it:

```text
PASS  the player walks right  (main, seed 1, 8 ms)
FAIL  walks too far  (main, 3 seeds 1..3: 0 ok, 3 failed, 1106 ms)
    seed 1: @player Transform.pos.x: 6 > 100 failed
    seed 1: replay at frame 60, with a clip: argus replay open path=<project>\.argus\replays\walks_too_far\seed1
    ...
3 scenarios, 5 runs, 3 failed, 1409 ms
```

## Reading a failure

A failure line says which check, the value it got, and what it wanted: `@player Transform.pos.x: 6 > 100 failed` means the field was 6 and had to be greater than 100.

Other failures you will meet:

| You see | It means |
|---|---|
| `tests/x.luau:12: ...` | An error in the scenario at that line, or a failing `assert`. |
| `world.get: error 4203: no entity with string id "nobody" (...)` | A `t:call` failed. The method's error code, message, and hint. |
| `scene hollow has 2 issue(s); the first: ...` | The scene's files have problems, so it did not spawn. `argus scene issues` lists them with file and line. |
| `mod lanterns was left out: ...` | A scenario's mod could not load. `argus query mods=true` says why. |
| A file under `load_errors` | A test file that does not load. The others still run. |

`argus test --json` gives each run's checks with `actual` and `expected`, its `log` lines, the tick it ended on, the world's `digest`, and the replay's paths. Script errors in the game itself are in `argus script errors`, with file and line.

## The replay a failing run saves

When a run fails, the harness saves a replay of that moment in `.argus/replays/<scenario>/seed<n>/`:

| File | What it is |
|---|---|
| `replay.bin` | A keyframe from before the failure and the input since: enough to play it again. |
| `replay.json` | The run's scene, seed, and code version, the world's digest, and the failure as its note. |
| `frame.png` | The frame at the failure. |
| `clip.png`, `clip-sheet.png`, `clip.wav` | The last two seconds as pictures and sound. |

Only the first five failures of a run get a clip, so a sweep of fifty failing seeds stays quick. When a seed passes again, its old replay is removed.

Open one to stand at the moment it failed:

```sh
argus replay list
argus replay open path=.argus/replays/walks_too_far/seed1
```

Opening loads the scene with the run's seed, restores the keyframe, and plays the seconds before the failure, so the event log and entity traces (`argus events`, and its `trace`) hold them. It stops paused at the failing frame and compares the world with the one the replay saved (`digest.same` says whether they match). From there, look around with `query`, `observe`, and `events`, or `seek` within the lead-in. An agent does the same with the `seek` tool (`replays=true` to list, `replay=<path>` to open). In the editor, the scrubber's Replays menu lists them with their failures, and Open opens one (see [Playtesting](playtesting.md)).

## Determinism: why the same seed gives the same run

A run is a value. The same scene, seed, input, and code reach the same world, bit for bit, on every tick. That is why a scenario never flakes, a mark reproduces, and a failing replay shows the failure again.

It holds because:

- **Time is ticks.** The sim moves in fixed steps (`continuous@60` is 60 a second) or in turns. Scripts read `q.time` and `q.dt`, never a wall clock.
- **Randomness is seeded by name.** Each system that wants random numbers names a stream (`rng = "loot"`), seeded from the run's seed and that name only. Adding or reordering systems never changes another system's numbers.
- **Input arrives as actions.** Gameplay sees the `Actions` component, which the input log records, never a device.
- **Scripts are sandboxed.** There is no `os`, `io`, clock, or device in Luau.

Every run in the report ends with the world's digest, 16 hex digits over every field of every live entity. Two runs with the same digest are the same world. `argus sim status digest=true` gives the digest now.

The promise is for one machine and one build. Measured across Windows, Linux, and Chrome, the four reference games were identical too, but that is not promised: the puppet example (skeletal animation) differs between platforms from its first tick, because the engine's own `f32` math uses each platform's math library.

If the same seed gives two different endings on one machine, that is a bug in the engine or in a script that reaches around the rules. It is worth reporting.

## Testing is how a change is known to be done

In an Argus project, a change is done when its scenarios pass. The `AGENTS.md` that `argus new` writes says so, and `argus new` itself runs a new project's scenarios before it says done. The habit:

1. Write a scenario for the feature or the bug first, and watch it fail.
2. Make the change.
3. Run that scenario, then the whole suite (`argus test`).
4. If the change was meant to change a golden, update it and read the diff.

To check that a scenario really tests something, break the rule it guards on purpose (in a copy) and see it fail. The template authors did this for each template's scenarios.

CI runs each example's suite with `argus test --software`. Your own CI can run the same command and fail on its exit code.

## Limits

- Picture goldens hold on one operating system only.
- `t:run_until` ignores keys it does not know, so a misspelled key is silently ignored.
- Only files directly in `tests/` are scenarios.
- A scenario runs headless. It cannot judge feel; for that, ask a person (see [Playtesting](playtesting.md)).

## Where to go next

- [Playtesting](playtesting.md): playing the game, marks, and the editor's replay scrubber.
- [Tuning and performance](tuning-and-performance.md): sweeps over many seeds, and `perf` checks.
- [Scripting](scripting.md): the Luau the scenarios test.
- [Working with AI agents](agents.md): how an agent uses `test`.
- [Reference](reference.md): the full scenario API is the "Scenarios" part of `argus docs`.
