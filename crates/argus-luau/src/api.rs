//! The scripting API's reference (§16: "the engine's own reference
//! documentation ships in the same shape"): every declaration, key, field,
//! and method a game's Luau can use, with what it does. `argus docs` and
//! each project's ARGUS.md print it. A test checks it names everything the
//! binding registers, so a method added without a line here fails CI.
//!
//! Found at the phase 5 gate: four agents, each adding a feature to a game
//! with only the project, the tools, and their docs, all learned the
//! scripting API from the games' own scripts, because nothing described it.

/// One name and what it does.
pub struct ApiDoc {
    /// How it is written: `q:each(...)`, `field.f32 {...}`, `stage`.
    pub name: &'static str,
    /// What it does, its arguments, and what it gives back.
    pub doc: &'static str,
}

/// A part of the API.
pub struct ApiSection {
    /// Its heading.
    pub title: &'static str,
    /// A paragraph before its entries.
    pub intro: &'static str,
    /// Its entries.
    pub entries: &'static [ApiDoc],
}

const fn d(name: &'static str, doc: &'static str) -> ApiDoc {
    ApiDoc { name, doc }
}

/// The scripting API, part by part.
pub const SECTIONS: &[ApiSection] = &[
    ApiSection {
        title: "Files and the sandbox",
        intro: "A game's Luau lives in `components/`, `systems/`, and `defs/` (each file returns one declaration or a table of them), and in mods' same folders; code they share is in modules in `lib/`, which they `require`. Scripts run sandboxed: globals are read-only once a file has loaded, and there is no `os`, `io`, clock, or device (a script reads time as `q.time` and input as the `Actions` component, so a run replays). Luau's `math` (doubles), `string`, `table`, and `vector` are there; a vec2 field is a Luau vector (`vector.create(x, y, 0)`, `.x`, `.y`). An instruction budget stops a runaway system with a `script.errors` row. `argus script reload` reloads changed files while the game runs; a component whose fields changed is refused there, because a running world's rows cannot change shape: `argus scene load` starts the scene again with the new schema, and `version` and `migrate` (below) carry old saves over.",
        entries: &[
            d("require(\"name\")", "A module: `lib/<name>.luau` (`require(\"rules/checks\")` is `lib/rules/checks.luau`), a Luau file that returns a value, usually a table of constants and functions. Evaluated once per load; every file that requires it gets the same value, read-only (every table in it too). It runs in the sandbox, its calls counting against the calling system's budget, and reads the world only through what it is handed (`rules.hit(q, e, n)`). A mod's files look in `mods/<id>/lib/` first, then the game's `lib/`; the game's only in the game's. A missing module, or modules that require each other in a circle, are errors naming the files; a reload with a module changed reloads every file that uses it. Its leading `--` comment is its description in ARGUS.md and `script.modules`."),
        ],
    },
    ApiSection {
        title: "Declarations",
        intro: "What a file returns. Names are global: a component's is its type (`Health`), a system's its id (`sim.player_turn`), a def table's its name (`item`).",
        entries: &[
            d("component \"Name\" { ... }", "A component: its fields as `name = field.<type> {...}`, and the keys below. Fields keep the order they are written in."),
            d("doc", "component, system, def: what it is, shown in ARGUS.md, `world describe`, and the editor."),
            d("version", "component: its schema version, 1 when absent. Adding, removing, renaming, or retyping a field changes the schema: `script reload` refuses it while the world runs (`argus scene load` starts over with it), and nothing more is needed until a save holds the component. To keep saves made before loading, raise the version and give `migrate` a step for it."),
            d("migrate", "component: how a saved row of an older version becomes this one, when a save, mark, or report from before loads. One function `function(row, from) ... return row end` (`from` the saved version), or a step per version from 2 up to `version`: `{ [2] = { rename = { hp = \"cur\" }, default = { max = 10 }, drop = { \"mood\" } }, [3] = function(row) ... return row end }`. A saved row runs the steps above its version, in order; a step for a version outside 2 to `version` is an error."),
            d("field.<type> { default, range, doc, tune }", "A field. Types: bool, u8, u16, u32, u64, i8, i16, i32, i64, f32, f64, vec2, str, entity, ref (a def row's id, `item:bread`). `default` is a new row's value (a vec2's is `{x, y}`: `field.vec2 { default = {40, 24} }`); `range = {lo, hi}` bounds it for the editor and the tuning panel; `tune = true` puts it in the tuning panel and `tune.fields`; `doc` says what it is."),
            d("field.list { of, default, doc }", "A list field: values of one type in order, any of the types above but entity (`of = \"ref\"`), at most 1,024 of them; no range or tune. In an entity file or a def row, an array (`pool = [\"monster:rat\", \"monster:bat\"]`, a vec2 element `[x, y]`); every element of a list of refs must name a row that exists, checked at load. A list is one value: reading it gives a read-only array (`#`, `ipairs`, `table.find`), and changing it is assigning a new array (`local o = table.clone(t.on); table.insert(o, \"burning\"); t.on = o`). A table with keys, or an element of the wrong type, is refused, naming the element. A mod's def row adds to a list with `\"+field\" = [...]` and takes from it with `-field = [...]`."),
            d("system \"id\" { ... }", "A system: `stage` and `run` are required; the other keys below are optional. A key a system does not take is an error."),
            d("stage", "system: input (devices into actions), sim (game logic), late (after the sim settles: a camera, a HUD's text), hierarchy (the engine's transform propagation), or present (drawing: read-only, a write there is refused)."),
            d("run = function(q) ... end", "system: called once a tick (once a turn on a discrete clock) with `q`, the system's view of the world (below)."),
            d("after", "system: ids of systems this one runs after, in the same stage; each must exist. Systems not ordered by `after` or `before` have no promised order between them (a reload can move them); `argus script systems` shows the order now."),
            d("before", "system: ids of systems this one runs before, in the same stage; each must exist. For a system you cannot change, an engine one among them: a look system that sets an animator's inputs says `before = { \"anim.update\" }`, so the frame changes the same tick."),
            d("reads, writes", "system: the components it reads and writes (`{\"Transform\", \"Health\"}`). Reading or writing one it did not declare is an error at run time (\"writes Health without declaring it\"), and the declarations decide which systems may run together; `obs.why` reads them."),
            d("spawns", "system: the prefabs it spawns with `q:spawn_prefab` (`spawns = {\"bullet\", \"enemy/bat\"}`, each a file under `prefabs/`). Naming a prefab grants its components, as a whole: they need not be in `writes`, and a component the prefab gains later needs no change here. Spawning one it does not name is an error."),
            d("rng", "system: the name of its random stream (`rng = \"loot\"`). The stream is seeded from the run's seed and its name only, so adding or reordering systems never changes another system's numbers; `q:rng()` draws from it."),
            d("unfrozen", "system: `true` runs it on ticks a `q:freeze()` stood still (a pause menu, the UI)."),
            d("between_turns", "system: `true` makes it move between turns in a turn-based scene: it also runs on the idle ticks while the game waits for a turn (`q.dt` 1/60), and on a turn it takes no time (`q.dt` 0). What the engine moves between turns already: animators, particles, tweens, the camera, timelines, post effects."),
            d("on_load", "system: `true` runs it once when the scene loads, before the first tick (a generated floor); its events belong to the run."),
            d("clock", "system: driving (the default: the clock the scene declares), continuous, or discrete; a scene with both clocks runs the system on that one. On a discrete clock a turn passes for each press of any bound action (a release is nothing), each turn a scenario or `sim.advance` asks for, and nothing else: a press the game should not count as a turn (the one that opens or closes a menu) is one its systems stand still with `q:freeze()`."),
            d("def \"table\" { ... }", "A data table, its rows in `defs/<table>.toml`: its fields as `field.<type>`, with `doc`, `key` (the field that names a row, `id` by default), and `prefix` (row ids are `<prefix>:<key>`, the table's name by default). Rows are read with `q:def` and `q:defs`, and edited with `argus edit` def ops."),
        ],
    },
    ApiSection {
        title: "q: a system's view of the world",
        intro: "The argument `run` receives. Its methods (`q:name(...)`) and fields (`q.name`):",
        entries: &[
            d("q.tick", "The tick (or turn) number."),
            d("q.time", "Sim seconds since the run began. It runs on through ticks a `q:freeze()` stood still (a pause stops the game's systems, not the clock), and so do `q.day` and `q.day_time`: a game with a pause keeps its own time in a component its systems advance."),
            d("q.dt", "Sim seconds this tick covers (1/hz on a continuous clock)."),
            d("q.day, q.day_time, q.day_length", "The game's day (`[time] day` in argus.toml, seconds): which day it is from 0, how far through it (0 to 1), and its length in seconds."),
            d("q.name", "This system's id."),
            d("q:each(\"A\", \"B\", ...)", "Every entity with all the named components: `for e, a, b in q:each(\"A\", \"B\") do ... end`, `e` the entity and `a`, `b` the components (fields read and written as `a.field`). The loop hands out one reused handle for `e` and one for each component, moved to the next entity each time round: to keep an entity past its turn in the loop, keep `e:id()` and get it back with `q:entity(id)`."),
            d("q:count(\"A\", ...)", "How many entities have all the named components. Like q:each, it passes by inactive entities."),
            d("q:inactive(\"A\", ...)", "As q:each, over the entities with all the named components that are switched off (e:set_active(false)) or under one that is: what a game switches on again. q:each and q:count skip them."),
            d("q:entity(ref)", "An entity by reference: `@id` (an authored or scene id), `e:12`, or a full id. An error when there is none."),
            d("q:find(ref)", "`q:entity(ref)`, or nil when there is none (an id no entity has, an `e:` ref no longer alive): `local door = q:find(\"@door.east\")`. A ref that is not one, and a short id several rooms share, are still errors."),
            d("q:get(e, \"A\")", "Entity `e`'s component `A` (as `e:get(\"A\")`)."),
            d("q:spawn(what)", "A new entity: `q:spawn()` a bare one; `q:spawn(\"id\")` with that string id, found with `q:entity(\"@id\")` and counted with the authored entities (an id another entity has is an error, DUPLICATE_ID, that names it; `q:find` asks first); `q:spawn({\"A\", \"B\"})` with those components at their defaults (one placement, the fast way). Returns the entity; set its fields with `e:get(\"A\").f = v`."),
            d("q:spawn_many(n, {\"A\", ...}, function(e, i) ... end)", "n entities in one call, each handed to the callback (through one reused handle: keep `e:id()`, not `e`)."),
            d("q:spawn_prefab(name, fields)", "A new entity from a prefab (`prefabs/<name>.toml`, its whole chain), with its children under it (each with a `Parent`, and a `GlobalTransform` beside its `Transform`), every field as the prefab gives it and then `fields`: `q:spawn_prefab(\"bullet\", { [\"Transform.pos\"] = p, [\"Bullet.dir\"] = d })`. Returns the entity; its children are runtime entities, found through `Parent`. The system must name the prefab in its `spawns`. A prefab's entity field may name an authored entity (`\"@player\"`), which must be in the world when it spawns. A prefab changed by an edit or `scene load` spawns as it is now."),
            d("q:despawn(e)", "Remove an entity now (as `e:despawn()`). Inside a `q:each` over the same components, collect the ids first and despawn after the loop."),
            d("q:within(pos, r, \"A\", ...)", "Like `each`, over the entities within `r` of `pos` (a vector, table, or entity) that have the named components, nearest first, the distance last: `for e, a, d in q:within(p, 3, \"A\")`. Uses the shared spatial index (Transform), rebuilt from the positions as they are when a tick ends and again by `physics.overlap` at the end of the sim stage: a `sim` system sees where things were when the last tick ended, so an entity spawned or moved earlier this tick is found at its new place by `late` systems and from the next tick. Despawned entities are left out."),
            d("q:nearest(pos, r, \"A\", ...)", "The closest such entity within `r`, and its distance; nil when there is none."),
            d("q:raycast(from, to, opts)", "The nearest collider or tile cell the line from `from` to `to` meets, or nil: a table with `entity` (or `tile`, `{map, x, y}`), `point`, `normal` (the surface's, outward; zero when the line starts inside it), `distance`, and `fraction` (0 to 1 along the line). It meets every active Collider whose layer shares a bit with `opts.mask` (all by default) and every Tilemap's collision cells (solid, slopes, and one-way platforms only from above). `opts`: `mask`, `ignore` (an entity or a list), `one_way = false` (one-way platforms are not there), `tiles = false`, `colliders = false`. Points are vectors, tables, or entities. The system declares reads of Collider and Tilemap (or leaves one out with its option)."),
            d("q:raycast_all(from, to, opts)", "Every hit `q:raycast` would see along the line, nearest first."),
            d("q:shape_cast(shape, from, to, opts)", "Where a shape moving from `from` to `to` first touches a collider or tile cell, or nil: a hit as `q:raycast`'s, with `position` (where the shape stops) and `point` (where they touch). A shape is `{ radius = r }` (a circle), `{ size = v }` (a box, its corners rounded by a `radius` too), `{ radius = r, height = h }` (a capsule standing up, `h` its whole height), or `{ points = { v, ... } }` (a convex polygon around its origin). Touching is not a hit unless the shape moves into it."),
            d("q:overlap(shape, at, opts)", "Every collider and tile cell a shape (as `q:shape_cast`'s) overlaps when it is at `at`, by more than touching; one-way platforms are not solid to it. `{ radius = 0 }` asks of a point."),
            d("q:draw_line(from, to, opts)", "Draw a line in the world for debugging, from `from` to `to` (vectors, tables, or entities). A system's shapes stay up until it runs again, so a turn-based game's last ones stay between turns; they are drawn over the world and under the UI in the window, the editor, and a picture (`obs.frame`, whose semantic frame lists them under `debug`), and never in a release build, where a draw does nothing. They are not part of the world: no digest, keyframe, or replay holds them. `opts`: `color` (`\"#rrggbb\"` or `\"#rrggbbaa\"`, amber by default), `width` (UI pixels, 2 by default, whatever the zoom). A system keeps at most 4096 shapes a run."),
            d("q:draw_arrow(from, to, opts)", "`q:draw_line` with a head at `to`: a velocity, a facing, an aim."),
            d("q:draw_box(at, size, opts)", "Draw a box for debugging, centred on `at`, `size` its whole width and height in world units. `opts`: `color`, `width`, and `fill = true` for a solid one. As `q:draw_line`."),
            d("q:draw_circle(at, r, opts)", "Draw a circle for debugging, centred on `at`, radius `r` in world units. `opts`: `color`, `width`, `fill = true`. As `q:draw_line`."),
            d("q:draw_text(at, text, opts)", "Draw words for debugging, centred on `at` (an entity, for a label over it), on a dark backing. `opts`: `color`, `size` (UI pixels, 14 by default). As `q:draw_line`."),
            d("q:screen_to_world(p)", "A point on the UI canvas (the `pointer` action's pixels) in the world, through the drawing camera, as a vector; nil with no camera. What was clicked: `local at = q:screen_to_world(act.pointer)`, then `map:cell(at)` for its cell or `q:nearest(at, r, ...)` for the entity there."),
            d("q:world_to_screen(p)", "A world point, or an entity's place, on the UI canvas in its pixels: `q:screen_to_world` the other way. Nil with no camera."),
            d("q:emit(name, fields)", "An event of this tick: `q:emit(\"potion.drunk\", { healed = 4, hp = e })`. Fields are numbers, strings, booleans, entities, and vectors; none is called `name`, which is the event's own name (`q:emit` refuses one: call it `who`, `title`, or `what`). The event log, `obs.events`, a scenario's `t.expect:events`, and later systems' `q:events` see it; the name is the game's to choose. An entity field is read by its id (`\"@colonist.ada\"`) while the entity lives and by its handle (`\"e:6\"`) once it has despawned, so an event about an authored entity that may go (a death) carries its name too, as a string: `{ who = e, name = e:name() }` (`colonist.ada`)."),
            d("q:send(channel, text)", "Text for whatever hosts the game: a `host.send` event `{channel, text}` (`q:send(\"gm.request\", digest)`). A release build's page reads it with `player.take_sent()`; the daemon's `events` and `obs.subscribe` see it like any event. A reply comes back as a `host.message` event (see `input.message`). The channel is a short name the game chose (letters, digits, `.`, `_`, `-`, up to 64); a text is up to 64 KiB."),
            d("q:events(name)", "The events emitted so far this tick with that name (every one for no name), oldest first, each a table of its fields plus `name`, the event's name, which no field has. This tick only: a system sees what systems before it emitted this tick (order them with `after`), and an event from an earlier tick is gone (keep what must last in a component). `argus events name=job.done limit=1` shows any event's fields, the engine's included."),
            d("q:freeze()", "The rest of this tick stands still but for `unfrozen` systems (a pause, a hit-stop), plugin systems included: on a discrete clock the rest of that turn's systems do not run. A `host.message` on that tick waits for each system it stopped, until it next runs."),
            d("q:save(name, summary)", "Ask whatever runs the game to save it, between ticks: `q:save()` is the save \"auto\". `summary` is the game's own fields for a slot menu to show (`{ level = 4, place = \"Hall\" }`: words, numbers, and true or false), which `q:saves()` gives back. The save holds the world as this tick ends; the daemon keeps saves in `.argus/saves/`, a release build where the platform says, the web page in the browser's storage, and each answers with a `game.saved {save}` event. (A `save.requested {save}` event.)"),
            d("q:saves()", "The saves there are, newest first: each `{name, saved_at, scene, tick, time, summary}`, `saved_at` in seconds since 1970 and `summary` the fields `q:save` was given. What a tick reads is recorded with the run's input (it needs input.toml), so a replay, a seek, or a report reads the same list whatever the disk holds now; while `argus test` runs scenarios the list starts empty."),
            d("q:delete_save(name)", "Ask for a save to be deleted after this step, from the disk and the platform's cloud and from what `q:saves()` lists. Answered with `game.save_deleted {save}`. (A `save.delete_requested {save}` event.)"),
            d("q:load(name)", "Ask for a save to be loaded after this step (`q:load()` loads \"auto\"); its components migrate as they declare. Answered with `game.loaded {save}`. (A `load.requested {save}` event.)"),
            d("q:net()", "The match, as this machine sees it: `{state, code, slot, players, present, why}`. `state` is `alone` (no room), `asking` (waiting for the relay), `room` (in a room: `code` is what the others join with, `slot` this player's, `present` the slots whose players are there), `playing` (a match: only `players`, since every player's world must be the same, so a script never reads which player this machine is), or `ended` (`why` says why: a player left, the worlds fell out of step, the relay closed). A match starts on its own once every slot is taken: every player loads the host's scene afresh, from the host's seed. What a tick reads is recorded with the run's input, as `q:saves()` is."),
            d("q:net_host(players, scene)", "Ask whatever runs the game to host a room on its relay for `players` (2 to 4; 2 when left out), whose match starts on `scene` (the current one when left out). `q:net()` then gives the room's code. (A `net.host_requested {players, scene}` event.)"),
            d("q:net_join(code)", "Ask to join the room with this code (a `net.join_requested {code}` event); `q:net()` says how it went."),
            d("q:net_leave()", "Leave the room, or the match (a `net.leave_requested` event)."),
            d("q:speed(x)", "`q:speed(3)` asks for 3 ticks a frame from the next frame (a `speed.requested` event, in the run, so replays agree); `q:speed()` is the clock's speed as this tick began, however it was set. Never 0: pause with `q:freeze()`."),
            d("q:rng()", "This system's random stream (its `rng` key; an error without one): `rng:next()`, `rng:below(n)`, `rng:range(lo, hi)`, `rng:chance(p)` (below)."),
            d("q:rng_from(a, b, ...)", "A generator of its own, seeded from these whole numbers alone, with the same four calls: the same numbers draw the same values on every machine, whatever was drawn first. For generation a game seeds itself: `local r = q:rng_from(run.seed, floor, room)` gives each room the same contents in whatever order rooms are visited. Make one where it is used, not in a file-level local; needs no `rng` key."),
            d("q:tiles(map)", "A Tilemap's cells (`map` its entity or `@id`): `map:get`, `map:set`, `map:fill`, `map:paint`, ... (below). A system that reads cells declares reads of Tilemap; one that writes them, writes."),
            d("q:path(from, to, grid)", "A path over a NavGrid (the scene's, or `grid` an entity): its waypoints (vectors) and its cost in steps, or nil. Points are vectors, tables, or entities. The system declares reads of NavGrid."),
            d("q:walkable(pos, grid)", "Whether a point is on a walkable cell."),
            d("q:distance(field, pos)", "Steps from `pos` to a NavField's goal (`field` the NavField's entity), or nil. The system declares reads of NavField."),
            d("q:downhill(field, pos)", "The next cell's centre toward a NavField's goal, or nil: one step of a flow field."),
            d("q:tween(e, \"Comp.field\", opts)", "Animate one field of `e`: `{to = v, from = v, duration = 0.15, easing = \"out_back\", on_complete = \"squash.done\"}` (`on_complete` an event to emit). Spawns a Tween entity, so declare writes of Tween; returns it."),
            d("q:play(sound, fields)", "Ask the mixer for a sound (`audio.toml`'s name): `{entity = e, gain = 0.8, pitch = 1.2}`. A `sound.play` event; the host answers with `sound.played`."),
            d("q:text(key, args)", "A player-facing string in the run's language: `q:text(\"hud.day\", {day = 3})`, its `{name}` holes from `args` and its `{@id:Comp.field}` holes from the world; `[key]` when no string table has it."),
            d("q:languages()", "The languages there are string tables for, the base first."),
            d("q:def(id)", "A data table's row, read-only: `q:def(\"item:bread\").name`."),
            d("q:defs(table)", "A table's rows in file order: `for _, r in q:defs(\"recipe\") do ... end`."),
            d("q:achieve(id, current, max)", "Unlock an achievement `platform.toml` declares, or with `current, max` show progress. A request event the host hands the platform (Steam, or the null one) between ticks; nothing comes back into the world."),
            d("q:presence(key, value)", "Set (or with no value clear) a rich presence key on the platform."),
            d("q:overlay(page)", "Open the platform's overlay at a page (`friends` by default)."),
        ],
    },
    ApiSection {
        title: "Entities and components",
        intro: "What `q:each`, `q:entity`, and `q:spawn` give back.",
        entries: &[
            d("e:id()", "Its id as text (`e:12`): what to keep across ticks or hand to `q:entity`. `tostring(e)` is the same; `e1 == e2` compares entities."),
            d("e:name()", "Its string id (`player`, `door.west`), or nil."),
            d("e:alive()", "Whether it is still in the world."),
            d("e:set_active(on)", "Switch it off (false) or on (true), and everything under it. Switched off, it stays in the world (saved, seekable) but q:each, drawing, physics, the UI, and the semantic frame pass it by. The system declares writes = { \"Inactive\" }. An entity file starts one off with tags = [\"Inactive\"]."),
            d("e:active()", "Whether it is in the game: alive, not switched off, and not under an entity that is."),
            d("e:has(\"A\")", "Whether it has component A."),
            d("e:get(\"A\")", "Its component A: read and write fields as `c.field` and `c.field = v` (a vec2 is a vector; an entity field an entity; a str or ref field a string). An error when it has none."),
            d("e:insert(\"A\", fields)", "Add component A at its defaults, then the given fields: `e:insert(\"Potion\", { heal = 4 })`."),
            d("e:remove(\"A\")", "Remove component A."),
            d("e:despawn()", "Remove the entity now (see `q:despawn`)."),
        ],
    },
    ApiSection {
        title: "Random streams",
        intro: "`q:rng()`, the system's own stream: the same seed gives the same numbers, whatever else changed.",
        entries: &[
            d("rng:next()", "A number in [0, 1)."),
            d("rng:below(n)", "An integer from 0 to n - 1."),
            d("rng:range(lo, hi)", "An integer from lo to hi, both included."),
            d("rng:chance(p)", "True with probability p."),
        ],
    },
    ApiSection {
        title: "Tile maps",
        intro: "`q:tiles(map)`: cells counted from the map's top-left, rows down; a tile is a number, 0 for none.",
        entries: &[
            d("map:size()", "Its width and height in cells."),
            d("map:layers()", "Its layers' names, in order."),
            d("map:get(layer, x, y)", "One cell's tile; 0 off the map, on every side."),
            d("map:cell(pos)", "The column and row a world point is in (a vector, table, or entity)."),
            d("map:center(x, y)", "That cell's centre in the world."),
            d("map:set(layer, x, y, tile)", "One cell (a runtime map's cells are the run's, in its digest and saves)."),
            d("map:fill(layer, x, y, w, h, tile)", "A rect of one tile (0 clears)."),
            d("map:copy(layer, x, y, w, h, to, to_layer, tx, ty)", "A rect of cells onto another map (`to`, from `q:tiles`; nil: this one) and layer (nil: the same name), its top-left at `tx, ty` (nil: `x, y`): a room kept on a storage map and shown on the drawn one, ten layers in ten calls. Cells off this map copy as 0. Writes Tilemap."),
            d("map:paint(layer, x, y, rows, legend)", "Rows of characters, each a cell whose tile `legend` gives: `map:paint(\"ground\", 0, 0, {\"###\", \"#.#\"}, {[\"#\"] = 1, [\".\"] = 0})`."),
        ],
    },
];

