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

**Status: done** (2026-09-25) in a new crate, `argus-nav` (grids, A\*, flow fields, baked grids, the `nav.agents` system), and in `argus-ecs` (`World::structure`, which tells a cache the world's shape moved), `argus-tiles` (a map's revision), `argus-luau` (`q:path`, `q:walkable`, `q:distance`, `q:downhill`), `argus-player` (navigation in the one assembly), the daemon (`nav.grids`, `nav.grid`, `nav.path`, `nav.field`, `nav.bake`; the `query` tool's `path`), `argus-test` (`t.expect:path` over tiles), and `argus-scene` (a baked grid's index is not an entity). A `NavGrid` covers a tilemap (solid tiles blocked), a baked grid, or a rectangle; `NavCost` entities over it block cells or make them slower. A `NavAgent` walks its entity to a goal or an entity by A\*, or down a `NavField`, a flow field toward another entity that a whole crowd shares. Delve's monsters now step down the hero's flow field instead of a breadth-first search in Luau every turn, and move exactly as before (its golden is unchanged); its stairs test runs over the floor, 200 seeds, as §9 has it. Outpost walks with its jobs in item 3. Measured in release: A\* across 256 x 256 (eight ways, a fifth walls, costs 1 to 3) 0.55 ms; a flow field over 256 x 256, 1.8 ms; 200 agents with goals of their own on 128 x 128, 1.6 ms the tick they all search and 0.09 ms a tick after; 10,000 followers of one field, 1.5 ms a tick. Found while building it: the first agents system cost 7.4 ms a tick for the 10,000, mostly lookups (the grid list and the field found again per agent, and a missing `Parent` looked up as an error to throw away); read and written by column, with the tick's grids and fields looked up once, it is 1.5 ms. Traces: `crates/argus-nav/SPEC-TRACE.md`, and rows in `argus-ecs`'s, `argus-tiles`'s, `argus-luau`'s, `argus-player`'s, the daemon's, `argus-test`'s, `argus-scene`'s, and `argus-cli`'s. Constraints:

