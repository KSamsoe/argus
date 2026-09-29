# Phase 3 gate

The plan (design §18) ends phase 3 with a gate:

> The agent builds a headless Outpost core — colonists, needs, jobs, stockpiles, a day-night cycle — and a menu-driven in-game inspector for it, then passes the 30-day, 20-seed starvation scenario. A save from the first week of the phase still loads at the end of it, through at least one migration. Hollow reaches 200 rooms with the §17 open and stream-in numbers green.

This file records that run, done on 2026-09-28 and 29 with the engine at the commit that adds it. Every part of the gate is met, with the numbers below measured in a release build on this machine. What is left for a person is to open the inspector and use it; the agent checked it every way it can without a window (a scenario, the semantic frame, an offscreen picture, and the window's own input path with the game playing).

## The four parts

| Part | Where it stands | Where to look |
|---|---|---|
| A headless Outpost core: colonists, needs, jobs, stockpiles, a day-night cycle | Built in items 0 and 3: six colonists with hunger and rest, harvest, haul, cook, eat, and sleep jobs claimed by skill and walked to over the navigation grid, a stockpile, fields that grow by day, a 60-second day with nights to sleep through, all of it running headless | [examples/outpost](../examples/outpost), `tests/colony.luau` |
| A menu-driven in-game inspector for it | Built at the gate: below | `systems/inspector.luau`, `tests/inspector.luau` |
| The 30-day, 20-seed starvation scenario | Passes: no colonist starves in 30 days on any of 20 seeds | `tests/outpost_food.luau` (§9's) |
| A save from the first week of the phase loads at the end, through at least one migration | `tests/saves/week1.save`, written on 2026-09-24 by Outpost's first skeleton, loads through 4 migrations and the colony plays on | `tests/week1.luau` |
| Hollow at 200 rooms with §17's open and stream-in numbers green | 201 rooms (item 6); a cold open 226 to 238 ms against 300, the worst room streaming in 1.56 ms against 4 | `crates/argus-daemon/tests/perf_rooms.rs`, Hollow's `tests/deep.luau` |

## The inspector

Tab opens a panel on the right of the screen with four tabs, and Tab closes it. The arrows (or a gamepad's pad) move the focus, Enter presses, Esc backs out, and the mouse clicks. It keeps working while the game is paused, and its words are rewritten six times a second of the player's time at any speed, so at 10x it keeps up with the colony.

| Page | What it says |
|---|---|
| Colony | the day and hour, day or night, and the speed; food stored in meals and crops; colonists, how many work and how many are idle; the lost; the fields, how many are ripe, today's growth; jobs open and taken; the most common reason for idling |
| People | the colonists by name, ten to a page with arrows: each one's fed and rest and what they are doing. Pressing one opens their page |
| A person | their skills; fed and rest; their job, its stage, how far along; or why they are idle; where they are. Follow has the view follow them; Back returns to the list |
| Stores | the stockpile's meals, crops, and goods; food stored; what is lying about, by kind; whether the stove is cooking and who is |
| Jobs | each kind of job by priority, open and taken; who has what |

It is made of ordinary UI entities (item 1), built in Luau by `sim.colony.inspector.build` the first tick there is none, so a save from before it (the first week's) gets one too, and run by `sim.colony.inspector`. Its words are in the string table (`strings/en.toml`, `[inspect]`). Its components are in `components/inspector.luau`. While it is open the arrows move its focus, not the view.

This is what the agent saw of it, the semantic frame's UI with the inspector open on the People page a day into the colony:

```
$ argus obs frame format=semantic
UI e:83: Inspector  at (361,52)-(623,68)
UI e:85 button: Colony  at (361,72)-(406,92)
UI e:86 button, focused: People  at (410,72)-(455,92)
UI e:87 button: Stores  at (458,72)-(502,92)
UI e:88 button: Jobs  at (505,72)-(540,92)
UI e:98 button: Ada   fed 57%   rest 84%   cook, going  at (361,96)-(623,113)
UI e:99 button: Bram   fed 54%   rest 88%   harvest, going  at (361,115)-(623,131)
UI e:100 button: Cleo   fed 58%   rest 91%   idle  at (361,133)-(623,150)
UI e:101 button: Dov   fed 57%   rest 91%   harvest, going  at (361,152)-(623,168)
UI e:102 button: Esme   fed 57%   rest 92%   idle  at (361,170)-(623,186)
UI e:103 button: Finn   fed 56%   rest 93%   idle  at (361,188)-(623,204)
UI e:109 button, disabled: < Prev  at (361,206)-(404,226)
UI e:110: 1 of 1  at (410,210)-(435,222)
UI e:111 button, disabled: Next >  at (441,206)-(484,226)
UI e:141: Arrows move, Enter picks, Esc backs out, Tab closes  at (361,231)-(623,242)
```

and an offscreen picture of the same frame showed the panel laid out as intended, the People tab marked and focused, the arrows greyed out on a single page.

The tests:

- `tests/inspector.luau`, the valley: closed at first; Tab opens it on the colony; the arrows and Enter reach People and list the six by name; down and Enter open Ada's page (her skills, her needs); Follow has the view follow her; Esc backs out, and again closes it, the arrows moving the view again; paused, it opens where it was left and the day stands still in it; a click on the Stores tab shows the stockpile; at 10x a day on, the Jobs page lists every kind and the colony page says day 1.
- The same file, the stress scene: two hundred colonists are twenty pages; a click on Next shows the next ten.
- `crates/argus-daemon/tests/window_play.rs`: the game playing in real time, keys and the mouse sent as a window sends them. Tab opens it, the arrows and Enter reach People, a click opens Jobs, Esc closes it, and the game plays on meanwhile. Kevin's play of items 7 and 8 found two faults on this path that tests driving actions had missed, both fixed before the gate (below).

## What the gate found

Building the inspector and measuring the gate's numbers found seven faults, all fixed.

**1. A closed inspector doubled a debug tick.** The UI layout read every text node's words from the string tables each tick, hidden ones too, and the UI crate and its layout library ran unoptimized in debug builds. Outpost's week scenario went from 26 s to 52 s, and the 30-day one from 505 s to 966 s. A hidden node now keeps its words while its text, its key, and the language are the same (a language chosen still reaches a hidden menu at once), and debug builds optimize `argus-ui` and `taffy` as they do the sim's crates. `ui.layout` in the valley went from 0.151 to 0.031 ms a tick in debug, and the two scenarios take 28 s and 547 s now.

**2. Hollow's cold open was over its budget.** 267 to 305 ms against 300. The open's parts, timed one by one, showed 52 ms of making item 7's music: Hollow's tone loops, several seconds of samples each, made one after another. Tones are made on every core now (`argus_audio::sound::synth_all`; the web build, which has no threads, makes them in turn), and the open is 226 to 238 ms.

**3. A long step grew a daemon to 26 GB.** Kevin noticed the disk filling: the release timing test stepped Outpost eight hours in one `sim.step`, and Windows paged the daemon's memory onto the disk. A keyframe is a copy of the world, and the world's queue of events holds, during a step, every event since the step began, until the daemon logs them. Every keyframe taken deep into the step copied all of it, and the keyframe ring's budget counted none of it. A snapshot keeps its event queue as §6 says, but the queue is this frame's events, the ones a system can still read; the older ones are the log's. `World::bytes` counts the queue and the broadphase now. One 108,000-tick step went from 1,231 MB to 317 MB.

**4. A load after eight hours took 1.6 s.** Outpost's HUD makes a new string twice a second, and every keyframe copied the whole string table, string by string: at eight hours, 24,000 strings in each of about 360 keyframes, millions of allocations to make and free. The table's strings are shared between copies now. The load at eight hours is 224 ms against 500.

**5. Each `q:events` call grew with the step.** A script's `q:events(name)` scanned the whole queue for this frame's events, so in a long step every call got slower; the inspector asks a dozen times a tick. It reads only the queue's tail now, where this frame's events are, as does the tick-end hook that finds `q:save` requests. The inspector's p95 went from 0.58 to 0.07 ms.

**6. The inspector did its work every tick it was open.** It sorted two hundred colonists every tick; it sorts them only for a page of them now, and writes its words by the player's time, not every ten ticks at any speed. With it open, the stress scene's tick p95 is 1.037 ms against 1.6.

**7. The build output filled the disk.** `target/debug` was 213 GB: every build's test binaries with full debug information, and every stale one kept. Debug builds keep line tables only now, which is what backtraces use.

Found on the way to the gate, in Kevin's play of items 7 and 8, and fixed then: `argus daemon run --window` opened the game stopped (the command he was given left out `--play`), so the window drew it and answered F8 but nothing moved; a window opened from the command line plays at once now. And `argus scene load mods=[...]` froze a playing window, since a scene load started its fresh clock stopped; a scene load keeps the play state now, as a save's load does. The daemon opens with mods from one command, `--mods`.

One was the agent's own, in the inspector's test: its `press` helper dropped the value it was given, so every arrow pressed right. Printing the buttons' rectangles and focus showed it at once.

## Did it fight the protocol

No. The inspector is Luau and UI entities, the same as Hollow's pause menu, and the agent saw it through the protocol: the semantic frame's UI section (above), `obs frame` for a picture, `world.query` for which page is shown and which button has focus, `input inject` and the harness for pressing. `prof systems` found what was slow in each case; nothing needed a debugger.

The limits it met, recorded so they are decisions:

- **The string table grows with play.** Outpost's HUD line is a new string twice a second: about 3,000 an hour, 24,000 by eight hours, a megabyte or two held once and shared by every copy. A per-column arena that reuses slots would bound it; nothing at this size needs it yet.
- **The daemon writes a save when the step returns.** A game asking to save every day, stepped eight hours in one `sim.step`, holds 480 saves until the step ends. A session steps a frame at a time, and the harness in runs of at most 100,000 ticks, so neither meets it; the timing test steps a game day at a time.
- **The mixer catches up at most ten seconds.** Headless, the mixer follows sim time, and one step longer than ten seconds leaves its clock behind the sim's. Nothing is heard headless, and the music's events are logged at the step's end, so no test meets it; a long run's music events come at the mixer's time.
- **The inspector covers the map's right edge** and has no scrolling: a page holds ten colonists, and the arrows turn pages.

## Numbers

In a release build on this machine (`cargo test -p argus-daemon --release --test perf_rooms --test perf_outpost -- --ignored --nocapture --test-threads 1`, and `cargo test -p argus-ui --release -- --ignored perf`):

| §17 row | Measured | Target |
|---|---|---|
| Hollow, 201 rooms: cold scene open, the index warm | 226 to 238 ms (five opens) | under 300 ms |
| Hollow: room stream-in, worst tick of a 25-room walk | 1.56 ms (mean 0.47 ms) | under 4 ms |
| Outpost: sim tick at 10x, 200 colonists, 20k items | p95 0.974 ms closed, 1.037 ms with the inspector open on its people page | under 1.6 ms |
| Outpost: headless fast-forward | 6,636 ticks a second (30 days in 16.3 s) | over 2,000 |
| Outpost: save and load at 8 h of play | 66.5 ms (2.0 MB) and 223.7 ms; the colony at day 479, six colonists | under 200 and 500 ms |
| Outpost: the first week's save | loaded in 35.9 ms through 4 migrations | under 500 ms |
| UI tree, 2,000 nodes with text | 0.909 ms mean (1.075 worst) | under 1 ms |
| The inspector's layout in the stress scene (62 nodes) | `ui.layout` p95 0.030 ms | |

The tests, in a debug build:

| Tests | Result |
|---|---|
| Workspace | 407 pass, none fail. The 32 timing tests are skipped in debug (the gate's ran in release, above), and one doc example is not run |
| Outpost | 11 scenarios, 34 runs, all pass in 587 s: the 30 days on 20 seeds in 547 s, the week on 5 seeds in 28 s |
| Hollow | 17 scenarios, all pass in 16 s |
| Delve, Swarm | Delve's 5 scenarios, 208 runs, and Swarm's 3, 5 runs, all pass |
