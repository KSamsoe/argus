# Tuning and performance

This chapter has two halves. Tuning: the fields marked `tune`, the tuning panel, the `tune` methods, and sweeps that try values across many seeds and report which hold. Performance: the `profile` tool, systems' mean and p95, the worst ticks, allocations, captures for Perfetto or Tracy, the performance targets and how they are gated, and when a hot path belongs in a Rust plugin.

## Tuning

### Tune fields

A tune field is a number a person adjusts by feel or for balance: a jump's height, a colonist's hunger, a hit-stop's length. Mark it with `tune = true` and give it a `range`, which the slider uses. From `examples/outpost/components/colony.luau`:

```lua
hunger_per_day = field.f32 { default = 1, range = {0, 10}, tune = true, doc = "Food a colonist needs a day (a meal is 1)" },
eat_below = field.f32 { default = 0.5, range = {0, 1}, tune = true, doc = "A colonist goes to eat once fed drops below this, and eats their fill" },
```

`tune = true` changes nothing while the game runs. It puts the field in the tuning panel and in `tune.fields`, and it makes every change to the field a row in the tuning log.

Def table fields can be tune fields too, as in Outpost's `defs/schemas.luau` (a meal's food, a job's priority). So can many of the engine's own components: `CharacterController`'s `jump_height`, `jump_time`, and `coyote_time`, and the `Camera`'s smoothing and zoom (`argus describe component=CharacterController` shows which). A particle effect's numbers (its rate, speed, size, and so on) are tunable as well.

List them all with their values, defaults, ranges, and docs:

```sh
argus tune fields                    # every tune field
argus tune fields entity=@colony     # one entity's
argus tune fields asset=fx/pop       # one particle effect's knobs
```

