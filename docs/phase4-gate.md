# Phase 4 gate

The plan (design §18) ends phase 4 with a gate:

> Everything the editor does appears in the change feed as a command the agent could have issued. A human tunes Outpost's food economy at 10× with sliders; the agent sees the slider path as data, sweeps the candidates, and reports which ones starve.

This file records that run, done on 2026-10-01 with the engine at the commit that adds it. Both parts are met, each by a test that drives the editor headless as a person would, clicking by name and dragging with the pointer. What is left for a person is Kevin's own session: tune Outpost at 10x by hand, then ask the agent to sweep the path.

## The parts

| Part | Where it stands | Where to look |
|---|---|---|
| Everything the editor does appears in the change feed as a command the agent could have issued | Met. One session drives every panel while the agent works beside the person. The feed's 53 rows, 45 of them the person's, typed into the CLI against a fresh daemon on a fresh copy of the project, make the same world: the same digest, clock, and world in use, and every file the same byte for byte | `crates/argus-editor/tests/gate.rs` |
| A human tunes Outpost's food economy at 10× with sliders | Met headless: the colony's hunger dragged three times while Outpost runs at 10x, then kept. Kevin's own session is the hand check | `crates/argus-editor/tests/gate_outpost.rs` |
| The agent sees the slider path as data | Its `query` tool reads the tuning log: the path in a line, and every move with its frame | the same test |
| It sweeps the candidates and reports which ones starve | Where the session began and each value the person let go at, each on §9's scenario for twenty seeds of thirty days. Two of the four starve (below) | the same test |
| Every earlier gate's tests still pass | Yes (below) | |

## Every panel, one feed

The project is the test fixture with something for each panel: a lamp whose light is a tune field and a timeline about it, a hero with an animator, a tileset, and a door with a box to tick. In one session:

| Panel | What the person did |
|---|---|
| The bar | a step; 3x and back to 1x |
| Hierarchy and inspector | clicked the door, ticked its box, undid and redid it; locked and unlocked the player; added a light to the player |
| Tuning | dragged the lamp's light up and kept it, with a note |
| Agent panel | answered the agent's question with a key; tried its proposal, looked at A, then B, and kept B |
| Viewport | played, clicked into the game, pressed F8, typed a note, pressed Enter; Stop, back where play began |
| Agent panel again | set the mark's note right and closed it |
| Scrubber | dragged back to frame 120; tried a world from there and stepped it; back to the run; opened a failing test's replay from the list |
| Profiler | went to a slow tick |
| Tiles | painted the agent's map with the pencil; made a new map |
| Animator | added an event to the hero's run |
| Timeline | played the show with the lamp as its actor, dragged a cue sooner, added a cue at the playhead, scrubbed back along the ruler |
| Change feed | read it |

Beside the person the agent walked the player, asked a question with a clip, proposed a tougher player, saved a replay, and made and placed the map.

Every row of the feed, all authors in order, was then run through `argus_cli::run` against a fresh daemon on a fresh copy. Each command succeeded, and the two worlds were compared: `sim.status {digest: true}` (the digest a mark records, every field of every live entity), the tick, frame, speed, play state, and world in use, and every file of the project but `.argus`.

## The path, the sweep, the report

The person, driven headless: Outpost playing at 10x, the Tuning tab, the colony's hunger (food a colonist needs a day, 1 by default) dragged to 1.3, a pause, to 2.0, a pause, back to 1.5, then Keep with a note. The agent, through its own tools (`argus_cli::tools`, which `argus mcp` serves), read the path. Its `query {tuning: "Colony.hunger_per_day"}` answers with every move and its frame, and the story in a line:

```
human set @colony Colony.hunger_per_day 1→1.1 at frame 271, →1.2 at frame 291, →1.3 at frame 331,
  →1.5 at frame 951, →1.6 at frame 991, →1.7 at frame 1001, →1.8 at frame 1011, →2 at frame 1031,
  →1.8 at frame 1701, →1.7 at frame 1721, →1.6 at frame 1751, →1.5 at frame 1761
```

It took where the session began and each value the person let go at (a move with half a second of play after it, and the last): 1.0, 1.3, 2.0, and 1.5. It swept them with its `test` tool, `sweep={entity: @colony, path: Colony.hunger_per_day, values: [1, 1.3, 1.5, 2]}` on §9's scenario (a six-colonist start, thirty days, twenty seeds), each candidate tried in the run's world without writing it. Its report:

| Hunger a day | Runs | Starve | Short of food only | Seeds that starve |
|---|---|---|---|---|
| 1.0, where it began | 20 | 0 | 0 | |
| 1.3 | 20 | 0 | 5 | |
| 1.5, what the person kept | 20 | 1 | 19 | 15 |
| 2.0 | 20 | 20 | 0 | all |

