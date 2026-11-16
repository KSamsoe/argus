# Phase 0 gate

The plan (design §18) ends phase 0 with a gate rather than an item:

> From an empty directory and a paragraph brief each, an agent using only the CLI builds headless cores of Swarm (continuous clock, 50k entities, waves) and Delve (discrete clock, generated floors, turn order) with passing tests, and can answer "why did the player die" in both from events and traces. If either game fights the protocol, that finding is the phase's output.

This file is the record of that run, done on 2026-09-18 with the phase-0 engine at commit `67956b0` plus the two example projects this commit adds. The agent wrote TOML and Luau files and drove the engine only through `argus`; it never touched Rust, the daemon's socket, or the index.

## The briefs

**Swarm.** A bullet heaven. One player walks around an open field and fires without aiming. Waves of enemies spawn on a ring around the player and walk straight at it; touching the player hurts, and the player's bullets kill them. A wave is cleared when nothing is left, and the next one is bigger. The core should run headless with tens of thousands of enemies alive and tell you what happened when the player dies.

**Delve.** A grid roguelike. A floor is rooms joined by corridors, generated from the run's seed. The hero moves one cell per turn or attacks the monster in the way; then every monster acts in initiative order, stepping toward the hero or striking when adjacent. Stairs in the last room lead to the next floor with more monsters. Nothing happens between turns. Sweeps over hundreds of seeds should shake out floors with unreachable stairs or monsters spawned on top of the hero.

## What was built

Both projects are in [examples/](../examples/). Each is `argus.toml`, `input.toml`, a `components/` file, a few `systems/` files, one or more scenes, and `tests/*.luau`.

| | Swarm ([examples/swarm](../examples/swarm)) | Delve ([examples/delve](../examples/delve)) |
|---|---|---|
| Clock | continuous at 60 Hz | discrete, one turn per `sim.advance` |
| Authored entities | `player`, `director` | `player`, `floor`, `turn` |
| Components | Health, Player, Enemy, Bullet, Velocity, Director | Health, Player, Monster, Stairs, Floor, Turn |
| Systems | `sim.move`, `sim.waves`, `sim.chase`, `sim.fire`, `sim.bullets`, `sim.hits` | `sim.floor`, `sim.player_turn`, `sim.monsters` |
| Runtime entities | enemies and bullets, 50,000 alive in the stress scene | monsters and stairs, regenerated per floor |
| Randomness | `waves` stream for spawn positions | `floor` stream for rooms, corridors, monsters |
| Scenes | `arena`, `last_stand`, `stress` | `delve` |
| Tests | 3 scenarios, 5 runs | 3 scenarios, 56 runs (200 with `--seeds 1..200`) |

Delve keeps the floor as a `Floor` component whose `map` field is one character per cell, because systems have no globals to hide state in (§7) and the whole game state has to be rows in typed columns (§3). That turned out to be the right shape: `argus world get @floor` shows the map, and a snapshot golden carries it.

## The tests

```
$ argus --project examples/swarm test
PASS  a standing player dies and the log says why  (last_stand, 3 seeds 1..3: 3 ok, 0 failed, 3483 ms)
PASS  fifty thousand enemies tick  (stress, seed 1, 8030 ms)
PASS  first wave clears within 20s  (arena, seed 7, 692 ms)
3 scenarios, 5 runs, 0 failed, 12204 ms

$ argus --project examples/delve test
PASS  a hero who waits is killed and the log says by whom  (delve, 5 seeds 1..5: 5 ok, 0 failed, 158 ms)
PASS  walking at the nearest monster attacks it  (delve, seed 4, 37 ms)
PASS  stairs are always reachable on floor 1  (delve, 50 seeds 1..50: 50 ok, 0 failed, 327 ms)
3 scenarios, 56 runs, 0 failed, 524 ms

$ argus --project examples/delve test stairs --seeds 1..200
PASS  stairs are always reachable on floor 1  (delve, 200 seeds 1..200: 200 ok, 0 failed, 1232 ms)
```

The design's `tests/delve_stairs.luau` runs as written apart from the `path` expectation being room-graph reachability in phase 0 (see the runner's trace). Two hundred generated floors in 1.2 s is inside the §17 line for seed sweeps.

## Why did the player die

Both answers came from `obs.events` and `world.who_writes`, with no trace facility, which is what the daemon's trace records as the phase-0 decision. The sessions below are abridged from the CLI's own output; `--json` gives the same rows to a program.

**Swarm.** The `last_stand` scene has a player that cannot move and fires slowly, against a wave of 400.

```
$ argus scene load last_stand
$ argus sim run_until event=player.died max_ticks=3000
  tick 599, hit: true

$ argus obs events name=player.died
  {"enemies_alive":235,"kills":165,"last_hit_by":"e:318","tick":598,"wave":1}

$ argus obs events name=damage entity=@player
  {"amount":1,"by":"e:316","crowd":1,"tick":545,"wave":1}
  {"amount":3,"by":"e:353","crowd":3,"tick":556,"wave":1}
  {"amount":1,"by":"e:130","crowd":1,"tick":576,"wave":1}
  {"amount":3,"by":"e:22","crowd":9,"tick":587,"wave":1}
  {"amount":3,"by":"e:318","crowd":4,"tick":598,"wave":1}

$ argus world who_writes Health
  systems: sim.waves, sim.hits
```

