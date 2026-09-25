# Phase 3 checklist

Section 18 of the design document lists phase 3 as one sentence and a gate. This file breaks the sentence into pieces, attaches every constraint from elsewhere in the document that each piece has to meet, and records status, the way `phase2-checklist.md` did for phase 2. The order was agreed with Kevin on 2026-09-24.

Phase 3 sentence: *UI tree with layout and text; dialogue and string tables; timelines; navigation grid, A\*, flow fields; job system; save games with migrations; room streaming and transitions at scale; audio buses, ducking, and the adaptive music system; mod pack loading.*

Gate: *The agent builds a headless Outpost core — colonists, needs, jobs, stockpiles, a day-night cycle — and a menu-driven in-game inspector for it, then passes the 30-day, 20-seed starvation scenario. A save from the first week of the phase still loads at the end of it, through at least one migration. Hollow reaches 200 rooms with the §17 open and stream-in numbers green.*

Already built, and extended here rather than started: audio buses, ducking rules, and a first music layer model (phase 1 item 3; tempo and bar-synchronised transitions were left for this phase); rooms, the room graph, streaming by graph distance, and `scene.transition` (phase 0 item 5; "full streaming at scale is phase 3"); content-addressed packs and language packs (phase 2 item 7); component versions, and `migrate` functions accepted and stored but never run (phase 0 items 3 and 5); the harness's `days`, `series`, `trend`, `sum`, and `path` (phase 0 item 10); Luau instruction budgets and the sandbox (phase 0 item 3).

Carried in, found while planning:

- **`world.query` has no aggregates.** §8 gives L1 as "world.query with selection, filters, spatial predicates, aggregates", and names Outpost's use: "world.query with group_by and sum over stockpiles, job queues, and colonist needs". The protocol has neither; the harness's `t.expect:sum` adds rows up on its own side. It goes in item 3, whose customer it is.
- **`t.expect:path` checks the room graph, not tiles.** Phase 1 carried "tile pathfinding under `t.expect:path`" to the navigation grid; it goes in item 2.
- **Stopgaps the UI tree replaces:** the window's one-line overlay bar (the F8 note prompt, the proposal bar), `Describe.shown` text drawn in the world, the post flash covering HUD text, no settings screen or fullscreen toggle in a release, and no pointer action for the mouse. Item 1.
- `crates/argus-cli/SPEC-TRACE.md` lists "show as command" as deferred to phase 3 with the editor; the editor is phase 4. Corrected when this file lands.

## Decided with Kevin (2026-09-24)

- **The order below.** Save games first, then the UI tree, then navigation and jobs with Outpost's core, then dialogue and timelines, then streaming at scale with the music that follows rooms, then mod packs.
- **An Outpost day is 60 seconds of sim time.** The harness's `days(n)` was `n × 86,400` sim seconds. At §17's fast-forward target of 2,000 ticks a second, 30 such days at 60 Hz is 155.5 million ticks, 21.6 hours a seed, where §17 says the scenario runs "in under a minute per seed"; the two agree only if a game day is about a minute. A project says how long its day is (`[time] day` in `argus.toml`, in seconds, 86,400 when it says nothing), and `days()` and `hours()` use it. At 60 s a day and 60 Hz, 30 days is 108,000 ticks, 54 s at 2,000 ticks a second.
- **The first-week save is a fixture.** Item 0 builds save games and a first Outpost skeleton (colonists eating from a stockpile, headless), saves it, and commits the save. A test loads it on every run from then on, so every later change to Outpost's components needs a migration or the test fails; at the gate it has been through several.
- **Hollow's 200 rooms are generated.** The three hand-made rooms stay as the start, and a tool in `examples/hollow/tools/` writes the other 197 as a connected graph of room directories, about 50 authored entities each (10,000 in all, §17's number), with tilemaps.
- **Outpost has placeholder art and a window**, beside its headless core, so the inspector is something a person can open and use.
- **Save games on the web persist in the browser's storage** (IndexedDB), in item 0.

## 0. Save games with migrations