- §12: a navigation grid derived from the tilemap and blocking entities, with A\* for individuals and flow fields for crowds, both Rust hot-path systems.
- §12: Outpost is the customer, but Delve's monsters and any game with NPCs use the same path queries.
- §7: Outpost's pathfinding and flow fields are named as Rust hot paths.
- §4: a baked navigation grid is bulk authored data: a text index plus a binary chunk, queried through the protocol.
- §9: `t.expect:path(...):exists()` (Delve's "stairs are always reachable on floor 1", 200 seeds) answers over tiles, not only the room graph (carried in from phase 1).
- §17: Outpost's sim tick at 10× with 200 colonists and 20,000 items under 1.6 ms "with pathfinding, jobs, and needs all running".

Decisions taken while building it:

- **Blocking entities are one component either way.** `NavCost` sets the cost of the cells under an entity: 0 blocks (a wall, a building), more is slower going. A grid whose `fill` is 0 is a map whose open cells are entities, which is Delve's floor.
- **Paths are world state, caches are content.** An agent keeps the cells its path was found between and the grid's hash, and follows the path from them; searches are cached by the grid's hash and the cells. A seek, a fork, a load, or a replay walks the same way with a store that never saw the run.
- **A grid is checked once a frame and after the world's shape changes.** The world's `structure()` moves on every spawn, despawn, component added or removed, and restore. A field edited mid-frame shows the next frame; protocol calls always check.
- **Integer costs, fixed tie order.** A straight step of cost 1 is 10, a diagonal 14; ties go east, west, north, south. The same grid gives the same path everywhere, the browser included.
- **Diagonals never cut a corner.** With `diagonal`, a diagonal step needs both cells beside it open.
- **The dozen tools stay a dozen.** The `query` tool takes `path: {from, to}` (§16 names the tool set); the `nav.*` methods are the namespace's own.
- **Scripts get queries, not the store.** `argus-luau` defines the `Paths` trait and the assembly hands it one, so the script crate needs nothing of navigation.

## 3. Job system, and Outpost's core

**Status: done** (2026-09-25) in a new crate, `argus-jobs` (the `Job`, `Worker`, and `JobKind` components; `jobs.update`; why a worker is idle), and in `argus-nav` (a grid's islands, who can reach what), `argus-index` (`world.query` aggregates: `count()`, `sum`, `mean`, `min`, `max`, `group_by`), `argus-luau` (`q:def`, `q:defs`, `q:speed`; a script error on an engine component names the script's line), `argus-player` (jobs, tables, and speed in the one assembly), the daemon (`jobs.why`; `obs.why` on a `job.*` event carries the worker's jobs; `speed.requested`; `sim.run_until` says when it stopped short), `argus-game` and `argus-web` (the speed a game asks for), `argus-test` (`sum` is the daemon's; a long run goes on past one call's cap), and the CLI (aggregates in bare words; `argus test name=...`). A game posts a `Job` by spawning it: a kind, a priority, the skill it needs, a site, seconds of work, and optionally something carried somewhere after. A `Worker` takes the best open one it can do, by priority and then distance, never one it cannot reach over the grid, walks there with its `NavAgent`, works, carries, and `job.done` says so. A job for one worker only (a meal, a night's sleep) that outranks what it is doing takes it off that. A worker with no job says why, in `Worker.idle` and a `job.idle` event, and `jobs.why` or `obs.why` list every job with what the worker makes of it. Outpost is a colony now: a valley laid out from the seed (a grid with trees, ten fields sown at different times, a stove, a stockpile, a bed each), six founders with their skills from `defs/founders.toml`, job kinds from `defs/jobs.toml`, items and recipes from `defs/items.toml` and `defs/recipes.toml`. Fields grow by day with the weather, farmers harvest them, haulers store the crops, cooks make meals, the hungry eat, and at dusk everyone sleeps until the day turns. The speed keys ask for 1x, 3x, and 10x and space pauses. The first week's save loads through four migrations (Colony and Stockpile declared, Colonist and Needs field by field), and the next tick lays a valley out around its six colonists. Section 9's scenario passes: thirty days on twenty seeds, nobody starves, food at least 60 and trending up. Measured: in release, Outpost's stress scene (200 colonists, 20,000 stones to haul on a 96 x 64 valley) ticks in 1.06 ms at the 95th percentile, under section 17's 1.6 ms; its largest parts are the needs script over 200 colonists (0.36 ms) and the broadphase rebuilt over 20,000 items (0.35 ms), with the job system 0.05 ms and the agents 0.06 ms. The valley fast-forwards at about 7,000 ticks a second in release, over section 17's 2,000, so a thirty-day seed takes about 15 s (under its minute); the twenty seeds run in six minutes in a debug build. The job system alone, 200 workers hauling among 20,000 items with 400 open jobs, is 0.45 ms a tick. Found while building it: a stove had three cook jobs, two orphaned, because the kitchen posted a new one between the job system finishing the old one and the system that handles finished jobs forgetting it (it forgot the new one); finished jobs are handled right after the job system now, and a reference is forgotten only when it names the job that finished. `sim.run_until {ticks}` stopped at its 100,000-tick cap and said `hit: true`, so a thirty-day run asked for in one call looked done at day 27.8; it says `hit: false`, and the harness goes on in calls. `argus test name=speed` ran every scenario: the command dropped `key=value` words; it takes them now and refuses one it does not know. Grouping by a field counted every entity without it as a blank group; `group_by` keeps to the entities that have its components. A range error on an engine component pointed at the Rust source that declared it; it names the script line now. `q:each` hands back one reused entity handle, so the skeleton's starvation code, which saved it in a table, would have despawned the wrong colonists had two died in one tick; Outpost's systems keep ids. Traces: `crates/argus-jobs/SPEC-TRACE.md`, and rows in `argus-nav`'s, `argus-index`'s, `argus-luau`'s, `argus-player`'s, the daemon's, `argus-cli`'s, `argus-test`'s, `argus-game`'s, and `argus-web`'s. Constraints:

- §12: entities publish tasks with requirements; agents claim them by priority and distance.
- §12: `obs.why` can explain "why is this colonist idle" from the job system's declared events.
- §8: `world.query` aggregates, group_by and sum, over stockpiles, job queues, and colonist needs (carried in above). The Outpost question is "is the economy drifting", not "where is entity 4011".
- §4: Outpost is mostly defs (items, recipes, job priorities); an agent balancing its economy edits tables and runs seed sweeps, never touching a scene.
- §10: in Outpost the tune fields are job priorities, consumption rates, and growth curves.
- §6: Outpost's 1×/3×/10× speed controls are `sim.speed`; headless fast-forward runs as fast as the CPU allows.
- §9: the balance scenario: `scene = "outpost/valley"`, seeds 1 to 20, `t:run_until { sim_time = days(30), speed = "max" }`, food stockpile at least 60, no `colonist.died` with cause starvation, the daily food series trending flat or up.
- §17: headless fast-forward over 2,000 ticks a second ("a 30-day balance scenario in under a minute per seed", question 2); sim tick at 10× with 200 colonists and 20,000 items under 1.6 ms.
- The gate's core: colonists, needs, jobs, stockpiles, a day-night cycle.

Decisions taken while building it:

- **A job is an entity; a worker has one at a time.** A game posts a task by spawning a `Job` and learns it is done from `job.done`, which carries everything needed to act on it; the job entity is gone after. Hauling several things in a trip, or two on one job, are a game's jobs made of these.
- **Priority first, then distance, never out of reach.** Distance is a straight line to where the worker would stand; reach is exact, from the grid's islands (a label a cell, found once a grid). A path's length for every candidate would be an A* per job per look.
- **Only a worker's own job takes it off another.** A meal or a night's sleep (`only`) outranks hauling and takes the worker off it; an open job for anyone waits for someone free. A carried job let go starts again where its cargo was left.
- **Job priorities are tune fields and defs both.** Section 4 puts job priorities in defs, section 10 makes them tune fields. `JobKind` entities hold the colony's priorities while it runs (a slider changes every job of a kind, the next tick); Outpost founds them from `defs/jobs.toml`, so a table edit changes a new colony and a slider the running one.
- **The queue is a query.** `world.query {group_by: "Job.kind", select: ["count()"]}`: aggregates were section 8's own ask, so no `jobs.list`. `jobs.why` is the one new method, and `obs.why` carries its answer for a `job.idle`.
- **Scripts read tables, whole rows, frozen.** `q:def("item:meal")`, `q:defs("jobs")`; a row is made once and kept, read-only.
- **A game asks for a speed; its host sets it.** `q:speed(x)` is an event the daemon, the executable, and the page answer after the step, so the ticks, and a replay, are the same at any speed. Not 0: a game pauses with `q:freeze()`, which the colony's controls do from the input stage.
- **Outpost's valley is its own runtime state.** A colony sim's map and buildings are the game's to lay out (from the seed) and the player's to change, so the scene holds the colony alone and `sim.colony.found` lays out the rest once. That is also how the first week's save, from before there was a valley, plays on: the next tick lays one out around its colonists. The colony keeps its own clock, which a pause stops.
- **The hot engine crates are optimized in debug builds too.** `argus-ecs`, `argus-sim`, `argus-luau`, `argus-nav`, and `argus-jobs` at opt-level 2, and the Luau VM at 3: the twenty-seed, thirty-day scenario is two million ticks of scripts, over twenty minutes unoptimized and six now. Debug assertions and overflow checks stay on.
- **Section 9's scenario declares its series first.** Written as section 9 has it, after the run, the harness runs each seed a second time to sample from the start; declared first, the same series costs nothing more.

## 4. Dialogue and string tables

**Status: done** (2026-09-25) in a new crate, `argus-dialogue` (the engine's `dialogue` def table, its graph, conditions and effects, the `Talk` component and `dialogue.update`), and in `argus-ui` (string tables: `strings/<lang>.toml`, `UiText.key`, `UiSettings.language`; the pixel font's letters past ASCII), `argus-render` (those letters drawn), `argus-luau` (`q:text`, `q:languages`), `argus-input` (the rebind's prompt from the tables), `argus-scene` (`[strings] base`), `argus-player` (tables and dialogue in the one assembly; a language's table in its own pack, every language mounted, the newest pack of a language the language), the daemon (`strings.list`; `edit.text`, a string changed in a transaction; dialogue rows and tables that do not read in `script.errors`), and the CLI (`query strings`, `edit` op `text`). Every player-facing word of the three games with words is a key now. Hollow's menus, signs, and HUD are in a string table, with a French one beside it, and Settings has a Language button that switches the whole game between them at once. Hollow gained a hermit at the cave mouth: E by him opens a conversation from `defs/dialogue.toml`, played in a box of UI buttons with the menu keys. He greets a first visit and a second one differently (a branch on whether he has met the hero), answers where the cave leads, and is offered the gem only once it has been found and until it has been given (a condition), and remembers both (effects). The game stands still while he talks, and his lines play a voice on a dialogue bus that ducks the music, section 13's own rule. Delve's menu and HUD line are keys, and it gained a message line that says what each turn did ("The rat hits you for 1."), from its events; Outpost's HUD is keyed and gained a notice line (a day begun, a colonist lost). A release build puts each language other than the base in its own pack, so French is added to a shipped game by dropping in `lang-fr.pack`. Found while building it: the pixel font drew only ASCII, so French showed `?` for every accent; it has 35 more letters now. Two builds' French packs listed French twice and the older one won; the newest pack of a language is the language. A picture of the French conversation showed the box hiding the hero and the hermit along the bottom of the screen; it sits at the top. Traces: `crates/argus-dialogue/SPEC-TRACE.md`, and rows in `argus-ui`'s, `argus-render`'s, `argus-luau`'s, `argus-input`'s, `argus-scene`'s, `argus-player`'s, the daemon's, and `argus-cli`'s. Constraints:

- §12: dialogue is a def table (§4) with a small node graph: lines, choices, conditions on world state, and effects as edits. It runs from Luau and is inspectable through `world.defs`.
- §12: all player-facing strings go through a string table keyed by id; the agent can author in one language and a translator can fill the others without opening the engine.
- §15: localization packs are separate, so a language can be added without a rebuild (phase 2 item 7 lays whole files over the game's; string tables replace that for text).
- §13 audio: "dialogue ducks music by 6 dB over 200 ms" is a ducking rule, so dialogue plays on a bus.
- §13: timelines have dialogue-line tracks (item 5).
- The customers: Delve's menus and messages, Hollow's signs, Outpost's event text.

Decisions taken while building it:

- **A language is a file of keys; the base is the fallback, key by key.** `strings/<code>.toml`, TOML's own tables or dotted keys; a key a language lacks shows in the base language, and a key no table has shows as `[key]`, which is what a missing translation looks like on screen. `strings.list {lang, missing: true}` is the translator's to-do list.
- **The language is a setting in the world.** `UiSettings.language` (empty: the base) is world state like the text size, so a replay, a seek, or a save shows the words the player saw. A UI text names a key and is looked up every layout, so a language chosen shows at once, a conversation in progress included.
- **The semantic frame's `Describe` texts stay in the base language.** They are the agent's reading of the world; what the player reads (the UI, drawn words) is in the player's language, and the frame's `ui` section reports those as drawn.
- **Dialogue is a def table the engine declares.** Every project has a `dialogue` table; its rows are nodes, choices (a row from a node with a line), and branches (one without). Lines and names are string keys, so the graph is the same in every language and a translator never opens it.
- **Conditions and effects are small languages, not Luau.** `when = "@hud:Run.gems >= 1 and not speaker:Hermit.has_gem"`, `effects = "speaker:Hermit.met = true; emit hermit.given"`: data an agent can read and change with `world.defs` and `edit`, checked when the table is read, and an edit that cannot be made is an event, not a fault.
- **A conversation is an entity; its box is ordinary UI.** A `Talk` holds where it is; its box is UI nodes with buttons, so the player's actions, the mouse, focus, the theme, and the semantic frame all work as they do for menus. A game that wants its own look reads the `Talk` and sets `pick` and `advance`.
- **Strings are edited as transactions.** `edit.text` is an op like a tile edit: kept at commit, in the change feed by author, undone with `edit.undo`.
- **The pixel font grew, rather than French moving to another font.** 35 letters past ASCII in the same 5 x 7 style; Hollow keeps its look in both languages.

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