/// The reference as Markdown, headings from `level`.
pub fn markdown(level: usize) -> String {
    let h = "#".repeat(level);
    let mut o = String::new();
    for s in SECTIONS {
        o.push_str(&format!("{h} {}\n\n{}\n\n", s.title, s.intro));
        for e in s.entries {
            o.push_str(&format!("- `{}`: {}\n", e.name, e.doc));
        }
        if !s.entries.is_empty() {
            o.push('\n');
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every name the binding registers on a type, by reading its source:
    /// `add_method("x"`, `add_method_mut("x"`, `add_field_method_get("x"`.
    fn registered(src: &str, ty: &str) -> Vec<String> {
        let at = src
            .find(&format!("impl UserData for {ty}"))
            .unwrap_or_else(|| panic!("no UserData for {ty}"));
        let body = &src[at..];
        let body = &body[..body[1..].find("\nimpl ").map_or(body.len(), |i| i + 1)];
        let mut out = Vec::new();
        for pat in ["add_method(", "add_method_mut(", "add_field_method_get("] {
            for (i, _) in body.match_indices(pat) {
                let rest = body[i + pat.len()..].trim_start();
                if let Some(r) = rest.strip_prefix('"') {
                    out.push(r[..r.find('"').unwrap()].to_string());
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    fn documented(prefix: &str) -> String {
        SECTIONS
            .iter()
            .flat_map(|s| s.entries.iter().map(|e| e.name))
            .filter(|n| n.contains(prefix))
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn every_method_and_field_a_script_can_call_is_in_the_reference() {
        let src = include_str!("bind.rs");
        // Methods made in a loop (`for (method, event) in [("load", ..)]`)
        // are not add_method("..") literals: q:save and q:load went
        // undocumented that way until the manual's writers found them.
        for (n, event) in [
            ("load", "load.requested"),
            ("delete_save", "save.delete_requested"),
        ] {
            assert!(
                src.contains(&format!("(\"{n}\", \"{event}\")")),
                "{n} is still made in the loop this test knows"
            );
            assert!(
                documented("q:").contains(&format!("q:{n}(")),
                "q:{n} is in the reference"
            );
        }
        for (ty, method, field) in [
            ("QueryHandle", "q:", "q."),
            ("EntityRef", "e:", "e."),
            ("RngRef", "rng:", "rng."),
            ("TilesRef", "map:", "map."),
        ] {
            let names = registered(src, ty);
            assert!(!names.is_empty(), "{ty}");
            let (m, f) = (documented(method), documented(field));
            for n in names {
                assert!(
                    m.contains(&format!("{method}{n}(")) || f.contains(&format!("{field}{n}")),
                    "{ty}'s {n} is not in the scripting reference (api.rs)"
                );
            }
        }
    }

    #[test]
    fn every_declaration_key_and_field_type_is_in_the_reference() {
        let all: String = SECTIONS
            .iter()
            .flat_map(|s| s.entries.iter().map(|e| format!("{} {}", e.name, e.doc)))
            .collect::<Vec<_>>()
            .join("\n");
        for k in crate::decl::SYSTEM_KEYS {
            assert!(all.contains(k), "system key {k}");
        }
        for t in crate::decl::FIELD_TYPES {
            assert!(all.contains(t), "field type {t}");
        }
        for k in [
            "version", "migrate", "default", "range", "tune", "prefix", "key",
        ] {
            assert!(all.contains(k), "{k}");
        }
    }
}