The answer: the player killed 165 of 400 enemies, the crowd reached it at tick 545, and five hits in 53 ticks took its ten hit points, the last three from crowds of three, nine, and four. The only systems that can change `Health` are the spawner and `sim.hits`, so the damage came from `sim.hits`, and the events name the enemy each time.

**Delve.** Seed 2, and the hero issues no commands.

```
$ argus scene load delve seed=2
$ argus sim advance n=1
$ argus obs events name=floor.generated
  {"floor":1,"monsters":6,"rooms":6,"tries":8}
$ argus world query select=[id,Monster.kind,Monster.initiative,Health.cur,Transform.pos] where.has=[Monster] order=dist(@player)
  e:9  bat     4  3  [15,10]
  e:4  goblin  9  6  [18,12]
  e:8  rat     3  3  [12,4]
  ...

$ argus sim advance n=200
$ argus obs events name=player.died
  {"floor":1,"killer":"e:8","kills":0,"kind":"rat","turn":27}
$ argus obs events name=damage entity=@player
  {"amount":1,"by":"e:8","kind":"rat","turn":18}
  ... ten rows, one per turn, all e:8 ...
  {"amount":1,"by":"e:8","kind":"rat","turn":27}
$ argus world who_writes Health
  systems: sim.floor, sim.player_turn, sim.monsters
```

The answer: a rat reached the hero on turn 18 and bit it once a turn for ten turns while the hero waited; nothing else got adjacent, because the corridor into the hero's room is one cell wide and the rat was in it. The hero never attacked (`kills: 0`).

## Did either game fight the protocol

Neither game needed anything the protocol did not have. The scenarios in the design ran in the shape they were written. Three things are worth recording, one of them a real limit.

**1. Luau at 50,000 entities is the ceiling §19 warned about.** The stress scene keeps 50,000 enemies alive with all gameplay in Luau. It runs, the test passes, and the profiler says exactly what it costs:

```
$ argus scene load stress
$ argus sim step n=30                       7.9 s wall for 30 ticks
$ argus prof systems last=30
  tick_p95_ms: 279.74
  sim.chase   mean 79.2 ms   p95  88.4 ms
  sim.hits    mean 164.1 ms  p95 176.3 ms
$ argus prof alloc last=30
  tick_mean_allocs: 997,091   tick_mean_bytes: 38.3 MB
  sim.chase   200,914 allocs/tick
  sim.hits    750,759 allocs/tick
```

A tick that should take under 6 ms (§17) takes 265 ms, and `prof.alloc` shows why in one screen: four allocations per enemy in the chase system and fifteen in the hit system, every one a Lua table for a `vec2` read or write. The rules the design already states cover this: Luau systems declare their write sets, so `sim.chase` and `sim.hits` can be promoted to Rust hot-path systems without changing anything around them (§19, "a migration, not a switch"). Two cheaper steps come first and belong to phase 1: return `vec2` fields as a reused value type instead of a fresh table, and give the binding a spatial query so `sim.hits` stops building its own hash. None of this changes the protocol; the finding is about the binding's cost model, and the profiler found it as a query result with a system name on it, which is what §5 promised.

At the sizes the other scenes use (a few hundred to a few thousand enemies) the same code runs at 1 to 3 ms a tick, and the arena scenario clears its first wave in 0.7 s of wall time for 2.7 s of sim.

**2. Spawning 50,000 entities from Luau costs 279 ms.** One `insert` per component per entity, each a call across the binding. A batch spawn on the query handle (`q:spawn_many(n, components)`) is a small addition to the binding for phase 1. The daemon's own scene load does the same work in Rust in under a millisecond per thousand.

**3. Two conveniences the harness could add.** Delve's fight scenario asks the daemon where the nearest monster is with `world.query` ordered by `dist(@player)` and steers with `t.input:tap`; it reads well, but a `t:nearest(where)` helper would save four lines. The Swarm scenarios all want "run until this entity's health is zero", which is `run_until { event }` on an event the game emits for the purpose; a `run_until { field = ..., eq = 0 }` form would remove that indirection. Neither blocked anything.

What did not come up at all: the write-set declarations never got in the way once a system listed what it touched, the discrete clock did exactly what Delve needed with `sim.advance` as the turn, seed sweeps found no bad floors in 200 tries, and every question about a run had an answer in the event log. The phase's output is finding 1.

## Numbers

| Measure | Value |
|---|---|
| Swarm arena, wave 1 (200 enemies) cleared | 2.7 s of sim in 0.7 s of wall time |
| Swarm arena tick p95 at a few hundred enemies | 2.6 ms |
| Swarm stress, 50,000 enemies, tick p95 | 280 ms (Luau; see finding 1) |
| Swarm stress, allocations per tick | 997,000 |
| Delve floor generation | under 1 ms, 6 rooms, 6 monsters |
| Delve 200-seed sweep of floor 1 | 1.2 s |
| Delve 120-turn run to the hero's death | 30 ms per seed |