"Starve" is a run in which a colonist died of hunger; "short of food only" is one that ended its thirty days under the scenario's sixty meals with nobody lost. So the value the person kept starves the colony on one seed in twenty and leaves the other nineteen short, which is what a person watching one valley at 10x could not have seen. The sweep took 21 minutes in a release build, 80 runs of thirty days.

## What the gate found

Building the gate found seven faults, all fixed, each with a test.

**1. Keeping a proposal erased the run before it.** After a trial the game goes on in the world picked (phase 2's decision, so the person keeps what they were watching), and that world was forked when the trial began, with a ring of its own. Its run began at the trial: the scrubber could not go back past it, and the event log had started again from one, so a client reading it since its last row saw nothing. The picked world now takes on the run's keyframes from before it forked, and its events, its own numbered on from the run's.

**2. A scene load emptied the change feed.** Opening a replay from the scrubber (or a mark, or loading a scene) reset the session's editing state, the feed included, so the feed no longer held every change of the session and could not make the same world. The feed stays across a load now and its ids count on. Undo, open transactions, and locks still go with the world they were about.

**3. The profiler offered ticks of a run that was gone.** Its slowest ticks include ones from a span Stop took back, and "Go there" asked for a frame past the run's end, which the daemon refused. It is offered only inside the run's reach now, and says why otherwise.

**4. The agent could not read the tuning log.** §10's agent "sees the search path", but no tool called `tune.log`. `query {tuning}` does.

**5. A sweep did not say why a run failed.** It gave each failing run's script error, which is empty when expectations fail, so the agent could not tell a colonist starving from a stock running low. Each failing run lists its failed expectations in words now.

**6. Two worlds could not be compared through the protocol.** Marks record the world's digest, but no call answered with it. `sim.status {digest: true}` does.

**7. Three lists had no name.** The hierarchy's two filters and the inspector's "add a component" list had empty accessible names, so a screen reader, and a test, could not find them. They are named for what they do.

The gate's own drag of the slider was off at first: egui's slider track is its rectangle less the knob's radius at each end, and pressing at the rectangle's share of the value moved the value before the drag began. The test drags by the track now and checks the slider is where it let go.

## Did it fight the protocol

No. The person's side is the editor, which has only the protocol; the agent's side is its tools; and the comparison is protocol calls. The gaps were missing answers, a digest and the tuning log for the agent, and a sweep's reasons, each one call or one field.

Limits it met, stated so they are decisions:

- **Keys in the viewport are not feed rows.** They are the game's input, in the input log (phase 4 item 1); what F8, F9, and F10 stand for is a row by the person. A play span with keys in it ends with Stop for the feed to make the same world, or its input travels as a replay.
- **A candidate is where the person let go.** The agent reads a move followed by half a second of play as one; a person who drags without stopping gives one candidate, the last. The values in between are on the path for an agent that wants them.
- **A sweep is minutes a candidate.** Twenty seeds of thirty days is about five minutes in a release build on this machine, one seed after another in one daemon. The test runs by hand at the gate (`--ignored`); the debug suite runs the same scenario once a run.

## Numbers and tests

The Outpost half, in a release build (`cargo test -p argus-editor --release --test gate_outpost -- --ignored --nocapture`): the sweep above, 80 runs in 21.0 minutes, all four candidates with their twenty runs.

Every earlier gate's tests, on this machine:

| Tests | Result |
|---|---|
| Workspace, debug | 468 pass, none fail; the 35 release-only tests skipped |
| Release-only tests (`cargo test --workspace --release -- --ignored --test-threads 1`, the Outpost sweep above apart) | 34 pass, none fail |
| Outpost | 12 scenarios, 35 runs, all pass in 596 s |
| Hollow | 17 scenarios, all pass in 16 s |
| Delve, Swarm | Delve's 5 scenarios, 208 runs, and Swarm's 3, 5 runs, all pass |

Among the release-only tests, §17's rows the earlier gates measured, now:

| §17 row | Now | At the phase 3 gate | Target |
|---|---|---|---|
| Hollow, 201 rooms: cold scene open, the index warm | 238 to 255 ms (five opens) | 226 to 238 ms | under 300 ms |
| Outpost: sim tick at 10x, 200 colonists, 20k items | p95 1.081 ms closed, 1.120 ms with the inspector open | 0.974 and 1.037 ms | under 1.6 ms |
| Outpost: headless fast-forward | 6,771 ticks a second (30 days in 15.9 s) | 6,636 | over 2,000 |
| Outpost: save and load at 8 h | 63.4 ms and 232.6 ms | 66.5 and 223.7 ms | under 200 and 500 ms |
| Outpost: the first week's save | loaded in 33.2 ms through 4 migrations | 35.9 ms | under 500 ms |
| UI tree, 2,000 nodes with text | 0.991 ms mean (1.340 worst) | 0.909 (1.075) | under 1 ms |
| Seek anywhere in the last five minutes (50,000 entities) | worst 47.3 ms; a keyframe 1.57 ms | | under 500 ms; a keyframe under 5 ms at 100k |

The UI row is the closest to its bound and worth watching.