The list holds the tune fields of every entity with an id, every def row's (`{def: "item:meal", path: "food"}`), and the effects' knobs. Entities made while the game runs are listed one by one while there are 32 or fewer of a kind (Outpost's job kinds), else as one row with their count, tuned through what they are made from.

### The tuning panel

The editor's Tuning tab has a slider for every tune field, while the game runs. Find filters them. Every move of a slider is applied at once, in the live world, with no file written yet, and logged with its frame. The path you took shows above the sliders. When you are done:

- **Keep** commits the final values as one change by you: the entity's field to its scene file, a def row's to its table, an effect's to its sidecar, comments kept. One undo goes back to where you started.
- **Put back** returns every field to where the session began.

Closing the game's window, or loading a scene, also ends the session and keeps its values.

The phone review page's Tune tab has the same sliders, with an edit token (see [Playtesting](playtesting.md)).

### The same from a shell

The slider is `tune.set`. The first one opens a session; `tune.end` closes it.

```sh
argus tune set entity=@colony path=Colony.hunger_per_day value=1.2
argus run step=60
argus tune set entity=@colony path=Colony.hunger_per_day value=1.4
argus tune log
argus tune end note="a little hungrier"     # keep: one transaction
argus tune end commit=false                 # or put everything back
```

`tune set` takes `def="item:meal" path=food` for a def row, and `asset=fx/pop path="speed[1]"` for an effect's knob. A field that is not a tune field is refused, and so is a value outside its range.

The tuning log (`.argus/tuning.jsonl`) keeps every change with its frame, its author, and the value before and after. Its `story` sums up each field's search path in one line:

```text
human set @colony Colony.hunger_per_day 1→1.2 at frame 0, →1.4 at frame 60
```

The path is information, not only the final value: a value tried and then backed off from says something. An agent reads it with `argus query tuning=true` (or `tuning=Colony.hunger_per_day` for one field), or subscribes to it live (`obs.subscribe` with the `tune` topic).

### Sweeps: trying values across seeds

Feel is tuned by hand. Balance is tuned by hand and checked by machine. A person nudges a value while watching the colony at 10x; then the scenarios check each candidate on many seeds, which no person could do.

Two parameters of `argus test` try values without writing them:

- `set` puts fields into each run's world just after its scene loads.
- `sweep` runs the chosen scenarios once for each value of one field, and reports each candidate.

```sh
argus test name="a week without starving" seeds=1 set='[{entity: "@colony", path: Colony.hunger_per_day, value: 3}]'
argus test name=week seeds=1..2 sweep='{entity: @colony, path: Colony.hunger_per_day, values: [1, 3]}'
```

The sweep's report, on a copy of Outpost:

```text
SWEEP @colony Colony.hunger_per_day
PASS  1        4 runs, 0 failed  (13.9 s)
FAIL  3        4 runs, 2 failed  (14.2 s)  first: a week without starving, saved every day seed 1: ["count {has = [\"Colonist\"]}: 0 == 6 failed","events colonist.died count: 6 == 0 failed","@colony Stockpile.food: 0 >= 6 failed"]
2 candidates, 1 failed, 28.1 s
```

A def row works the same way: `sweep='{def: "item:meal", path: food, values: [1, 0.3]}'`. Nothing is written to any file, whatever the result.

The full loop, as Outpost's phase 4 gate ran it: `Colony.hunger_per_day` was tuned with the slider at 10x, and 1.5 kept. The agent read the path from the tuning log, swept the values on it over Outpost's 20-seed, 30-day food scenario (`tests/outpost_food.luau`), and reported which starve: 1.5 starved one valley in twenty and left the rest short of food, and 2.0 starved all twenty. When Kevin tuned it by hand, the sweep of his path found 1.0, 1.2, and 1.3 fed every valley. Outpost stayed at 1.0.

Sweeps take time: each value runs every chosen scenario on every seed. The phase 4 gate's sweep (four values, twenty seeds, thirty days each) took 21 minutes in a release build. Start with a few seeds and a short scenario.

## Performance

### The profile tool

`argus profile` (the `profile` tool for an agent) answers how fast the game runs, as data. Run the game for a while first (`argus run step=600`, or play it), since the numbers cover recent ticks.

| Command | What it gives |
|---|---|
| `argus profile` | The performance targets beside what is measured now (below). |
| `argus profile systems=true` | Each system's mean, p95, and worst milliseconds, in stage order, with its allocations. |
| `argus profile worst=5` | The five slowest recent ticks, each with every system's time and the entity counts by archetype. |
| `argus profile calls=true` | How long protocol calls took: seeks, reloads, queries. |
| `argus profile capture=300` | Record the next 300 ticks as a trace file. |
| `last=n` | With any of these: how many ticks to cover (600 by default). |

The editor's Profile tab shows the same: the tick against the game's budget, the targets, every system, the slowest ticks (Go there seeks to one), and captures.

Lower-level methods are there too: `argus prof frame` (the last tick), `argus prof systems series=true` (each tick's time, to draw), `argus prof alloc`, and `argus prof archetypes` (entities and bytes by component set).

### Reading the numbers

- **The tick's p95** is the number that matters most: 95 ticks in 100 are at least this fast. A 60 Hz game has 16.6 ms a frame for everything, the drawing included.
- **The heaviest system** is named in the answer. Start there.
- **Mean against p95 against max.** A system whose max is far above its p95 has spikes: look at `worst` to see which ticks.
- **Allocations.** `profile systems=true` gives each system's allocations per tick. A Luau system that builds a table for every entity shows up here, and is usually the first thing to fix.
- **Debug or release.** The answer's `build` says which kind of daemon measured. The targets are release numbers; a debug build runs slower.

### Captures for Perfetto or Tracy

```sh
argus profile capture=300
argus run step=300
argus events name=prof.capture
```

A capture is armed, and written once the ticks have run (play the game, or step it). It goes to `.argus/captures/<time>-tick<n>.json`, and a `prof.capture` event names the file. It is the Chrome trace event format: every system's span on one time base, with its allocations. Open it in Perfetto (ui.perfetto.dev, Open trace file), or convert it for Tracy with `tracy-import-chrome`.

### The performance targets

The design sets 23 performance targets, and `argus profile` (the `prof.targets` method) shows each beside what this daemon measures. A trimmed example from Hollow:

```text
game     measure                         target               ok     system        text
-------  ------------------------------  -------------------  -----  ------------  -------------------------------
hollow   Animators evaluated per tick    2,000 at < 0.3 ms    true   anim.update   0.003 ms p95 over 600 ticks
hollow   Room stream-in                  < 4 ms               true   rooms.stream  0.014 ms at worst over 600 ticks
hollow   Cold scene open (index warm)    < 300 ms             false                1183 ms the last session.open
all      Full snapshot at 100k entities  < 5 ms               true                 0.209 ms the last keyframe
all      Seek to any point in last 5 min < 500 ms                                  no sim.seek yet
```

Each row has the target in the design's words, why it matters, and `gate`: the test that enforces it. Rows marked with a reference game (Swarm, Delve, Hollow, Outpost) are measured only when that game is open, matched by its `argus.toml` name. Rows marked `all` apply to every game. A row over its bound has `ok` false and names the system to look at; `over` counts them. (The cold-open row above was over because the copy's index was cold: it had just been made.)

Your own game gets the `all` rows. The engine's measured rows are not a budget for your game; set one yourself (below).

### How the targets are gated

The targets are the engine's, and the engine's CI checks them on the reference laptop: its perf step runs every timing test in a release build, one at a time, and its games step runs each example's suite with `argus test --software`. A target with no test is a target nobody checks, so each row names its test.

For your game's own budget, put the bound in a scenario. Hollow's run scenario ends with:

```lua
t.expect:perf("sim"):p95_ms():lt(4.0)
```

Swarm's stress scenario checks a closed pause menu stays cheap by reading `prof.systems` itself (`examples/swarm/tests/stress.luau`). Then `argus test` in your own CI gates it, with `perf_margin` (1.5 by default) as slack. Timing tests want a quiet machine: one busy with a build reads slow, so run a failure again before hunting for a slowdown.

## When to move a hot path into a Rust plugin

Most of a game should stay Luau: it reloads in milliseconds, it is sandboxed, and it runs everywhere, the web included. A Rust plugin is for a handful of hot systems that rarely change.

Move a system to Rust when all of these hold:

1. `profile systems=true` shows it is most of the tick at the entity counts the game really has.
2. Its work is per-entity arithmetic over thousands of entities.
3. You have already removed its allocations (no table per entity) and it is still too slow.

Swarm is the example. Its stress scene has 50,000 enemies. With the chase in Luau, a tick took 40.6 ms at p95, 37 ms of it the chase. With `sim.chase` moved to `plugins/swarm-hot`, the chase takes 0.15 ms and the tick 2.5 ms, under its 6 ms target. The system kept its name, stage, order, reads, and writes, and the world it makes is the same, bit for bit, every tick: nothing around it changed.

What a plugin costs:

| Cost | Why |
|---|---|
| Rust on every machine that builds the game | The daemon builds the plugin with cargo when the project opens and on `reload`; `argus build` builds it too. Players need no Rust. |
| No web build | A page cannot load a library. |
| Released on its own platform only | A Windows build cannot make the Linux plugin. |
| No mods can carry one | Mods are Luau and data only. |
| Trusted code | A plugin runs outside the Luau sandbox. |

Luau systems all run on one thread. A plugin system can say `.parallel()`, and then runs beside other parallel systems it does not conflict with, on other threads, with the same world as a result.

How to write one, its files, and how it ships are in [Releasing](releasing.md#rust-plugins).

## Limits

- Ending a tuning session (Keep, or `tune end`) writes `f32` values as the world holds them, so 1.2 is written as `1.2000000476837158`. The value is right; edit the file if you want the short form.
- The performance targets for the reference games apply only to them. Your game has no targets of its own until you write `perf` checks.
- Timing depends on the machine and how busy it is.

## Where to go next

- [Testing](testing.md): scenarios, seeds, and `perf` checks.
- [Playtesting](playtesting.md): the editor, and the person's side of tuning.
- [Releasing](releasing.md): Rust plugins in full, and release builds.
- [Scripting](scripting.md): declaring fields, ranges, and `tune`.
- [Reference](reference.md): `tune.*`, `prof.*`, and `test.run` in `argus docs`.
