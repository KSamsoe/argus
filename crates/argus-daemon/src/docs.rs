//! The method table. `session.capabilities` and `session.docs` serve it,
//! the CLI's help and the MCP tool descriptions are generated from it
//! (§16), so a method is described exactly once.

/// One protocol method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MethodDoc {
    /// Dotted name (`world.query`).
    pub name: &'static str,
    /// One or two sentences on what it does.
    pub doc: &'static str,
    /// Parameters, as `name: type (note)` items separated by `; `.
    pub params: &'static str,
    /// What the result holds.
    pub returns: &'static str,
}

/// Every method the phase-0 daemon answers, in namespace order.
pub const METHODS: &[MethodDoc] = &[
    MethodDoc {
        name: "session.open",
        doc: "Open a project and load a scene into the one world this daemon holds. Scripts in components/, defs/, systems/ load first, then prefabs and defs, then the scene.",
        params: "project: string (directory with argus.toml); scene: string (default from argus.toml); seed: integer (default: the scene's seed policy)",
        returns: "the world.summary, plus issues: [] when the scene could not be instantiated",
    },
    MethodDoc {
        name: "session.status",
        doc: "What is open: project, scene, seed, clock, and whether the sim is playing.",
        params: "",
        returns: "{project, scene, seed, clock, playing, paused, tick, time, frame}",
    },
    MethodDoc {
        name: "session.capabilities",
        doc: "The daemon's version and every method it answers, with parameter docs.",
        params: "",
        returns: "{version, phase, transports, methods: [{name, doc, params, returns}]}",
    },
    MethodDoc {
        name: "session.docs",
        doc: "The method reference as text, for one method or all of them.",
        params: "method: string (optional)",
        returns: "{text}",
    },
    MethodDoc {
        name: "session.close",
        doc: "Drop the project, scene, and world. The daemon keeps running.",
        params: "",
        returns: "{closed: true}",
    },
    MethodDoc {
        name: "session.shutdown",
        doc: "Close the project and exit the daemon process after replying. `argus daemon stop` is this.",
        params: "",
        returns: "{shutdown: true}",
    },
    MethodDoc {
        name: "world.summary",
        doc: "One-screen orientation: entity counts by archetype, named entities, active systems, frame, seed, clock. The first call to make on connect. Served from the index (same shape, source: \"index\") for a scene that is not loaded.",
        params: "scene: string (optional: another scene, from the index); source: \"index\" (force the index)",
        returns: "{frame, tick, time, seed, clock, playing, scene, rooms, entities, authored, archetypes: [[components, count]], named: [[id, components]], systems: [name], disabled: [name]}",
    },
    MethodDoc {
        name: "world.query",
        doc: "Select field paths from entities matching a filter. Rows are compact arrays in select order; total is always present; next is a cursor for the rest.",
        params: "select: [\"id\" | \"Component.field\" | \"Component\"] (default [\"id\"]); where: {has: [Component], not: [Component], within: {of: entity, r: number}, \"Component.field\": value | {eq|ne|lt|lte|gt|gte: value}}; order: \"Component.field\" | \"-Component.field\" | \"dist(entity)\"; limit: integer (default 50); cursor: string; scene: string (optional: another scene, from the index); source: \"index\"",
        returns: "{frame, total, rows: [[...]], truncated, next}",
    },
    MethodDoc {
        name: "world.get",
        doc: "One entity in full. resolved: false adds the entity's own file overrides; resolved: true adds the merged prefab result with each field's provenance (own, a prefab path, or default).",
        params: "entity: string (\"@id\" or \"e:n\"); resolved: boolean (default false); scene: string (optional); source: \"index\"",
        returns: "{entity, id, authored, components: {Name: {field: value}}, overrides | resolved}",
    },
    MethodDoc {
        name: "world.describe",
        doc: "Component schemas with doc strings, field types, defaults, ranges, tune flags, and declaring file:line, plus which systems read and write them.",
        params: "component: string (optional; all when omitted); limit; cursor; source: \"index\"",
        returns: "{total, rows: [{name, doc, version, source, fields: [{name, type, default, range, doc, tune}], written_by, read_by}], next}",
    },
    MethodDoc {
        name: "world.who_writes",
        doc: "The systems that declare a write to a component, in run order: what could have changed it.",
        params: "component: string; source: \"index\"",
        returns: "{component, systems: [name], readers: [name]}",
    },
    MethodDoc {
        name: "world.defs",
        doc: "Data tables. Without a table, list them; with one, its rows filtered by field equality.",
        params: "table: string (optional); where: {field: value}; limit; cursor; source: \"index\"",
        returns: "{tables: [...]} or {table, columns, total, rows: [[...]], next}",
    },
    MethodDoc {
        name: "world.refs",
        doc: "Everything in the project that references a target: an entity (@id), a prefab path, a def ref (item:key), or a file path. From the index.",
        params: "target: string; limit; cursor",
        returns: "{target, total, columns, rows: [[scene, from_kind, from, path, kind, at]], next}",
    },
    MethodDoc {
        name: "world.search",
        doc: "Full-text search over component, field, and system doc strings. From the index.",
        params: "text: string; limit; cursor",
        returns: "{total, columns, rows: [[kind, name, doc]], next}",
    },
    MethodDoc {
        name: "world.instances",
        doc: "Every entity in every scene whose prefab chain includes a prefab. From the index.",
        params: "prefab: string; limit; cursor",
        returns: "{prefab, total, columns, rows: [[scene, entity]], next}",
    },
    MethodDoc {
        name: "scene.list",
        doc: "Scenes in the project (scenes/<name>/scene.toml).",
        params: "",
        returns: "{scenes: [name], current}",
    },
    MethodDoc {
        name: "scene.load",
        doc: "Reload the current scene, or switch to another one. Re-reads scripts, prefabs, defs, and files, and rebuilds the world.",
        params: "scene: string (default: current); seed: integer",
        returns: "the world.summary, plus issues when the scene is not clean",
    },
    MethodDoc {
        name: "scene.issues",
        doc: "Everything wrong with the current scene's files, each with file:line. Empty when the scene is loaded.",
        params: "",
        returns: "{total, rows: [{code, message, hint, ref}]}",
    },
    MethodDoc {
        name: "scene.rooms",
        doc: "Rooms of the current scene with graph neighbours, bounds, entity counts, loaded state, and which is current.",
        params: "scene: string (optional: another scene, from the index); source: \"index\"",
        returns: "{rooms: [{name, loaded, entities, neighbors, bounds, bulk}], current, radius}",
    },
    MethodDoc {
        name: "scene.index",
        doc: "Bring .argus/index.db up to date (files changed outside the daemon are picked up; rebuild re-indexes everything) and report its size.",
        params: "rebuild: boolean (default false)",
        returns: "{scenes, entities, failed, ms, stats}",
    },
    MethodDoc {
        name: "asset.move",
        doc: "Rename a prefab, entity file, def row, or asset and rewrite every reference to it; appends to .argus/moves.log. The only sanctioned way to rename.",
        params: "from: string (project path, or defs/x.toml#key); to: string",
        returns: "{kind, from, to, refs, rewritten: [file]}",
    },
    MethodDoc {
        name: "scene.transition",
        doc: "Move the player to a room: rooms within the streaming radius load, the rest unload, and a room.transition event is emitted.",
        params: "room: string",
        returns: "{loaded: [room], unloaded: [room], current}",
    },
    MethodDoc {
        name: "scene.tiles.get",
        doc: "A rectangle of cells from a bulk index file (tiles.map.toml) in the current scene, without reading the chunk yourself.",
        params: "file: string (project-relative *.map.toml); layer: string; rect: {x, y, w, h}",
        returns: "{file, layer, rect, width, height, layers, cells: [integer]}",
    },
    MethodDoc {
        name: "edit.begin",
        doc: "Start a transaction. Edits queue until commit and apply atomically; authored entities are written to their files on commit.",
        params: "author: string (\"agent:claude\", \"human:kevin\"); note: string (why)",
        returns: "{tx}",
    },
    MethodDoc {
        name: "edit.set",
        doc: "Queue a field write.",
        params: "tx: integer; entity: string; path: \"Component.field\"; value: any",
        returns: "{queued}",
    },
    MethodDoc {
        name: "edit.add",
        doc: "Queue adding a component with its defaults (an override when the entity has a prefab).",
        params: "tx; entity; component: string",
        returns: "{queued}",
    },
    MethodDoc {
        name: "edit.remove",
        doc: "Queue removing a component (an explicit -Component entry in the file when a prefab provides it).",
        params: "tx; entity; component: string",
        returns: "{queued}",
    },
    MethodDoc {
        name: "edit.spawn",
        doc: "Queue a spawn. With id it is an authored entity and gets a file (in room, for room scenes); without, a runtime entity that is never written.",
        params: "tx; id: string (optional); room: string (optional); prefab: string (optional); tags: [string]; components: {Name: {field: value}}",
        returns: "{queued}",
    },
    MethodDoc {
        name: "edit.despawn",
        doc: "Queue a despawn. An authored entity's file is deleted on commit.",
        params: "tx; entity: string",
        returns: "{queued}",
    },
    MethodDoc {
        name: "edit.commit",
        doc: "Apply the transaction: all of it or none. Records its inverse for undo, appends to the change feed, emits edit.committed.",
        params: "tx: integer",
        returns: "{commit, frame, touched: [entity], files: [path], ops}",
    },
    MethodDoc {
        name: "edit.abort",
        doc: "Discard a transaction.",
        params: "tx: integer",
        returns: "{aborted}",
    },
    MethodDoc {
        name: "edit.undo",
        doc: "Undo the author's latest commit (as a new commit). Scoped per author: one party's undo never unwinds the other's work.",
        params: "author: string",
        returns: "the commit result",
    },
    MethodDoc {
        name: "edit.redo",
        doc: "Redo the author's latest undo.",
        params: "author: string",
        returns: "the commit result",
    },
    MethodDoc {
        name: "edit.feed",
        doc: "The change feed: every commit with author, note, frame, and what it touched. Answers who changed this and why.",
        params: "since: integer (commit id, exclusive); author: string (optional); limit; cursor",
        returns: "{total, rows: [{id, author, note, frame, kind, touched, files, ops}], next}",
    },
    MethodDoc {
        name: "edit.lock",
        doc: "Fence entities or paths for an author. Another author's write into them fails naming the lock, its owner, and its note.",
        params: "author: string; note: string; entities: [entity] (optional); paths: [\"Component\" | \"Component.field\"] (optional)",
        returns: "{lock}",
    },
    MethodDoc {
        name: "edit.unlock",
        doc: "Release a lock you own.",
        params: "author: string; lock: integer",
        returns: "{unlocked}",
    },
    MethodDoc {
        name: "edit.locks",
        doc: "Locks currently held.",
        params: "",
        returns: "{rows: [{id, author, note, entities, paths}]}",
    },
    MethodDoc {
        name: "input.inject",
        doc: "Inject an action for a player slot; gameplay sees it as the Actions component next tick. Continuous: held for frames ticks (default 1) or until another inject with hold; discrete: a command for the next turn. Toggles flip.",
        params: "action: string; slot: integer (default 1); value: true|false | number | [x, y] (default: the active value); frames: integer; hold: boolean",
        returns: "{action, slot, tick, until}",
    },
    MethodDoc {
        name: "input.script",
        doc: "Schedule several injects at ticks relative to now.",
        params: "steps: [{at: integer, action, slot, value, frames, hold}]",
        returns: "{scheduled, from, until}",
    },
    MethodDoc {
        name: "input.record",
        doc: "Start or stop writing the input log to .argus/runs/<stamp>-<seed>/input.log, next to a run.toml with the run identity (scene, seed, clock).",
        params: "on: boolean (default true)",
        returns: "{recording, path, run, entries}",
    },
    MethodDoc {
        name: "input.replay",
        doc: "Feed a recorded input log back in: the same scene, seed, and code then give the same run.",
        params: "path: string (project-relative or absolute)",
        returns: "{path, entries, last_tick}",
    },
    MethodDoc {
        name: "input.log.get",
        doc: "The input log: every action value change by tick (continuous) or turn (discrete).",
        params: "since: integer (tick); slot: integer; action: string; limit; cursor",
        returns: "{total, columns, rows: [[tick, slot, action, value]], recording, clock, next}",
    },
    MethodDoc {
        name: "input.devices",
        doc: "Real devices feeding the action map (phase 1): keys and mouse buttons held, gamepads connected in slot order, events seen. Keyboard and mouse arrive through the window; gamepads are polled while a window is open.",
        params: "",
        returns: "{window, keys: [name], mouse: [name], gamepads: [[index, name]], events_seen, turns_taken}",
    },
    MethodDoc {
        name: "input.bindings",
        doc: "The action map from input.toml: actions with type, doc, toggle, and bindings per device; player slots with their devices.",
        params: "",
        returns: "{file, actions: [{name, type, doc, toggle, bindings, at}], slots: [{slot, devices}]}",
    },
    MethodDoc {
        name: "input.state",
        doc: "The current value of every action for a slot.",
        params: "slot: integer (default 1)",
        returns: "{slot, tick, actions: {name: value}, pending}",
    },
    MethodDoc {
        name: "sim.play",
        doc: "Enter play: snapshot the world and enable the sim stages. In a continuous scene the daemon ticks in real time; in a discrete scene time moves on sim.advance.",
        params: "",
        returns: "sim.status",
    },
    MethodDoc {
        name: "sim.pause",
        doc: "Pause the clock; the world stays as it is.",
        params: "",
        returns: "sim.status",
    },
    MethodDoc {
        name: "sim.stop",
        doc: "Leave play and restore the snapshot taken at sim.play.",
        params: "",
        returns: "sim.status",
    },
    MethodDoc {
        name: "sim.step",
        doc: "Run n ticks of the driving clock now, whatever the play state.",
        params: "n: integer (default 1)",
        returns: "sim.status",
    },
    MethodDoc {
        name: "sim.advance",
        doc: "Advance the discrete clock n turns.",
        params: "by: \"turn\" (default); n: integer (default 1)",
        returns: "sim.status",
    },
    MethodDoc {
        name: "sim.speed",
        doc: "Continuous clock speed: sim ticks per render frame. 0 pauses; 3 runs three ticks per frame.",
        params: "x: number",
        returns: "sim.status",
    },
    MethodDoc {
        name: "sim.run_until",
        doc: "Fast-forward headless, as fast as the CPU allows, until an event fires, a sim time is reached, or n ticks pass.",
        params: "event: string | time: number (seconds) | ticks: integer; max_ticks: integer (default 100000)",
        returns: "{ticks, hit} plus sim.status",
    },
    MethodDoc {
        name: "sim.status",
        doc: "Clock state.",
        params: "",
        returns: "{playing, paused, clock, hz, speed, tick, time, frame, seed}",
    },
    MethodDoc {
        name: "script.reload",
        doc: "Recompile changed Luau files, rebind their systems in place, keep world state, tick, and RNG streams, and re-read def tables.",
        params: "",
        returns: "{reloaded: [file], removed: [file], errors: integer}",
    },
    MethodDoc {
        name: "script.systems",
        doc: "Every system with stage, ordering, declared reads and writes, clock, RNG stream, enabled state, and source.",
        params: "limit; cursor",
        returns: "{total, rows: [{name, stage, after, reads, writes, clock, rng, enabled, disabled_reason, source, doc}], next}",
    },
    MethodDoc {
        name: "script.errors",
        doc: "Outstanding script errors as file:line data, never a crash.",
        params: "",
        returns: "{total, rows: [{file, line, message, phase}]}",
    },
    MethodDoc {
        name: "obs.events",
        doc: "The structured event log: spawns, despawns, gameplay events, system faults, room transitions, edits. Filter by name, entity, or frame; page with since.",
        params: "since: integer (sequence number, exclusive); name: string (optional); entity: string (optional); from_frame: integer; limit; cursor",
        returns: "{total, rows: [{seq, frame, name, fields}], next, last_seq}",
    },
    MethodDoc {
        name: "obs.frame",
        doc: "Render the current world offscreen and write it as a PNG (section 8 L5). Sprites and the active Camera come from the world; described shapes need no asset. Semantic frames and annotation arrive with phase-1 item 12.",
        params: "format: \"png\" (default); width: integer (default 640); height: integer (default 360); path: string (default .argus/frames/frame-<tick>.png); software: boolean (force the software rasteriser)",
        returns: "{path, width, height, tick, alpha, sprites, draw_calls, atlases, missing_atlases, interpolated, adapter, ms}",
    },
    MethodDoc {
        name: "obs.subscribe",
        doc: "Receive notifications on this connection: `event` for each new event, `edit.committed` for each commit. Topics: events, edits.",
        params: "topics: [\"events\" | \"edits\"]; off: boolean (unsubscribe)",
        returns: "{topics}",
    },
    MethodDoc {
        name: "test.list",
        doc: "The scenarios under tests/*.luau: name, file, scene, seeds. Files that fail to load are listed, not fatal.",
        params: "",
        returns: "{total, rows: [{name, file, scene, seeds, runs}], load_errors: [{file, message}]}",
    },
    MethodDoc {
        name: "test.run",
        doc: "Run scenarios headless in this daemon: each seed loads the scene fresh, scripts input, runs frames, and checks expectations; state goldens live in tests/goldens/ and a mismatch is a field-level diff. The scene that was open is reloaded afterwards. `argus test` is this.",
        params: "name: string (optional; exact or substring); file: string (optional, tests/x.luau); seeds: string (optional override: \"7\", \"1..50\", \"1,2,9\"); update: boolean (rewrite goldens)",
        returns: "{ok, ms, total_runs, passed_runs, failed_runs, scenarios: [{name, file, scene, ok, seeds: [{seed, ok, ms, tick, error, expectations: [{kind, desc, ok, message, ...}], log}]}], load_errors}",
    },
    MethodDoc {
        name: "render.window",
        doc: "Open or close the window the daemon presents into every frame (playable by a human, section 18 phase 1). Device events from it feed the input layer.",
        params: "open: boolean (default true); width: integer (default 1280); height: integer (default 720); title: string; software: boolean",
        returns: "render.status",
    },
    MethodDoc {
        name: "render.status",
        doc: "Whether a window is open, the adapter in use, and the last frame's statistics.",
        params: "",
        returns: "{window, size: [w, h], adapter, software, last: {sprites, draw_calls, atlases, interpolated, missing_atlases, ms}, atlases: [name]}",
    },
    MethodDoc {
        name: "prof.frame",
        doc: "The last tick: total time and per-system breakdown.",
        params: "",
        returns: "{tick, clock, total_ms, allocs, bytes, systems: [[name, ms, allocs, bytes]], archetypes: [[id, entities]]}",
    },
    MethodDoc {
        name: "prof.worst",
        doc: "The n slowest recent ticks with per-system breakdown and entity counts by archetype.",
        params: "n: integer (default 5)",
        returns: "{rows: [{tick, clock, total_ms, allocs, bytes, systems, archetypes}]}",
    },
    MethodDoc {
        name: "prof.systems",
        doc: "Per-system mean and p95 over the last n ticks, in stage order.",
        params: "last: integer (default 600)",
        returns: "{last, tick_p95_ms, rows: [[name, stage, mean_ms, p95_ms, max_ms, samples]]}",
    },
    MethodDoc {
        name: "prof.archetypes",
        doc: "Entity counts and bytes by archetype.",
        params: "",
        returns: "{rows: [[components, entities, bytes]]}",
    },
    MethodDoc {
        name: "prof.alloc",
        doc: "Heap allocations by system over the last n ticks, in stage order: a Luau system that builds a table per entity shows up here. Counted on the sim thread by argusd's allocator; `counted` is false in a build without it.",
        params: "last: integer (default 600)",
        returns: "{last, counted, tick_mean_allocs, tick_mean_bytes, rows: [[name, stage, mean_allocs, mean_bytes, max_allocs, max_bytes, samples]]}",
    },
    MethodDoc {
        name: "prof.capture",
        doc: "Capture the next n ticks as a trace file for a human: every system span on one time base with its allocation counts, in the Chrome trace event format that Tracy imports (tracy-import-chrome) and Perfetto opens. Written to .argus/captures/ when the last tick is in; a `prof.capture` event names the file. Without `ticks`, reports the capture in progress.",
        params: "ticks: integer (1 to 100000); path: string (optional, where to write)",
        returns: "{armed, captured, ticks, path, from_tick}",
    },
];

/// Look a method up.
pub fn find(name: &str) -> Option<&'static MethodDoc> {
    METHODS.iter().find(|m| m.name == name)
}

/// The reference as text, for one method or all.
pub fn text(method: Option<&str>) -> String {
    let mut out = String::new();
    for m in METHODS {
        if method.is_some_and(|n| n != m.name) {
            continue;
        }
        out.push_str(m.name);
        out.push('\n');
        out.push_str("  ");
        out.push_str(m.doc);
        out.push('\n');
        if !m.params.is_empty() {
            out.push_str("  params: ");
            out.push_str(m.params);
            out.push('\n');
        }
        out.push_str("  returns: ");
        out.push_str(m.returns);
        out.push_str("\n\n");
    }
    out
}