**Status: done** (2026-09-24) in `argus-ecs` (component versions in the world format; `decode_state_with` and the `Migrate` trait), `argus-luau` (`migrate` as a table of steps by version or a function; the host runs them; `q:save`, `q:load`, `q.day`), `argus-player` (`savegame.rs`: the save file, reading it through migrations; `Player::take_saves`, `take_loads`, `load`), the daemon (`saves.rs`: `sim.save`, `sim.load`, the tick-end capture of a game's saves), `argus-game` (saves in the save location's `saves/`), `argus-web` and the page (saves in IndexedDB), `argus-scene` (`[time] day`), and `argus-test` (`days()` in the project's days). A save is the game's identity and a keyframe without the input state; a load opens the save's scene fresh with its seed and puts the rest back, components saved at an older version through their declared migration (or field by field, by name), each reported as a `component.migrated` event; a save from a newer version is refused. Outpost's first skeleton is in `examples/outpost`: six colonists eat from a stockpile the fields fill, a day is 60 s, the game saves itself as "auto" when a day turns, and `tests/saves/week1.save` (day 7) is the first-week save the gate asks for, loaded by `tests/week1.luau` on every run. Measured: a week of Outpost (25,200 ticks) runs in about 2.5 s in a debug build; its save is 5.6 KB. In headless Chrome, Outpost's page kept its autosave in IndexedDB, loaded it back to its tick, still had it after a reload, and refused a damaged one. Found while building it: the first version of the atomic load swapped in an empty event log and the harness's event counts after a load read nothing (a `since` cursor from before it), so a test of "nobody died" passed for the wrong reason; event numbers now keep counting over a load, and a test checks it. A fresh scene's clock starts stopped, so a game loaded while playing would have frozen, the same fault as phase 2's proposal variants; a load carries the play state over. Outpost's first meal rule wasted half of every meal and starved the colony in a week; a meal now fills a colonist up with what the stockpile has. Traces: rows in `argus-ecs`'s, `argus-luau`'s, `argus-player`'s, the daemon's, `argus-scene`'s, `argus-test`'s, `argus-os`'s, `argus-game`'s, and `argus-web`'s. Constraints:

- §6: a save is a snapshot with a schema version, written by `sim.save` to a player-facing location instead of the dev ring. It must survive code changes: components carry a version, and a component's schema file may declare a migrate function from older versions.
- §6: on `sim.load` the daemon runs migrations column by column and reports each one as an event ("Health migrated v2→v3 for 1,204 entities"), and a test can assert a saved game from last month still loads.
- §6: Outpost is the reference game; Delve uses it for run persistence; Swarm barely needs it and pays nothing.
- §19 open question: migrations as Luau functions per component or a declarative rename/default/drop table. "Probably both, table first."
- §15: save migrations stay in the release build, "because they are how a patched game loads an old save"; a save location per platform convention (`argus-player`'s `save_dir`, phase 2 item 7).
- §3: runtime-spawned entities (colonists born mid-game) never get string ids, and Outpost's runtime state is the game and must persist as a save.
- §5: `sim.save` and `sim.load` in the `sim` namespace.
- §17: save and load at 8 hours of play under 200 ms and under 500 ms with migrations ("saves must be frequent enough to be automatic"); measured at the gate, on Outpost.
- Phase 2 decision: the Luau heap is not snapshotted, so a save holds nothing a script keeps outside components.
- The gate: a save from the first week of the phase loads at the end, through at least one migration.

Decisions taken while building it:

- **Migrations are both, table first.** `migrate = { [2] = { rename, default, drop }, [3] = function(row) ... end }` steps from the saved version up, a table step run in Rust, a function in the VM; or one `migrate = function(row, from)`. Without one, a component's rows come field by field, by name (what marks already did), and the change is still reported.
- **A save does not hold the input state.** A loaded game hears the player's hands now.
- **A game asks; its runtime keeps.** `q:save(name)` captures the world of the tick that asked; the daemon writes `.argus/saves/`, a release build its save location, the page IndexedDB. `q:load(name)` happens after the step that asked, since a world is not replaced in the middle of ticks.
- **A save from a newer version is refused**, not read field by field, since it would silently lose what the newer game knew.
- **A load keeps the session's**: the change feed, undo, locks, and the event numbers. It leaves the game as it was when it fails.
- **Days are the project's**: `[time] day` in argus.toml (question 2 of the plan, decided with Kevin).
- **Delve's run persistence waits** for a need: nothing in Delve asks to be saved yet.

## 1. UI tree with layout and text

**Status: done** (2026-09-25) in a new crate, `argus-ui` (the components, the flexbox layout on taffy, text, the theme, focus and presses, the draw list), and in `argus-render` (the UI's own pass after the post passes; font atlases; the semantic frame's `ui` section; the window's bar through the UI), `argus-assets` (fonts: a `.ttf` or `.otf` baked into a distance-field atlas), `argus-material` (the `sdf_text` built-in; parameters padded for WebGL2), `argus-input` (the pointer action; rebinding by a UI button, `input.bind`), `argus-luau` (`q:freeze()`), `argus-player` (the UI and rebinding in the one assembly; fonts), the daemon, `argus-game` (fullscreen from the game's settings), `argus-web` and the page (the mouse as the pointer, `semantic()`, fullscreen), and the CLI (`UI` lines; a web-build fix). Hollow's HUD and signs are UI now, and Escape opens a pause menu (Resume, Restart the run, Controls, Settings): Controls rebinds Jump, Restart, and Pause to the next key pressed; Settings cycles the text size (100, 125, 150%) and turns high contrast and fullscreen on and off. Delve's HUD line and its menu (Resume, New run) are UI in Liberation Sans, shaped and drawn from a distance field. Measured: 2,000 nodes with text lay out in 0.89 ms a tick in release (a first layout from nothing, about 7.5 ms). In headless Chrome the page drew Hollow's pause menu as the desktop does, on WebGPU and WebGL2; Escape opened it, the real mouse focused Restart the run, and a click pressed it. Found while building it: a text measured whole and then broken word by word came out a pixel wider than the box laid out for it, so Delve's menu title wrapped under its buttons; a paragraph is measured as its words and spaces now, in whole pixels up. The first font made every Delve test run bake it again (2.7 s became 17.7 s); a process bakes a font once. `sdf_text`'s two parameters stopped the page on WebGL2, which binds no uniform smaller than 16 bytes; every material's parameters are padded now. And the web build could not link a new build script (`libm`, through rustybuzz): Windows' `\\?\` path prefix reached MinGW's linker. Traces: `crates/argus-ui/SPEC-TRACE.md`, and rows in `argus-render`'s, `argus-input`'s, `argus-assets`'s, `argus-material`'s, `argus-luau`'s, `argus-player`'s, the daemon's, `argus-game`'s, `argus-web`'s, and `argus-cli`'s. Constraints:

- §12: a retained UI tree built from the same entities and hierarchy as everything else, with a flexbox-style layout system, bitmap and SDF text with shaping, nine-slice panels, and input focus.
- §12: not a separate world: a menu is a query away like anything else, and the semantic frame (§8) reads it directly.
- §12: the customers are Delve's menus, Outpost's inspector panels and overview screens, and every game's pause menu. It is "the subsystem that most clearly separates a general engine from an action-game framework, and it is scheduled early for that reason."
- §12 accessibility: text scaling and high-contrast palettes are UI theme tokens; rebinding is a UI over the action map's table; input-hold and toggle alternatives are action-map options; the semantic frame is a screen-reader feed.
- §12 input actions: gameplay never sees keys; menus are driven by actions (confirm, cancel, move) from any device and player slot.
- §3: the transform hierarchy is first-class, and UI trees use it.
- §13: UI slides are tweens.
- §17: a UI tree of 2,000 nodes with text lays out in under 1 ms ("inspector panels and overview screens are most of what the player looks at").
- The stopgaps it replaces (carried in above), and the release game's settings screen and fullscreen toggle.
- The web page draws it the same way as the desktop.

Decisions taken while building it:

- **One canvas, fitted.** A project's UI is laid out on one canvas (`ui.toml`, 960 x 540 by default), scaled whole to fit the window and centred. The mouse goes onto the canvas through the same fit, so a click lands where the button is drawn.
- **Layout is world state.** `ui.layout` runs in the tick (late stage, after the camera), so every `UiRect` is in the world: a seek, a fork, a replay, or a test sees the layout the player saw. The taffy tree is kept between ticks as a cache and compared with the world each tick.
- **The UI runs while a game is frozen.** `ui.input` and `ui.layout` are `unfrozen`, and `q:freeze()` lets a game's pause stop everything else, the hit-stop's mechanism.
- **Focus is fixed when a menu appears.** A menu that shows with nothing focused gets its first button that tick, so a keyboard or pad player never has to find it.
- **The mouse is an action.** A `pointer` action is a place on the canvas; gameplay still never sees a device.
- **Rebinding is UI over the map.** A button with an `InputBinding` takes the next press; `input.bind` is the protocol's way, and `save` writes `input.toml`.
- **Fonts are assets, baked on import.** A distance field per font at 32 pixels an em, shaped with rustybuzz; the font ships as it is in a release and bakes at start. The OFL font Delve uses, Liberation Sans, was already on this machine (in a package's font folder), so nothing was downloaded; its licence is beside it.
- **The window's bar is the UI's.** The F8 prompt, the proposal bar, and the release game's report prompt draw through the UI, in the game's font, four lines at most. The picture laid over the frame is gone, and so is `Describe.shown`.
- **Fullscreen is a setting the host reads.** `UiSettings.fullscreen` is written by the game's settings screen and applied by the release window and the page each frame, so the choice goes through the input log like any other.
- **Settings last for the session.** Keeping them between launches needs a store outside the world (a replay must not change with them); that is phase 5's accessibility pass.

## 2. Navigation grid, A\*, flow fields

**Status: to do.**

- §12: a navigation grid derived from the tilemap and blocking entities, with A\* for individuals and flow fields for crowds, both Rust hot-path systems.
- §12: Outpost is the customer, but Delve's monsters and any game with NPCs use the same path queries.
- §7: Outpost's pathfinding and flow fields are named as Rust hot paths.
- §4: a baked navigation grid is bulk authored data: a text index plus a binary chunk, queried through the protocol.
- §9: `t.expect:path(...):exists()` (Delve's "stairs are always reachable on floor 1", 200 seeds) answers over tiles, not only the room graph (carried in from phase 1).
- §17: Outpost's sim tick at 10× with 200 colonists and 20,000 items under 1.6 ms "with pathfinding, jobs, and needs all running".

## 3. Job system, and Outpost's core

**Status: to do.**

- §12: entities publish tasks with requirements; agents claim them by priority and distance.
- §12: `obs.why` can explain "why is this colonist idle" from the job system's declared events.
- §8: `world.query` aggregates, group_by and sum, over stockpiles, job queues, and colonist needs (carried in above). The Outpost question is "is the economy drifting", not "where is entity 4011".
- §4: Outpost is mostly defs (items, recipes, job priorities); an agent balancing its economy edits tables and runs seed sweeps, never touching a scene.
- §10: in Outpost the tune fields are job priorities, consumption rates, and growth curves.
- §6: Outpost's 1×/3×/10× speed controls are `sim.speed`; headless fast-forward runs as fast as the CPU allows.
- §9: the balance scenario: `scene = "outpost/valley"`, seeds 1 to 20, `t:run_until { sim_time = days(30), speed = "max" }`, food stockpile at least 60, no `colonist.died` with cause starvation, the daily food series trending flat or up.
- §17: headless fast-forward over 2,000 ticks a second ("a 30-day balance scenario in under a minute per seed", question 2); sim tick at 10× with 200 colonists and 20,000 items under 1.6 ms.
- The gate's core: colonists, needs, jobs, stockpiles, a day-night cycle.

## 4. Dialogue and string tables

**Status: to do.**

- §12: dialogue is a def table (§4) with a small node graph: lines, choices, conditions on world state, and effects as edits. It runs from Luau and is inspectable through `world.defs`.
- §12: all player-facing strings go through a string table keyed by id; the agent can author in one language and a translator can fill the others without opening the engine.
- §15: localization packs are separate, so a language can be added without a rebuild (phase 2 item 7 lays whole files over the game's; string tables replace that for text).
- §13 audio: "dialogue ducks music by 6 dB over 200 ms" is a ducking rule, so dialogue plays on a bus.
- §13: timelines have dialogue-line tracks (item 5).
- The customers: Delve's menus and messages, Hollow's signs, Outpost's event text.

## 5. Timelines

**Status: to do.**

- §13: above tweens sits a timeline: a def table of tracks (tweens, animator states, camera cuts, dialogue lines, events) against time, playable from Luau.
- §13: scrubbable in the editor. The editor is phase 4; here, scrubbing is `sim.seek` over a timeline's frames, which the editor will drive.
- §13: cutscenes, boss intros, and Outpost's event vignettes are timelines, and "sequencing lives in data, not in the Luau heap" (the §19 open question on coroutine state).
- §12 camera: cutting between two cameras is an event; blends and camera moves on a curve were left for tweens and timelines (`argus-camera` trace).
- §4: room transitions have a game hook (fade, camera slide); a slide is a timeline or a tween (item 6).

## 6. Room streaming and transitions at scale

**Status: to do.**

- §4: rooms stream in and out around the player by graph distance, so a two-hundred-room game never has more than a handful of rooms live, and the index still answers queries about rooms that aren't loaded.
- §4: transitions between rooms are an engine event with a hook for the game (fade, camera slide), not something each game reinvents.
- §17: 10,000 authored entities across 200 rooms; cold scene open (index warm) under 300 ms; room stream-in under 4 ms ("must fit inside a frame so transitions never hitch"); a 4,096 × 4,096 tilemap with 8 layers (met in phase 1 item 5).
- The gate: Hollow reaches 200 rooms with the open and stream-in numbers green (question 4).
- §4: the index is what keeps file-per-entity true at Hollow's scale, "where the whole scene is thousands of files".
- Carried from item 1: Hollow's HUD, pause menu, Controls, and Settings live in room `a`, which the scene's radius of 2 keeps loaded. With 200 rooms they need a place that is always loaded.

## 7. Adaptive music, and the buses and ducking around it

**Status: to do.**

- §13: the mixer has named buses (music, sfx, ui, ambience) with a routing tree and ducking rules declared as data (built in phase 1).
- §13: the music system is adaptive: a track is a set of layers with a tempo, transitions between tracks happen on beat or bar boundaries, and intensity is a parameter a system writes (`Music.intensity = enemies_near / 10`). Phase 1 built layers and intensity; tempo and synchronised transitions are this item.
- §13: Hollow's room-to-room music and Outpost's day-night ambience are both this.
- §8: all of it is observable as the event log; the human hears it, the agent reads it.

## 8. Mod pack loading

**Status: to do.**

- §15: a release build loads mod packs (additional defs, scenes, assets, and Luau systems) from a user directory, in a declared load order.
- §15: with the same sandbox and instruction budgets as the game's own scripts, and the same schema validation.
- §15: a load-order rule and a namespace prefix on mod content; games that don't want mods set one flag.
- §15: packs are content-addressed (phase 2 item 7); a mod pack is one more layer over the game's.
- Last, so mods can carry everything the earlier items make: dialogue, string tables, timelines, music.

## Gate

**Status: not started.**

- The agent builds a headless Outpost core (colonists, needs, jobs, stockpiles, a day-night cycle) and a menu-driven in-game inspector for it.
- It passes the 30-day, 20-seed starvation scenario (§9's `tests/outpost_food.luau`).
- A save from the first week of the phase loads at the end, through at least one migration.
- Hollow reaches 200 rooms with §17's open and stream-in numbers green.
