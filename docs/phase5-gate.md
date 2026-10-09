# Phase 5 gate

The plan (design §18) ends phase 5 with a gate:

> All four reference games ship as example projects, each with its test suite green in CI on the reference laptop, and an agent given any one of them can add a feature end to end without reading engine source.

This file records that run, done on 2026-10-09 with the engine at the commit that adds it. Both parts are met.

The second part was tested directly. Four agents (Opus 5.5, Kevin's choice), one for each game, were each given a copy of the game, the `argus` tool, and the docs those give. Each was asked to add a feature and was not allowed to read the engine's source. All four built their feature, proved it with new scenarios, and finished with the whole suite passing. I checked each one afterwards: their transcripts, a rerun of their suites, and their changes.

The agents also reported what was missing. Every one of them said the same thing first: nothing described the scripting API or the scenario API, so they learned both from the games' own files. Both are documented now, and so are 20 other gaps they named. Two of them found real bugs: one in Hollow, and one in a performance gate that was measuring the wrong part of Swarm. Both are fixed.

## The parts

| Part | Where it stands | Where to look |
|---|---|---|
| All four reference games ship as example projects | Met. Each builds as a Windows release, and `argus build` checks it plays and looks as the project does: the same world after 300 ticks, drawn the same. Hollow, Delve, and Outpost also build for the web; Swarm cannot, as its chase is a Rust plugin (phase 5 item 6) | `examples/`, below |
| Each with its test suite green in CI on the reference laptop | Met: CI's games step runs every scenario of the four games, and toybox and puppet | `tools/ci/ci.sh`, below |
| An agent given any one of them can add a feature end to end without reading engine source | Met, four times: below | `docs/phase5-gate/<game>.diff` |
| Every earlier gate's tests still pass | Yes, below | |

## The four trials

Each agent worked alone in a copy of its game, outside the repository, with the release `argus` built from the commit before. The brief:

- Read and change only files inside the project, and run the engine only through the tool. Read nothing of the engine's source: not the repository, not any `crates/` folder, not the plugin SDK's source the engine writes into `.argus/plugins/sdk`.
- Start with `argus docs --for-agent`, which writes `ARGUS.md`. Use `argus docs` and `argus help`. When the docs do not say something, find out with the tool and note the gap.
- Headless only, no downloads.
- Done means the feature works as a player meets it, at least one new scenario proves it, the whole suite passes, and `ARGUS.md` is regenerated. Then report what was built, how each thing was learned, and every gap.

| Game | The feature | What the agent built | Its scenarios | The suite at the end |
|---|---|---|---|---|
| Hollow | A feather that grants a double jump | A pickup on the plank over the hermit's fire, glowing in the dark. A system between the controller and the move turns a mid-air press into one more jump, back on landing. The run owns the feather, so a fall keeps it and a new run puts it back. Its words, HUD line, two sounds, a sprite, and French strings | Without the feather a mid-air press does nothing; with it the second jump happens once, carries the hero 6.34 up against 3.20, and comes back after landing; a fall keeps it, a new run does not | 25 scenarios, 29 runs, 0 failed |
| Delve | Healing potions | A drop rolled on its own named stream ("loot") when the hero kills a monster, so a seed always gives the same drops and the floor's own stream is untouched. Picking up on walking over. A new `drink` action on Q that heals by a set amount and takes the turn (or, with no potion, only says so). The HUD count, five message lines, and a Drink row on the Controls page | Over six seeds: drops match kills, pickups match drops, a seek back replays the same drops; drinking heals and takes a turn, never overheals, and with none does nothing but say so | 10 scenarios, 223 runs, 0 failed |
| Swarm | A dasher enemy | From wave 2, a tenth of each wave. A state machine in the game's Rust plugin (walk, wind up swelling yellow, charge red along a line fixed when the wind-up began, recover), the chase leaving held enemies alone. It first wrote the dasher in Luau, found with `argus profile` that 5,000 of them broke the stress target, and moved it to Rust: 0.019 ms a tick | Dashers from their wave on; every charge 40 ticks after its wind-up, on its line within 1e-3, at least five times walking speed, ending 3 units past the mark; a player who steps aside is missed and one who stands is hit | 10 scenarios, 23 runs, 0 failed |
| Outpost | Woodcutting | Trees in the valley, a chop job colonists with the skill take below every food job, logs hauled to the stockpile, wood on the HUD and in the inspector. Felled trees regrow over three days of daylight. Colony and Stockpile went up a version with migrations, so the first week's save still loads | Chopping within the first day on ten seeds, wood stored and shown; a stump grows back and is felled again; the inspector's counts; an old valley gets its trees and woodcutters | 24 scenarios, 58 runs, 0 failed, the 30-day 20-seed food scenario among them |

What it took:

| Game | Lines added | Files | Tool calls | Time |
|---|---|---|---|---|
| Hollow | 335, a sprite, and four pictures re-recorded (two show the feather; `update` rewrote the other two unchanged to the eye, which it no longer does) | 11 | 99 | 19 minutes |
| Delve | 459 | 18 | 92 | 17 minutes |
| Swarm | 426 | 5 | 98 | 23 minutes |
| Outpost | 475 | 13 | 126 | 57 minutes, ten of them for each run of the suite |

Two agents checked their own scenarios could fail. Hollow's ran them with the extra jump's height set low through `test set`, and Swarm's broke the charge to re-aim every tick. Both saw the scenarios fail. Swarm's agent also found that dashers died before finishing a charge, and raised their health so the charge matters.

### How they were checked

- **Their transcripts.** Every tool call the four agents made, 415 in all, was read by a script for any path outside its project or any engine source, and every flagged call was read by hand. None read engine source. What touched the outside were the agents' own command outputs: Hollow read two long outputs of its own commands that the harness had saved elsewhere, Swarm kept a probe script one folder above its project (it only called the tool), and Outpost wrote its full-suite output to the same folder. Each said so in its report.
- **Their suites.** I ran each game's whole suite again in its copy: the same lines as above.
- **Their changes.** They are in `docs/phase5-gate/<game>.diff`, against the games as committed (pictures left out). They are not in the games: whether any of them should be is Kevin's call.

## What the agents needed, and what changed

What each agent reported missing or wrong, and what was done:

| Found by | What | Now |
|---|---|---|
| all four | No reference for the scripting API: declarations, `q`, entities, random streams, tile maps | `argus docs` and every ARGUS.md end with it (`argus_luau::api`). A test reads the binding and fails on a method the reference lacks |
| all four | No reference for the scenario API: `scenario`, `t`, `t.input`, `t.expect`, comparisons | The same (`argus_test::api`), checked against the harness the same way |
| Hollow | ARGUS.md said "Declared in `crates\...`" for the engine's components | Not any more. It says they are the engine's, and where their methods are documented |
| Swarm | No reference for the plugin SDK | ARGUS.md points at the SDK's own docs: `cargo doc` in `.argus/plugins/sdk`, which has no dependencies and works offline |
| Delve, Hollow | `rect={x:0,y:0,w:72,h:24}` read as one string, though the help says `{k: v}` takes bare words | An object's key ends at its colon however it is glued |
| Hollow, Swarm, Outpost | `argus test --params '{...}'` dropped the params without a word (Outpost's ran a ten-minute suite by it) | `argus test` reads `--params` as every command does |
| Delve | A whole-number vec2 in a query's `where` matched nothing | Numbers compare by value at any depth |
| Hollow | A system's unknown key (`before`, a misspelling) loaded silently | Refused with its file and line; `before` says what to do instead |
| Hollow, Outpost | A reload's ordering error was in its answer but not in `script.errors`; fixing a component left the 16 systems that had failed because of it listed | The ordering error stays in `script.errors` until a reload fixes it, and files with errors are tried again whenever anything reloads |
| Delve, Swarm, Outpost | A changed component on reload: "Bump the version and add a migration", when the way on was `scene load` | The message says `scene load`, and when a version and migration are needed. The reference gives the migration's forms |
| Delve | `obs.frame`'s docs said a HUD is in `hud`; a HUD of UI nodes is in `ui` | Documented |
| Delve | Keyboard names were nowhere, and any name was accepted | Documented in `input.bind`; open-ended by design (a test holds it), so the doc says an unknown name never fires |
| Delve | A key pressed and released in one `input.device` call is never seen on a discrete clock | Documented |
| Hollow | `test update=true` rewrote every golden of the scenario | Only those that no longer match |
| Delve | Replays of failures stayed after the scenario passed | A seed that passes drops its old replay |
| Swarm | A plugin's compile error kept only rustc's headline ("mismatched types") | The row carries what rustc said at the place, and its notes |
| Swarm | The `events` tool had no `from_frame` | It has, and `cursor` |
| Swarm | `events entity=` of a dead entity was refused; `trace` of one said only "not alive" | Events of a gone entity are found by its old handle; `trace` says it was never traced and how to trace one |
| Hollow | The character controller's behaviour was undocumented: it learned buffering, take-off speed, and the cut with a probe of every tick | CharacterController's doc says how a jump works and where a system can change it |
| Hollow | Whether `q:events` is this tick's or everything since the last run | The reference says: this tick's |
| Hollow | UI text with a `max` width kept one line's height and spilled below its panel (Hollow's shaft sign too) | Words wrap at the node's `max` width |
| Outpost | Event fields are undocumented | The reference points at `argus events name=... limit=1`, which shows any event's fields; a list of the engine's events is left for later |

Left as they are, and recorded: moving a system from Luau into a plugin in one reload fails (a scene load does it), and the SDK's string type has no documented way to make one from Luau's side. Outpost's agent wished for a script-side "is this job reachable", and for a sweep that reports a number (food on day 30), not only pass or fail.

## What the gate found

- **Hollow's gem stayed taken after a new run.** The agent who added the feather noticed the gem's restart only reset gems whose rooms were in. A restart made deep in the cave, with the shaft streamed out, brought the shaft back with its gem remembered as taken, in a run that had not taken it. It was confirmed with a scenario. Now a gem remembers which run took it (`Gem.taken_in`, `Run.number`), and one taken in an earlier run is back; `tests/gem_restart.luau` holds it.
- **Swarm's stress target was measured before the stress.** §17's "Swarm stress tick under 6 ms" was measured over ticks 30 to 150, before the swarm reaches the hero. The agent who added the dasher found the game at 8.3 ms once it did; measured again here, 12 to 13 ms at the 95th percentile. The cost was the game's music, counting every enemy within 15 units of the hero to set an intensity that is full at 30, through `q:within`, which sorted all of them first. The music stops counting at 30, and `q:within` now orders its results as the loop goes, so a loop that stops early pays for what it takes. The music went from 10.7 ms a tick to 0.47, and the tick with the swarm on the hero is 1.6 ms. The test measures that window too now.
- **The index's timing gate was flaky.** Phase 5 item 11's CI failed it twice in three runs: a cold first re-index after a build took 25 ms against 20, while a warm one takes 15. It takes the best of three runs now. The work is the same each time.

## Did it fight the protocol

No. All four features were data and scripts (and, for Swarm, Rust in the game's own plugin), built with the commands an agent already has. None needed a change to the engine. The observation tools did the finding out: pictures (`obs frame`), event logs and field traces (`events`), the profile, queries, test sweeps, and seeks. Swarm's agent read the profile and the §17 targets, found its first dasher too slow, moved it into the plugin, and checked both versions reached the same game. Outpost's agent swept `chop_open` from 0 to 6 over the food scenario to show woodcutting never starves the colony.

What was missing was words, not tools. The scripting and scenario APIs existed only as the games' own code, which an agent can read but cannot be sure is the whole story.

## Numbers and tests

Every earlier gate's tests, on this machine: CI's six steps (`tools/ci/ci.sh`) at the commit that adds this file, all passed.

| Tests | Result |
|---|---|
| Formatting and clippy | clean, no warnings |
| Workspace, debug | 685 pass, none fail; the 46 release-only tests skipped |
| Release-only tests (`cargo test --workspace --release -- --ignored --test-threads 1`, the Outpost sweep below apart) | 42 pass, none fail |
| Hollow | 24 scenarios, all pass (the gem scenario is new) |
| Delve | 8 scenarios, 212 runs, all pass |
| Swarm | 7 scenarios, 9 runs, all pass |
| Outpost | 20 scenarios, 43 runs, all pass in 804 s |
| toybox, puppet | 3 scenarios each, all pass |
| The web page | Hollow built and checked in headless Chrome (the cold load over 20 Mbit, the run against the desktop's, the AudioWorklet), and the browser editor's page as a phone |

Swarm's stress gate with both windows: the tick's 95th percentile 1.9 ms over ticks 30 to 150, and 1.6 ms over ticks 1,080 to 1,200 with the swarm on the hero (the music 0.47 ms of it), against 6.

Outpost against the phase 4 gate, in a release build: the 10x stress tick 0.82 ms closed and 0.84 ms with the inspector open (1.08 and 1.12 then); a save at eight hours 63.8 ms and its load 247.1 ms (63.4 and 232.6); the first week's save loaded in 36.0 ms through 4 migrations (33.2). Headless fast-forward is slower: 4,982 ticks a second, 30 days in 21.7 s, against 6,771 (target over 2,000). No one system stands out. A valley tick is about 0.2 ms, the largest share the UI's layout at 0.035 ms and the valley's light following the day (item 3) at 0.015 ms, so it is the sum of what phase 5 added. It is worth watching, not a miss.

The Outpost half of the phase 4 gate, the tuning sweep (`cargo test -p argus-editor --release --test gate_outpost -- --ignored --nocapture`): the person's path read back the same way, the same four candidates, and the same report, seed for seed. 1.0 and 1.3 starve none of twenty, 1.5 starves seed 15, and 2.0 starves all twenty. It took 28.9 minutes against 21.0 at phase 4, for the slower fast-forward above.

The release builds, each checked by `argus build`:

| Game | Packs | Check |
|---|---|---|
| Hollow | 1,376 files, 3.3 MB | the same world after 300 ticks, drawn the same |
| Delve | 20 files, 204 KB | the same |
| Swarm | 30 files, 89 KB | the same |
| Outpost | 36 files, 218 KB | the same |

## Kevin's hand check

- Read the four diffs and say whether any feature should be in its game.
- Still open from earlier items: open a review link on the phone over the tailnet (item 10), and the Steam overlay and Hollow on Steam (item 9).
