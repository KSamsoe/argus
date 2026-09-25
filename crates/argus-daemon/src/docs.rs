//! The one schema (§16): every protocol method and every agent tool,
//! described once. The protocol serves it (`session.capabilities`,
//! `session.docs`); the CLI's help, its bare-value parsing, the MCP tool
//! descriptions and input schemas, and the agent-facing docs are generated
//! from it, so none of them can drift from the others.

use serde_json::{json, Map, Value as Json};

/// A parameter's JSON type: "string".
pub const STRING: &str = "string";
/// "integer".
pub const INTEGER: &str = "integer";
/// "number".
pub const NUMBER: &str = "number";
/// "boolean".
pub const BOOLEAN: &str = "boolean";
/// "array".
pub const ARRAY: &str = "array";
/// "object".
pub const OBJECT: &str = "object";
/// Any JSON value.
pub const ANY: &str = "any";

/// One parameter of a method or a tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Param {
    /// Its key.
    pub name: &'static str,
    /// Its JSON type (`STRING`, `INTEGER`, ...).
    pub ty: &'static str,
    /// A call without it is refused.
    pub required: bool,
    /// What it is: allowed values, the default, what it means. A doc that
    /// starts "one of a, b" names a group of which a call gives one.
    pub doc: &'static str,
}

/// A required parameter.
pub const fn req(name: &'static str, ty: &'static str, doc: &'static str) -> Param {
    Param {
        name,
        ty,
        required: true,
        doc,
    }
}

/// An optional parameter.
pub const fn opt(name: &'static str, ty: &'static str, doc: &'static str) -> Param {
    Param {
        name,
        ty,
        required: false,
        doc,
    }
}

impl Param {
    /// The parameter as JSON: `{name, type, required, doc}`.
    pub fn to_json(&self) -> Json {
        json!({ "name": self.name, "type": self.ty, "required": self.required, "doc": self.doc })
    }

    /// Part of a group of which a call gives one (`one of a, b`).
    pub fn one_of(&self) -> bool {
        self.doc.starts_with("one of ")
    }
}

/// Parameters as one line of text: `name: type, required (doc); ...`, the
/// shape the CLI's help and `session.docs` print.
pub fn params_text(params: &[Param]) -> String {
    params
        .iter()
        .map(|p| {
            let mut s = format!("{}: {}", p.name, p.ty);
            if p.required {
                s.push_str(", required");
            }
            if !p.doc.is_empty() {
                s.push_str(&format!(" ({})", p.doc));
            }
            s
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Parameters as a JSON Schema object (an MCP tool's `inputSchema`).
pub fn input_schema(params: &[Param]) -> Json {
    let mut props = Map::new();
    for p in params {
        let mut o = Map::new();
        if p.ty != ANY {
            o.insert("type".into(), json!(p.ty));
        }
        if p.ty == ARRAY {
            o.insert("items".into(), json!({}));
        }
        if !p.doc.is_empty() {
            o.insert("description".into(), json!(p.doc));
        }
        props.insert(p.name.into(), Json::Object(o));
    }
    let required: Vec<&str> = params
        .iter()
        .filter(|p| p.required)
        .map(|p| p.name)
        .collect();
    json!({ "type": "object", "properties": props, "required": required })
}

/// One agent tool (§16): what `argus mcp` exposes and the CLI mirrors, each
/// a few protocol methods behind one verb. `argus <name>` runs it from a
/// shell with the same parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolDoc {
    /// The verb (`query`).
    pub name: &'static str,
    /// What it does and when to reach for it.
    pub doc: &'static str,
    /// Its parameters.
    pub params: &'static [Param],
    /// The protocol methods it calls.
    pub calls: &'static [&'static str],
}

/// The tools, about a dozen on purpose: enough to build, run, look, and
/// work through the human's marks and proposals; every method stays
/// reachable through the CLI.
pub const TOOLS: &[ToolDoc] = &[
    ToolDoc {
        name: "summary",
        doc: "One-screen orientation: entity counts by archetype, named entities, active systems, frame, seed, clock. The MCP server runs it on connect; run it again after a scene load or a big edit.",
        params: &[opt("scene", STRING, "another scene of the project, from the index; default the one loaded")],
        calls: &["world.summary"],
    },
    ToolDoc {
        name: "query",
        doc: "Read the world: one entity in full, or the entities matching a filter with the fields you select, as compact rows. With entity: that entity, every component and field. Otherwise select, where, and order, e.g. select [\"id\", \"Health.cur\"], where {has: [\"Enemy\"], \"Health.cur\": {lt: 3}}, order \"dist(@player)\".",
        params: &[
            opt("entity", STRING, "\"@id\" or \"e:n\": one entity in full"),
            opt("select", ARRAY, "[\"id\" | \"Component.field\" | \"Component\"]; default [\"id\"]"),
            opt("where", OBJECT, "{has: [Component], not: [Component], within: {of: entity, r: number}, \"Component.field\": value | {eq|ne|lt|lte|gt|gte: value}}"),
            opt("order", STRING, "\"Component.field\", \"-Component.field\", or \"dist(entity)\""),
            opt("limit", INTEGER, "default 50"),
            opt("scene", STRING, "another scene, from the index"),
        ],
        calls: &["world.get", "world.query"],
    },
    ToolDoc {
        name: "describe",
        doc: "What things are: components with their fields and docs, or the systems in run order. A component: its fields with types, defaults, ranges, tune flags, and docs, and the systems that write and read it. No component: every component. systems: every system in run order with its stage, reads, writes, and doc.",
        params: &[
            opt("component", STRING, "a component's name"),
            opt("systems", BOOLEAN, "the systems instead of components"),
        ],
        calls: &["world.describe", "script.systems"],
    },
    ToolDoc {
        name: "edit",
        doc: "Change the game's authored state (its files) in one transaction by an author with a note, so the change feed says who and why and undo takes it back. ops run in order and all are kept or none: each is {op: set|add|remove|spawn|despawn|tiles, ...} with that edit method's parameters, e.g. {op: \"set\", entity: \"@player\", path: \"CharacterController.jump_height\", value: 3.4}. undo: take back the author's last transaction instead.",
        params: &[
            opt("ops", ARRAY, "one of ops, undo; [{op, ...}]: set {entity, path, value}, add {entity, component}, remove {entity, component}, spawn {id, room, prefab, tags, components}, despawn {entity}, tiles {file, layer, rect, cells | tile | terrain | erase}"),
            opt("undo", BOOLEAN, "one of ops, undo; undo the author's last transaction"),
            opt("note", STRING, "why, for the change feed"),
            opt("author", STRING, "default \"agent:claude\""),
        ],
        calls: &["edit.begin", "edit.set", "edit.add", "edit.remove", "edit.spawn", "edit.despawn", "edit.tiles", "edit.commit", "edit.abort", "edit.undo"],
    },
    ToolDoc {
        name: "run",
        doc: "Advance the simulation: step ticks, run until something happens, take turns, play, pause, or stop. step: that many ticks; until: run until an event, a time, or a tick count; turns: take turns on a discrete clock; play, pause, speed, and stop (back to where play started). With none, where the sim is. Always answers with sim.status after.",
        params: &[
            opt("step", INTEGER, "ticks to run"),
            opt("until", OBJECT, "{event: name} | {time: seconds} | {ticks: n}; max_ticks: n (default 100000)"),
            opt("turns", INTEGER, "turns to take on a discrete clock"),
            opt("play", BOOLEAN, "start playing in real time (a window, if open, shows it)"),
            opt("pause", BOOLEAN, "pause"),
            opt("stop", BOOLEAN, "stop and go back to the frame play started from"),
            opt("speed", NUMBER, "ticks per real-time frame; 0 pauses"),
        ],
        calls: &["sim.stop", "sim.play", "sim.pause", "sim.speed", "sim.step", "sim.run_until", "sim.advance", "sim.status"],
    },
    ToolDoc {
        name: "seek",
        doc: "Go to any frame of this run, rebuilt from keyframes and the recorded input; stepping on replays what happened next until a new input branches the run. fork: instead build a second world at that frame, named, to try something beside this one (every method then takes fork: name).",
        params: &[
            opt("frame", INTEGER, "one of frame, turn; the tick"),
            opt("turn", INTEGER, "one of frame, turn; the turn, on a discrete clock"),
            opt("fork", STRING, "a name: fork there instead of seeking"),
        ],
        calls: &["sim.seek", "sim.fork"],
    },
    ToolDoc {
        name: "observe",
        doc: "Look at the game: the semantic frame by default, or the picture. The semantic frame is the camera's rect, each notable entity with its screen bounds and description, HUD text, and the effects on screen, in about 3 KB. format \"png\": the frame drawn (annotate adds entity ids and boxes); the MCP server returns the picture.",
        params: &[
            opt("format", STRING, "\"semantic\" (default) or \"png\""),
            opt("annotate", BOOLEAN, "png: draw entity ids and bounds over it"),
            opt("width", INTEGER, "default 640"),
            opt("height", INTEGER, "default 360"),
        ],
        calls: &["obs.frame"],
    },
    ToolDoc {
        name: "events",
        doc: "What happened: the event log (collisions, deaths, sounds, faults, anything a system emitted), by name, entity, or since a sequence number. why: the systems and events that led to one event. trace: an entity's fields frame by frame.",
        params: &[
            opt("name", STRING, "an event name, e.g. \"player.fell\""),
            opt("entity", STRING, "events naming this entity"),
            opt("since", INTEGER, "events after this sequence number"),
            opt("limit", INTEGER, "default 50"),
            opt("why", ANY, "an event's sequence number, or a name for the latest such event"),
            opt("trace", OBJECT, "{entity, fields: [Component or Component.field], frames: n or [from, to]}"),
        ],
        calls: &["obs.events", "obs.why", "obs.trace"],
    },
    ToolDoc {
        name: "test",
        doc: "Run the project's scenarios (tests/*.luau): each run's pass or fail with the expectations that failed; a failing run saves a replay of the moment it failed.",
        params: &[
            opt("name", STRING, "a scenario, exact or a substring; default all"),
            opt("file", STRING, "only tests/x.luau"),
            opt("seeds", STRING, "\"7\", \"1..50\", or \"1,2,9\""),
            opt("update", BOOLEAN, "rewrite the state goldens"),
        ],
        calls: &["test.run"],
    },
    ToolDoc {
        name: "reload",
        doc: "Take changes made on disk now: scripts are recompiled and swapped in while the world goes on from the current frame, and assets are reimported. Answers with what changed and every script error.",
        params: &[],
        calls: &["script.reload", "asset.import", "script.errors"],
    },
    ToolDoc {
        name: "marks",
        doc: "The human's marks: moments they flagged while playing, each with a note, in a queue. list (default): the open ones. open: reproduce one (its scene, the seconds before it in the log, the world compared with the one they saw). get, note, close (say what was done). import: a player's report file; pull: the reports a playtest server collected.",
        params: &[
            opt("action", STRING, "list (default), get, open, note, close, import, or pull"),
            opt("id", INTEGER, "the mark, for get, open, note, close"),
            opt("note", STRING, "for note and close"),
            opt("path", STRING, "for import"),
            opt("url", STRING, "for pull: the server's tailnet or LAN address"),
            opt("all", BOOLEAN, "list closed marks too"),
        ],
        calls: &["mark.list", "mark.get", "mark.open", "mark.note", "mark.close", "mark.import", "mark.pull"],
    },
    ToolDoc {
        name: "propose",
        doc: "Offer a change for the human to feel before it lands: ops as in edit become a proposal they try in the running game (F9 switches between the game as it is and each variant at the same moment, F10 keeps one). proposal: add these ops as another variant of that proposal. list: the proposals waiting.",
        params: &[
            opt("ops", ARRAY, "one of ops, list; as in edit"),
            opt("list", BOOLEAN, "one of ops, list; the proposals waiting"),
            opt("note", STRING, "what to try it for"),
            opt("proposal", INTEGER, "add a variant to this proposal"),
            opt("author", STRING, "default \"agent:claude\""),
        ],
        calls: &["edit.begin", "edit.set", "edit.add", "edit.remove", "edit.spawn", "edit.despawn", "edit.propose", "edit.abort", "proposal.list"],
    },
];

/// Look a tool up.
pub fn tool(name: &str) -> Option<&'static ToolDoc> {
    TOOLS.iter().find(|t| t.name == name)
}

/// A tool's description as the MCP server and `argus <tool> --help` give
/// it: the doc, then the protocol methods behind it and its shell form.
pub fn tool_description(t: &ToolDoc) -> String {
    format!(
        "{}\n\nCalls {}. From a shell: argus {} key=value ... (or --params '<the same JSON>').",
        t.doc,
        t.calls.join(", "),
        t.name
    )
}

/// One protocol method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MethodDoc {
    /// Dotted name (`world.query`).
    pub name: &'static str,
    /// One or two sentences on what it does.
    pub doc: &'static str,
    /// Parameters, in the order a bare CLI value fills them.
    pub params: &'static [Param],
    /// What the result holds.
    pub returns: &'static str,
}

/// Every method the phase-0 daemon answers, in namespace order.
pub const METHODS: &[MethodDoc] = &[
    MethodDoc {
        name: "session.open",
        doc: "Open a project and load a scene into the one world this daemon holds. Scripts in components/, defs/, systems/ load first, then prefabs and defs, then the scene.",
        params: &[
            req("project", STRING, "directory with argus.toml"),
            opt("scene", STRING, "default from argus.toml"),
            opt("seed", INTEGER, "default: the scene's seed policy"),
        ],
        returns: "the world.summary, plus issues: [] when the scene could not be instantiated, and run_from: the seq of the run's first event (after the load's own; what on_load systems emit is the run's)",
    },
    MethodDoc {
        name: "session.status",
        doc: "What is open: project, scene, seed, clock, and whether the sim is playing.",
        params: &[],
        returns: "{project, scene, seed, clock, playing, paused, tick, time, frame}",
    },
    MethodDoc {
        name: "session.capabilities",
        doc: "The daemon's version and every method it answers, with parameter docs.",
        params: &[],
        returns: "{version, phase, transports, methods: [{name, doc, params, returns}]}",
    },
    MethodDoc {
        name: "session.docs",
        doc: "The method reference as text, for one method or all of them.",
        params: &[
            opt("method", STRING, "optional"),
        ],
        returns: "{text}",
    },
    MethodDoc {
        name: "session.close",
        doc: "Drop the project, scene, and world. The daemon keeps running.",
        params: &[],
        returns: "{closed: true}",
    },
    MethodDoc {
        name: "session.shutdown",
        doc: "Close the project and exit the daemon process after replying. `argus daemon stop` is this.",
        params: &[],
        returns: "{shutdown: true}",
    },
    MethodDoc {
        name: "world.summary",
        doc: "One-screen orientation: entity counts by archetype, named entities, active systems, frame, seed, clock. The first call to make on connect. Served from the index (same shape, source: \"index\") for a scene that is not loaded.",
        params: &[
            opt("scene", STRING, "optional: another scene, from the index"),
            opt("source", STRING, "\"index\"; force the index"),
        ],
        returns: "{frame, tick, time, seed, clock, playing, scene, rooms, entities, authored, archetypes: [[components, count]], named: [[id, components]], systems: [name], disabled: [name]}",
    },
    MethodDoc {
        name: "world.query",
        doc: "Select field paths from entities matching a filter. Rows are compact arrays in select order; total is always present; next is a cursor for the rest.",
        params: &[
            opt("select", ARRAY, "[\"id\" | \"Component.field\" | \"Component\"]; default [\"id\"]"),
            opt("where", OBJECT, "{has: [Component], not: [Component], within: {of: entity, r: number}, \"Component.field\": value | {eq|ne|lt|lte|gt|gte: value}}"),
            opt("order", STRING, "\"Component.field\" | \"-Component.field\" | \"dist(entity)\""),
            opt("limit", INTEGER, "default 50"),
            opt("cursor", STRING, ""),
            opt("scene", STRING, "optional: another scene, from the index"),
            opt("source", STRING, "\"index\""),
        ],
        returns: "{frame, total, rows: [[...]], truncated, next}",
    },
    MethodDoc {
        name: "world.get",
        doc: "One entity in full. resolved: false adds the entity's own file overrides; resolved: true adds the merged prefab result with each field's provenance (own, a prefab path, or default).",
        params: &[
            req("entity", STRING, "\"@id\" or \"e:n\""),
            opt("resolved", BOOLEAN, "default false"),
            opt("scene", STRING, "optional"),
            opt("source", STRING, "\"index\""),
        ],
        returns: "{entity, id, authored, components: {Name: {field: value}}, overrides | resolved}",
    },
    MethodDoc {
        name: "world.describe",
        doc: "Component schemas with doc strings, field types, defaults, ranges, tune flags, and declaring file:line, plus which systems read and write them.",
        params: &[
            opt("component", STRING, "optional; all when omitted"),
            opt("limit", INTEGER, "rows per page"),
            opt("cursor", STRING, "the next page, from a previous answer's cursor"),
            opt("source", STRING, "\"index\""),
        ],
        returns: "{total, rows: [{name, doc, version, source, fields: [{name, type, default, range, doc, tune}], written_by, read_by}], next}",
    },
    MethodDoc {
        name: "world.who_writes",
        doc: "The systems that declare a write to a component, in run order: what could have changed it.",
        params: &[
            req("component", STRING, ""),
            opt("source", STRING, "\"index\""),
        ],
        returns: "{component, systems: [name], readers: [name]}",
    },
    MethodDoc {
        name: "world.defs",
        doc: "Data tables. Without a table, list them; with one, its rows filtered by field equality.",
        params: &[
            opt("table", STRING, "optional"),
            opt("where", OBJECT, "{field: value}"),
            opt("limit", INTEGER, "rows per page"),
            opt("cursor", STRING, "the next page, from a previous answer's cursor"),
            opt("source", STRING, "\"index\""),
        ],
        returns: "{tables: [...]} or {table, columns, total, rows: [[...]], next}",
    },
    MethodDoc {
        name: "world.refs",
        doc: "Everything in the project that references a target: an entity (@id), a prefab path, a def ref (item:key), or a file path. From the index.",
        params: &[
            req("target", STRING, ""),
            opt("limit", INTEGER, "rows per page"),
            opt("cursor", STRING, "the next page, from a previous answer's cursor"),
        ],
        returns: "{target, total, columns, rows: [[scene, from_kind, from, path, kind, at]], next}",
    },
    MethodDoc {
        name: "world.search",
        doc: "Full-text search over component, field, and system doc strings. From the index.",
        params: &[
            req("text", STRING, ""),
            opt("limit", INTEGER, "rows per page"),
            opt("cursor", STRING, "the next page, from a previous answer's cursor"),
        ],
        returns: "{total, columns, rows: [[kind, name, doc]], next}",
    },
    MethodDoc {
        name: "world.instances",
        doc: "Every entity in every scene whose prefab chain includes a prefab. From the index.",
        params: &[
            req("prefab", STRING, ""),
            opt("limit", INTEGER, "rows per page"),
            opt("cursor", STRING, "the next page, from a previous answer's cursor"),
        ],
        returns: "{prefab, total, columns, rows: [[scene, entity]], next}",
    },
    MethodDoc {
        name: "world.prefabs",
        doc: "The prefab catalog: every prefab with the prefab it extends, its components, its tags, and how many entities are instances of it (served from the index).",
        params: &[opt("limit", INTEGER, "default 100"), opt("cursor", STRING, "")],
        returns: "{total, rows: [{path, base, chain, components, tags, instances}], next}",
    },
    MethodDoc {
        name: "scene.list",
        doc: "Scenes in the project (scenes/<name>/scene.toml).",
        params: &[],
        returns: "{scenes: [name], current}",
    },
    MethodDoc {
        name: "scene.load",
        doc: "Reload the current scene, or switch to another one. Re-reads scripts, prefabs, defs, and files, and rebuilds the world.",
        params: &[
            opt("scene", STRING, "default: current"),
            opt("seed", INTEGER, ""),
        ],
        returns: "the world.summary, plus issues when the scene is not clean, and run_from: the seq of the run's first event (after the load's own; what on_load systems emit is the run's)",
    },
    MethodDoc {
        name: "scene.issues",
        doc: "Everything wrong with the current scene's files, each with file:line. Empty when the scene is loaded.",
        params: &[],
        returns: "{total, rows: [{code, message, hint, ref}]}",
    },
    MethodDoc {
        name: "scene.rooms",
        doc: "Rooms of the current scene with graph neighbours, bounds, entity counts, loaded state, and which is current.",
        params: &[
            opt("scene", STRING, "optional: another scene, from the index"),
            opt("source", STRING, "\"index\""),
        ],
        returns: "{rooms: [{name, loaded, entities, neighbors, bounds, bulk}], current, radius}",
    },
    MethodDoc {
        name: "scene.index",
        doc: "Bring .argus/index.db up to date (files changed outside the daemon are picked up; rebuild re-indexes everything) and report its size.",
        params: &[
            opt("rebuild", BOOLEAN, "default false"),
        ],
        returns: "{scenes, entities, failed, ms, stats}",
    },
    MethodDoc {
        name: "asset.list",
        doc: "Every asset under assets/: sprites (a PNG, or a sidecar describing a shape), sounds (a WAV, or a sidecar naming variations), animators (a sidecar with type = \"animator\": states, transitions, events by frame), particle effects (type = \"emitter\"), and materials (a .wgsl shader, with a sidecar of typed parameters), with the warnings the importer found. A name is the path without the extension; sprites/hero is assets/sprites/hero.png with assets/sprites/hero.toml beside it. For an animator the frames and clips columns are its state count and names; for an emitter its pool size and sprite; for a material its parameter count and names. kind = \"material\" also lists the engine's material library (unlit, lit, palette, dissolve, outline), source builtin:<name>, unless a project asset of the same name shadows one.",
        params: &[
            opt("kind", STRING, "\"sprite\" | \"sound\" | \"animator\" | \"emitter\" | \"material\"; optional"),
            opt("limit", INTEGER, "rows per page"),
            opt("cursor", STRING, "the next page, from a previous answer's cursor"),
        ],
        returns: "{total, columns: [name, kind, source, frames, clips, warnings], rows, failed: [{name, message, at}], next}",
    },
    MethodDoc {
        name: "asset.get",
        doc: "One asset as imported: frames with rects, pivots, durations, and properties; clips; the cached texture; a sound's bus, gain, and files with rate, channels, length, and peak; an animator's states; an emitter's rate, burst, and pool; a material's shader, blend, and parameters (type, default, range, from, doc); every warning with file:line. A library material (unlit, lit, palette, dissolve, outline) answers too, with builtin: true.",
        params: &[
            req("name", STRING, ""),
        ],
        returns: "{name, kind, sidecar, hash, warnings, ...}",
    },
    MethodDoc {
        name: "asset.import",
        doc: "Scan assets/ now instead of waiting for the watcher (it polls twice a second). Changed sprites and materials reload in every renderer (a material's shader is checked at import, so one that does not compile fails here with its line and the last good version keeps drawing) and changed sounds in the mixer; each import is an asset.imported event, each warning an asset.warning, each failure an asset.failed. force re-imports everything.",
        params: &[
            opt("force", BOOLEAN, "default false"),
        ],
        returns: "{imported: [name], unchanged, removed: [name], failed: [{name, message, at}], ms}",
    },
    MethodDoc {
        name: "asset.move",
        doc: "Rename a prefab, entity file, def row, or asset and rewrite every reference to it; appends to .argus/moves.log. The only sanctioned way to rename.",
        params: &[
            req("from", STRING, "project path, or defs/x.toml#key"),
            req("to", STRING, ""),
        ],
        returns: "{kind, from, to, refs, rewritten: [file]}",
    },
    MethodDoc {
        name: "scene.transition",
        doc: "Move the player to a room: rooms within the streaming radius load, the rest unload, and a room.transition event is emitted.",
        params: &[
            req("room", STRING, ""),
        ],
        returns: "{loaded: [room], unloaded: [room], current}",
    },
    MethodDoc {
        name: "scene.tiles.get",
        doc: "A rectangle of cells from a tilemap (*.tilemap.toml) or a phase-0 bulk index (*.map.toml), without reading the chunks yourself. Tile ids: 0 is empty, n is frame n - 1 of the layer's tileset.",
        params: &[
            req("file", STRING, "project-relative *.map.toml"),
            opt("layer", STRING, ""),
            req("rect", OBJECT, "{x, y, w, h}"),
        ],
        returns: "{file, layer, rect, width, height, layers, cells: [integer]}",
    },
    MethodDoc {
        name: "scene.tiles.create",
        doc: "Create an empty tilemap: a text index (*.tilemap.toml) and a folder of chunk files that only holds chunks with tiles in them. Place it in the world with a Tilemap component whose map names the file.",
        params: &[
            req("file", STRING, "project-relative, ending .tilemap.toml"),
            opt("width", INTEGER, ""),
            opt("height", INTEGER, ""),
            req("layers", ARRAY, "[{name, tileset (a sprite asset), z, collision}]"),
            opt("tile_size", NUMBER, "world units, default 1"),
            opt("chunk", INTEGER, "default 64"),
            opt("cell", INTEGER, "1 | 2 | 4; bytes per tile id, default 2"),
        ],
        returns: "scene.tiles.info",
    },
    MethodDoc {
        name: "scene.tiles.info",
        doc: "A tilemap's index: size, tile size, layers with their tileset and its terrains, chunks on disk and in memory.",
        params: &[
            req("file", STRING, ""),
        ],
        returns: "{file, width, height, tile_size, chunk, cell, hash, layers: [{name, tileset, z, collision, terrains, tileset_found}], chunks, loaded}",
    },
    MethodDoc {
        name: "scene.tiles.collision",
        doc: "What bodies hit in a rect of a tilemap, one string per row: # solid, - one-way, / and \\ slopes rising right and left, _ a low flat block, . empty. From every collision layer's tiles' solid, slope, and one_way properties, as the physics step reads them.",
        params: &[
            req("file", STRING, ""),
            req("rect", OBJECT, "{x, y, w, h}"),
        ],
        returns: "{file, rect, rows: [string]}",
    },
    MethodDoc {
        name: "edit.tiles",
        doc: "Queue a tile edit in a transaction: cells (row-major ids), one tile to fill the rect, a terrain to paint and auto-tile, or erase. Applied at commit, saved to the changed chunks only, undoable per author, in the change feed.",
        params: &[
            req("tx", INTEGER, "the transaction, from edit.begin"),
            req("file", STRING, ""),
            req("layer", STRING, ""),
            req("rect", OBJECT, "{x, y, w, h}"),
            opt("cells", ARRAY, "one of cells, tile, terrain, erase; [integer], a tile id per cell of rect, row by row"),
            opt("tile", INTEGER, "one of cells, tile, terrain, erase; fill rect with this tile id"),
            opt("terrain", STRING, "one of cells, tile, terrain, erase; paint rect with an auto-tiling terrain"),
            opt("erase", BOOLEAN, "one of cells, tile, terrain, erase; true clears rect"),
        ],
        returns: "{queued}",
    },
    MethodDoc {
        name: "edit.begin",
        doc: "Start a transaction. Edits queue until commit and apply atomically; authored entities are written to their files on commit.",
        params: &[
            req("author", STRING, "\"agent:claude\", \"human:kevin\""),
            opt("note", STRING, "why"),
        ],
        returns: "{tx}",
    },
    MethodDoc {
        name: "edit.set",
        doc: "Queue a field write.",
        params: &[
            req("tx", INTEGER, ""),
            req("entity", STRING, ""),
            req("path", STRING, "\"Component.field\""),
            req("value", ANY, ""),
        ],
        returns: "{queued}",
    },
    MethodDoc {
        name: "edit.add",
        doc: "Queue adding a component with its defaults (an override when the entity has a prefab).",
        params: &[
            req("tx", INTEGER, "the transaction, from edit.begin"),
            req("entity", STRING, "\"@id\" or \"e:n\""),
            req("component", STRING, ""),
        ],
        returns: "{queued}",
    },
    MethodDoc {
        name: "edit.remove",
        doc: "Queue removing a component (an explicit -Component entry in the file when a prefab provides it).",
        params: &[
            req("tx", INTEGER, "the transaction, from edit.begin"),
            req("entity", STRING, "\"@id\" or \"e:n\""),
            req("component", STRING, ""),
        ],
        returns: "{queued}",
    },
    MethodDoc {
        name: "edit.spawn",
        doc: "Queue a spawn. With id it is an authored entity and gets a file (in room, for room scenes); without, a runtime entity that is never written.",
        params: &[
            req("tx", INTEGER, "the transaction, from edit.begin"),
            opt("id", STRING, "optional"),
            opt("room", STRING, "optional"),
            opt("prefab", STRING, "optional"),
            opt("tags", ARRAY, "[string]"),
            opt("components", OBJECT, "{Name: {field: value}}"),
        ],
        returns: "{queued}",
    },
    MethodDoc {
        name: "edit.despawn",
        doc: "Queue a despawn. An authored entity's file is deleted on commit.",
        params: &[
            req("tx", INTEGER, "the transaction, from edit.begin"),
            req("entity", STRING, ""),
        ],
        returns: "{queued}",
    },
    MethodDoc {
        name: "edit.commit",
        doc: "Apply the transaction: all of it or none. Records its inverse for undo, appends to the change feed, emits edit.committed.",
        params: &[
            req("tx", INTEGER, ""),
        ],
        returns: "{commit, frame, touched: [entity], files: [path], ops}",
    },
    MethodDoc {
        name: "edit.abort",
        doc: "Discard a transaction.",
        params: &[
            req("tx", INTEGER, ""),
        ],
        returns: "{aborted}",
    },
    MethodDoc {
        name: "edit.undo",
        doc: "Undo the author's latest commit (as a new commit). Scoped per author: one party's undo never unwinds the other's work.",
        params: &[
            req("author", STRING, ""),
        ],
        returns: "the commit result",
    },
    MethodDoc {
        name: "edit.redo",
        doc: "Redo the author's latest undo.",
        params: &[
            req("author", STRING, ""),
        ],
        returns: "the commit result",
    },
    MethodDoc {
        name: "edit.feed",
        doc: "The change feed: every commit with author, note, frame, and what it touched. Answers who changed this and why.",
        params: &[
            opt("since", INTEGER, "commit id, exclusive"),
            opt("author", STRING, "optional"),
            opt("limit", INTEGER, "rows per page"),
            opt("cursor", STRING, "the next page, from a previous answer's cursor"),
        ],
        returns: "{total, rows: [{id, author, note, frame, kind, touched, files, ops}], next}",
    },
    MethodDoc {
        name: "edit.lock",
        doc: "Fence entities or paths for an author. Another author's write into them fails naming the lock, its owner, and its note.",
        params: &[
            req("author", STRING, ""),
            opt("note", STRING, ""),
            opt("entities", ARRAY, "[entity]; optional"),
            opt("paths", ARRAY, "[\"Component\" | \"Component.field\"]; optional"),
        ],
        returns: "{lock}",
    },
    MethodDoc {
        name: "edit.unlock",
        doc: "Release a lock you own.",
        params: &[
            req("author", STRING, ""),
            req("lock", INTEGER, ""),
        ],
        returns: "{unlocked}",
    },
    MethodDoc {
        name: "edit.locks",
        doc: "Locks currently held.",
        params: &[],
        returns: "{rows: [{id, author, note, entities, paths}]}",
    },
    MethodDoc {
        name: "input.inject",
        doc: "Inject an action for a player slot; gameplay sees it as the Actions component next tick. Continuous: held for frames ticks (default 1) or until another inject with hold; discrete: a command for the next turn. Toggles flip.",
        params: &[
            req("action", STRING, ""),
            opt("slot", INTEGER, "default 1"),
            opt("value", ANY, "true|false | number | [x, y]; default: the active value"),
            opt("frames", INTEGER, ""),
            opt("hold", BOOLEAN, ""),
        ],
        returns: "{action, slot, tick, until}",
    },
    MethodDoc {
        name: "input.script",
        doc: "Schedule several injects at ticks relative to now.",
        params: &[
            req("steps", ARRAY, "[{at: integer, action, slot, value, frames, hold}]"),
        ],
        returns: "{scheduled, from, until}",
    },
    MethodDoc {
        name: "input.record",
        doc: "Start or stop writing the input log to .argus/runs/<stamp>-<seed>/input.log, next to a run.toml with the run identity (scene, seed, clock).",
        params: &[
            opt("on", BOOLEAN, "default true"),
        ],
        returns: "{recording, path, run, entries}",
    },
    MethodDoc {
        name: "input.replay",
        doc: "Feed a recorded input log back in: the same scene, seed, and code then give the same run.",
        params: &[
            req("path", STRING, "project-relative or absolute"),
        ],
        returns: "{path, entries, last_tick}",
    },
    MethodDoc {
        name: "input.log.get",
        doc: "The input log: every action value change by tick (continuous) or turn (discrete).",
        params: &[
            opt("since", INTEGER, "tick"),
            opt("slot", INTEGER, ""),
            opt("action", STRING, ""),
            opt("limit", INTEGER, "rows per page"),
            opt("cursor", STRING, "the next page, from a previous answer's cursor"),
        ],
        returns: "{total, columns, rows: [[tick, slot, action, value]], recording, clock, next}",
    },
    MethodDoc {
        name: "input.devices",
        doc: "Real devices feeding the action map (phase 1): keys and mouse buttons held, gamepads connected in slot order, events seen. Keyboard and mouse arrive through the window; gamepads are polled while a window is open.",
        params: &[],
        returns: "{window, keys: [name], mouse: [name], gamepads: [[index, name]], events_seen, turns_taken}",
    },
    MethodDoc {
        name: "input.bindings",
        doc: "The action map from input.toml: actions with type, doc, toggle, and bindings per device; player slots with their devices.",
        params: &[],
        returns: "{file, actions: [{name, type, doc, toggle, bindings, at}], slots: [{slot, devices}]}",
    },
    MethodDoc {
        name: "input.state",
        doc: "The current value of every action for a slot.",
        params: &[
            opt("slot", INTEGER, "default 1"),
        ],
        returns: "{slot, tick, actions: {name: value}, pending}",
    },
    MethodDoc {
        name: "sim.play",
        doc: "Enter play: snapshot the world and enable the sim stages. In a continuous scene the daemon ticks in real time; in a discrete scene time moves on sim.advance.",
        params: &[],
        returns: "sim.status",
    },
    MethodDoc {
        name: "sim.pause",
        doc: "Pause the clock; the world stays as it is.",
        params: &[],
        returns: "sim.status",
    },
    MethodDoc {
        name: "sim.stop",
        doc: "Leave play and restore the snapshot taken at sim.play.",
        params: &[],
        returns: "sim.status",
    },
    MethodDoc {
        name: "sim.step",
        doc: "Run n ticks of the driving clock now, whatever the play state.",
        params: &[
            opt("n", INTEGER, "default 1"),
        ],
        returns: "sim.status",
    },
    MethodDoc {
        name: "sim.advance",
        doc: "Advance the discrete clock n turns.",
        params: &[
            opt("by", STRING, "\"turn\"; default"),
            opt("n", INTEGER, "default 1"),
        ],
        returns: "sim.status",
    },
    MethodDoc {
        name: "sim.speed",
        doc: "Continuous clock speed: sim ticks per render frame. 0 pauses; 3 runs three ticks per frame.",
        params: &[
            opt("x", NUMBER, ""),
        ],
        returns: "sim.status",
    },
    MethodDoc {
        name: "sim.run_until",
        doc: "Fast-forward headless, as fast as the CPU allows, until an event fires, a sim time is reached, or n ticks pass.",
        params: &[
            opt("event", STRING, "one of event, time, ticks"),
            opt("time", NUMBER, "one of event, time, ticks; seconds"),
            opt("ticks", INTEGER, "one of event, time, ticks"),
            opt("max_ticks", INTEGER, "default 100000"),
        ],
        returns: "{ticks, hit} plus sim.status",
    },
    MethodDoc {
        name: "sim.seek",
        doc: "Go to a frame of this run: the nearest earlier keyframe is restored and the run replayed forward to it (section 6). Back, or forward along a run already played. The replay's events are checked against the log; stepping afterwards replays what happened next until a live command branches the run. Edits written to files are not rewound; undo does that.",
        params: &[
            opt("frame", INTEGER, "one of frame, turn; the tick to go to"),
            opt("turn", INTEGER, "one of frame, turn; the turn to go to, on a discrete clock"),
            opt("verify", BOOLEAN, "default false: also replay the keyframe before into the one used and compare the worlds field by field"),
        ],
        returns: "sim.status plus {from: the keyframe's frame, replayed: ticks, ms, checked: events compared, diverged: the first difference or null, verified}",
    },
    MethodDoc {
        name: "sim.fork",
        doc: "A second world from this one's state, inside the same daemon, with its own clock (section 6). Built from the files as they are now, so a fork after a script edit runs the new code beside the old; it copies the input journal, so from a past frame it replays the same inputs until a command of its own branches it. Any method takes fork: \"name\" to act on that world; switch makes it the world in use, the one the window shows.",
        params: &[
            opt("name", STRING, "omit to list the worlds"),
            opt("frame", INTEGER, "default: now; a past frame of this run"),
            opt("switch", BOOLEAN, ""),
            opt("drop", BOOLEAN, ""),
            opt("turn", INTEGER, "on a discrete clock, the turn to fork at instead of frame"),
        ],
        returns: "{active, worlds: [{name, tick, frame, entities}]}, plus made: {name, frame, from} when one was made",
    },
    MethodDoc {
        name: "sim.snapshot",
        doc: "Take a keyframe now, kept like a change's, and report the ring: keyframes every `every` ticks, all of them for the last five minutes of sim time, then one a minute to an hour, then one per ten minutes, under a byte budget ([ring] every and budget_mb in argus.toml).",
        params: &[],
        returns: "{keyframes, bytes, budget, every, oldest, newest, barriers, end, floor, tick}",
    },
    MethodDoc {
        name: "mark.drop",
        doc: "Mark this moment as a bug report (section 10): what F8 does in the window. Saves the frame as a picture, a two-second clip replayed from the ring, the last two seconds of sound, the last five seconds of input and events, the world's digest, and a replay file (a keyframe and the input since) that reproduces it after this daemon is gone. On a discrete clock the seconds are turns: ten of lead-in, a clip of eight. Marks land in .argus/marks/<id>/ and wait in the queue (mark.list).",
        params: &[
            opt("note", STRING, "optional; mark.note adds one later"),
            opt("author", STRING, "default \"human\""),
            opt("clip", BOOLEAN, "default true"),
        ],
        returns: "{id, status, source, frame, time, scene, note, author, dropped_at, dir, picture, clip, sound: absolute paths, ms}",
    },
    MethodDoc {
        name: "mark.list",
        doc: "The marks waiting for the agent, oldest first: the queue. A mark leaves it when closed.",
        params: &[
            opt("all", BOOLEAN, "default false: open marks only"),
        ],
        returns: "{open, closed, rows: [mark.drop's summary]}",
    },
    MethodDoc {
        name: "mark.get",
        doc: "Everything a mark holds: its frame, note, run identity (scene, seed, code, clock), digest, the input of the five seconds before (changes, and what was already held), the events of those seconds (counts by name and the latest 40), and its files.",
        params: &[
            req("id", INTEGER, ""),
        ],
        returns: "mark.json plus paths: the summary with absolute paths",
    },
    MethodDoc {
        name: "mark.open",
        doc: "Reproduce a mark: load its scene with its seed, restore its keyframe, and play the five seconds before it, so obs.events and obs.trace hold them, stopping at the mark's frame. Then compare the world with the one the mark saved, field by field: after a fix, the difference is the fix's effect at that frame. A web mark (mark.import) replays its input from the load instead. The sim is paused at the mark; sim.seek moves within the lead-in. A mark with no clip gets one.",
        params: &[
            req("id", INTEGER, ""),
        ],
        returns: "{id, frame, from, replayed, against_mark: {same, appeared, vanished, changed, first: [{entity, fields: [[path, then, now]]}]} (null for a web mark), digest: {mark, now, same}, code: {mark, now}, remapped, dropped: components changed or gone since, missing_effects, mark, note, ms}",
    },
    MethodDoc {
        name: "mark.note",
        doc: "Set a mark's note (\"dash feels late\"), as the window's prompt after F8 does.",
        params: &[
            req("id", INTEGER, ""),
            req("note", STRING, ""),
        ],
        returns: "mark.drop's summary",
    },
    MethodDoc {
        name: "mark.close",
        doc: "Done with a mark: it leaves the queue. The note says what was done.",
        params: &[
            req("id", INTEGER, ""),
            opt("note", STRING, "optional: \"fixed: dash buffer 6 frames\""),
            opt("author", STRING, "default \"agent\""),
        ],
        returns: "mark.drop's summary",
    },
    MethodDoc {
        name: "mark.import",
        doc: "Put a player's report into the queue as a mark: a replay file (.argr) a release build saves when its report key is pressed or it crashes, or the web page posts, with the player's note in it (phase 2 item 7); mark.open reproduces it like any mark. Also takes a web spike mark (the JSON WebPlayer.mark gives), which has no keyframe: mark.open replays its input from the load.",
        params: &[
            req("path", STRING, "the report or mark; relative to the project"),
            opt("author", STRING, "default player"),
        ],
        returns: "mark.drop's summary",
    },
    MethodDoc {
        name: "mark.pull",
        doc: "Fetch the reports browsers posted to a playtest server (tools/web/serve.mjs, hosting a web build) and put each into the queue as a mark, once: the ones already pulled from that server are remembered in .argus/marks/pulled.json.",
        params: &[
            req("url", STRING, "the server's LAN or tailnet address, http://host:port/"),
            opt("author", STRING, "default player"),
        ],
        returns: "{server, pulled: [mark.drop's summary], already: how many were pulled before}",
    },
    MethodDoc {
        name: "edit.propose",
        doc: "Submit a transaction as a proposal instead of committing it (section 10): nothing changes in the world or the files until the human picks it. Proposing into an existing proposal adds a variant (B, C, ...); A is the game as it is. It is checked by applying it to a fork of the world now, and refused if it does not apply. Proposals are files in .argus/proposals/ and join the change feed; a proposal may touch what another author locked, since the human decides. Entity edits only (set, add, remove, spawn, despawn); a spawn with an id cannot be previewed.",
        params: &[
            req("tx", INTEGER, ""),
            opt("note", STRING, "the proposal's, for a new one; default the transaction's"),
            opt("proposal", INTEGER, "add this transaction as the next variant of that proposal"),
        ],
        returns: "{proposal, variant, feed: the feed row, summary: proposal.list's row}",
    },
    MethodDoc {
        name: "proposal.list",
        doc: "The proposals waiting for the human, oldest first, with each variant's note and edits, and the trial running, if any.",
        params: &[
            opt("all", BOOLEAN, "default false: pending only"),
        ],
        returns: "{pending, rows: [{id, status, author, note, scene, variants: [{label, note, ops}], picked, made_at}], trial: {proposal, showing, worlds, toggles} or null}",
    },
    MethodDoc {
        name: "proposal.get",
        doc: "All of a proposal: its variants with the edit calls that make them, and what became of it (who picked or turned it down, how often they switched, and at which ticks).",
        params: &[
            req("id", INTEGER, ""),
        ],
        returns: "the proposal's file",
    },
    MethodDoc {
        name: "proposal.try",
        doc: "Try a proposal by feel: a fork of the world now for each variant, its edits applied in memory; A, the world as it is, stays in use. From then on every world of the trial takes the same input at the same tick, the ones not showing catching up behind the one that is (silently), so switching shows each at the same point. F9 in the window does this and then switches. A seek or a scene load ends a trial.",
        params: &[
            req("id", INTEGER, ""),
        ],
        returns: "{trial: {proposal, showing, worlds: [{label, world}], toggles}}",
    },
    MethodDoc {
        name: "proposal.show",
        doc: "Make one world of the trial the one in use and on screen, caught up to the same tick (what F9 does, going round A, B, C).",
        params: &[
            req("variant", STRING, "A, B, ..."),
        ],
        returns: "{trial}",
    },
    MethodDoc {
        name: "proposal.pick",
        doc: "Keep a variant: its edits are committed as an ordinary commit by whoever picked (the feed shows it, undo takes it back; their own locks do not stand in the way, another's do). In a trial the game goes on in the picked world when its edits are all field writes, else in A; the other worlds go. Picking A turns the proposal down. F10 in the window picks the world showing.",
        params: &[
            req("id", INTEGER, ""),
            req("variant", STRING, ""),
            opt("author", STRING, "default \"human\""),
            opt("note", STRING, "for A"),
        ],
        returns: "{proposal, commit: the feed row}",
    },
    MethodDoc {
        name: "proposal.reject",
        doc: "Turn a proposal down: nothing changes, a trial of it ends back in A, and the feed says so.",
        params: &[
            req("id", INTEGER, ""),
            opt("author", STRING, "default \"human\""),
            opt("note", STRING, ""),
        ],
        returns: "{proposal}",
    },
    MethodDoc {
        name: "proposal.stop",
        doc: "Stop trying: back to A, the other worlds dropped; the proposal waits on.",
        params: &[],
        returns: "{trial: null}",
    },
    MethodDoc {
        name: "tune.fields",
        doc: "The tune fields (tune = true) in the world with their values, defaults, ranges, and docs: what the editor's tuning panel will show (section 10). Every one of an entity, or of every entity with an id.",
        params: &[
            opt("entity", STRING, "optional"),
        ],
        returns: "{total, rows: [{entity, path, value, default, range, doc}], session: the open tuning session or null}",
    },
    MethodDoc {
        name: "tune.begin",
        doc: "Open a tuning session (tune.set opens one too). Its changes are live only until tune.end commits the final values as one transaction by its author.",
        params: &[
            opt("author", STRING, "default \"human\""),
            opt("note", STRING, ""),
        ],
        returns: "{session: {id, author, note, fields}}",
    },
    MethodDoc {
        name: "tune.set",
        doc: "The slider, until the editor has one: set a tune field in the live world now, with no file written, and log it with its frame (section 10). Opens a tuning session if none is open. A field that is not a tune field is refused; edit.set changes any field.",
        params: &[
            req("entity", STRING, ""),
            req("path", STRING, "Component.field"),
            req("value", ANY, ""),
            opt("author", STRING, "default \"human\", when it opens the session"),
        ],
        returns: "{entity, path, from, to, session}",
    },
    MethodDoc {
        name: "tune.end",
        doc: "End the tuning session: its final values are committed as one ordinary transaction by the session's author (one feed row; one undo goes back to where the session started), or with commit: false put back as they were. Closing the game's window, or loading a scene, ends a session and commits it.",
        params: &[
            opt("commit", BOOLEAN, "default true"),
            opt("note", STRING, "the transaction's"),
        ],
        returns: "{session, fields, commit: the feed row or null}",
    },
    MethodDoc {
        name: "tune.log",
        doc: "The tuning log, .argus/tuning.jsonl: every change to a tune field with its frame, author, value before and after, and how it came (tune, edit, undo, redo), and sessions begun and ended. story is each field's search path in one line: \"human set @player CharacterController.jump_height 3.2→3.6 at frame 3011, →3.4 at frame 3090\". obs.subscribe {topics: [\"tune\"]} streams the rows.",
        params: &[
            opt("since", INTEGER, "rows after this seq"),
            opt("session", INTEGER, ""),
            opt("path", STRING, "a field, or a component's prefix"),
            opt("limit", INTEGER, "default 200"),
        ],
        returns: "{total, story: [string], rows: [{seq, kind, at, frame, author, entity, path, from, to, via, session, note}], session}",
    },
    MethodDoc {
        name: "replay.save",
        doc: "Save this moment as a replay, with no queue: a replay file (the keyframe five seconds back, the input since, the world now), the frame, a two-second clip and its contact sheet, the last two seconds of sound, and replay.json (the run's identity, the digest, the input and events of those seconds, a note, and what about says). test.run saves one for each failing run (section 9).",
        params: &[
            opt("dir", STRING, "default .argus/replays/<time>-<frame>; relative to the project; replaces what was there"),
            opt("note", STRING, ""),
            opt("clip", BOOLEAN, "default true"),
            opt("software", BOOLEAN, "draw with the software rasteriser"),
            opt("about", ANY, "any JSON kept with it"),
        ],
        returns: "{dir, frame, scene, note, about, picture, clip, sheet, sound: absolute paths, ms}",
    },
    MethodDoc {
        name: "replay.open",
        doc: "Reproduce a saved replay (the directory replay.save or a failing test wrote, or its replay.bin): load its scene with its seed, restore its keyframe, and play the seconds before it so obs.events and obs.trace hold them, stopping at its frame. Then compare the world with the one it saved, field by field. Paused there; sim.seek moves within the lead-in, and an open window shows it (the editor opens these in phase 4).",
        params: &[
            req("path", STRING, "relative to the project"),
        ],
        returns: "{frame, from, replayed, against_mark: {same, appeared, vanished, changed, first}, digest: {saved, now, same}, code: {saved, now}, remapped, dropped, missing_effects, replay: replay.save's summary, ms}",
    },
    MethodDoc {
        name: "sim.status",
        doc: "Clock state.",
        params: &[],
        returns: "{playing, paused, clock, hz, speed, tick, time, frame, seed, end: the furthest frame this run has reached}",
    },
    MethodDoc {
        name: "script.reload",
        doc: "Recompile changed Luau files, rebind their systems in place, keep world state, tick, and RNG streams, and re-read def tables.",
        params: &[],
        returns: "{reloaded: [file], removed: [file], errors: integer}",
    },
    MethodDoc {
        name: "script.systems",
        doc: "Every system with stage, ordering, declared reads and writes, clock, RNG stream, enabled state, and source.",
        params: &[
            opt("limit", INTEGER, "rows per page"),
            opt("cursor", STRING, "the next page, from a previous answer's cursor"),
        ],
        returns: "{total, rows: [{name, stage, after, reads, writes, clock, rng, enabled, disabled_reason, source, doc}], next}",
    },
    MethodDoc {
        name: "script.errors",
        doc: "Outstanding script errors as file:line data, never a crash. A system a fault disabled is a row with phase \"run\", the system, and the frame, while it stays disabled. Materials are code too: a shader that does not compile, or a material sidecar that does not read, is a row with phase \"material\", the file and line of the mistake, and the asset.",
        params: &[],
        returns: "{total, rows: [{file, line, message, phase, ref, system?, frame?, asset?}]}",
    },
    MethodDoc {
        name: "obs.events",
        doc: "The structured event log: spawns, despawns, gameplay events, system faults, room transitions, edits. Filter by name, entity, or frame; page with since. by names the system that emitted an event; it is absent for the daemon's own (edits, the mixer's answers).",
        params: &[
            opt("since", INTEGER, "sequence number, exclusive"),
            opt("name", STRING, "optional"),
            opt("entity", STRING, "optional"),
            opt("from_frame", INTEGER, ""),
            opt("limit", INTEGER, "rows per page"),
            opt("cursor", STRING, "the next page, from a previous answer's cursor"),
        ],
        returns: "{total, rows: [{seq, frame, name, by?, fields}], next, last_seq}",
    },
    MethodDoc {
        name: "obs.diff",
        doc: "What changed since a frame (section 8 L3), in about 1 KB: the entities that appeared and vanished, by kind (their game components) with counts and the first ids; and the entities that changed, with each field and how many changed it, the components gained and lost, and the named entities first with what each field went from and to. The world is kept at each frame something looked at it (any world.* or obs.* call), the last few; since is compared with the newest of those at or before it (from). With no since, it is the last look: what changed since I last looked.",
        params: &[
            opt("since", INTEGER, "a frame, as world.summary or sim.status gave it; default the last look"),
            opt("limit", INTEGER, "entities listed with their changes; default 12"),
        ],
        returns: "{since, from, to, appeared: {count, kinds: [[kind, count, ids]]}, vanished: {...}, changed: {count, fields: [[path, count]], components: {added, removed}, entities: [[id, {path: [from, to]}, {more_fields?, added?, removed?}?]], omitted}, bytes}",
    },
    MethodDoc {
        name: "obs.trace",
        doc: "One entity's component history over a window of frames (section 8): every field that changed in it, from what to what and on which frames (the first and last points of a field that changed often); the components it gained and lost; its events; and the systems that write the components that changed. Frames are those events carry. Named entities are traced from the start, for the last 3,600 frames; any other is watched from the first time it is asked for. A gone entity is found by the id it had.",
        params: &[
            req("entity", STRING, "@id or e:N"),
            opt("frames", ANY, "integer (the last N) or [from, to]; default all kept"),
            opt("fields", ARRAY, "[Component; or Component.field]; default every field"),
            opt("points", INTEGER, "per field, default 12"),
        ],
        returns: "{entity, traced, from, to, fields: [{path, start, end, changes, points: [[frame, value]], skipped?}], steady, components: [[frame, component, added|removed]], event_columns, events, writers: {component: [system]}, gone?, bytes}",
    },
    MethodDoc {
        name: "obs.why",
        doc: "Walk the declared write sets backwards from an event (section 8): the system that emitted it, then the systems that write what that system reads, and so on for depth steps, each with why, in stage order, with the file:line that declares it. A static answer from the declarations; obs.trace says what actually changed. For an event no system emitted, the systems that write the entities it names.",
        params: &[
            opt("event", INTEGER, "one of event, name; a seq from obs.events"),
            opt("name", STRING, "one of event, name; the latest such event"),
            opt("depth", INTEGER, "default 3"),
        ],
        returns: "{event, columns: [system, stage, depth, because, source], rows, entities: [{id, writers: {component: [system]}}], note?}",
    },
    MethodDoc {
        name: "obs.frame",
        doc: "What the player sees. format \"semantic\" (section 8 L4) is data, about 3 KB, and needs no GPU: the camera and the world rect it shows; hud, the lines of entities on the screen rather than in the world; entities, the described and named ones in view with their Describe text filled in, bounds in pixels (x0, y0, x1, y1, y down) of a width x height frame, z, and how they are drawn, in reading order; groups, everything else by kind (their game components, else their name or look) with a count, bounds, and the first ids; effects on screen (camera shake, particles, materials); tiles, what the tilemaps in view collide with, a character a cell; and bytes. The same frame is a screen-reader feed. format \"png\" (section 8 L5) renders the world as it is now and writes a PNG; with annotate: true the ids and bounds of the semantic frame are drawn over it (yellow for entities, cyan for groups) and returned as marks. Sprites come from the world and the view from the drawing camera (the active Camera with the highest priority, its shake included); described shapes need no asset.",
        params: &[
            opt("format", STRING, "\"png\" (default) | \"semantic\""),
            opt("width", INTEGER, "default 640"),
            opt("height", INTEGER, "default 360"),
            opt("limit", INTEGER, "entities listed one by one; default 24"),
            opt("groups", INTEGER, "groups listed; default 12"),
            opt("annotate", BOOLEAN, "png only"),
            opt("path", STRING, "default .argus/frames/frame-<tick>.png"),
            opt("software", BOOLEAN, "force the software rasteriser"),
        ],
        returns: "png: {path, width, height, tick, alpha, sprites, draw_calls, atlases, missing_atlases, interpolated, particles, materials, missing_materials, material_problems, camera, view: {center, half, angle, rect: [x0, y0, x1, y1]}, adapter, ms, marks?}; semantic: {format, size, camera: {id, rect, angle}, hud: [{id, name?, role?, text, bounds?}], columns: [id, name, text, bounds, z, look], entities, groups: [{what, count, bounds, look, ids}], effects, omitted, problems, tick, time, tiles?: {key, maps: [{entity, map, cell, step, at: [x, y, px a cell], rows}]}, bytes}",
    },
    MethodDoc {
        name: "obs.annotate",
        doc: "obs.frame with format \"png\" and annotate: true: the picture with the ids and bounds of what is on it drawn over it, and those marks returned, so what is seen can be matched to what can be queried (section 8 L5).",
        params: &[
            opt("width", INTEGER, "as obs.frame"),
            opt("height", INTEGER, "as obs.frame"),
            opt("limit", INTEGER, "as obs.frame"),
            opt("groups", INTEGER, "as obs.frame"),
            opt("path", STRING, "as obs.frame"),
            opt("software", BOOLEAN, "as obs.frame"),
        ],
        returns: "as obs.frame's png, with marks: [[label, bounds, labelled]]",
    },
    MethodDoc {
        name: "obs.subscribe",
        doc: "Receive notifications on this connection: `event` for each new event, `edit.committed` for each commit (and proposal made or turned down), `tune.changed` for each tuning log row. Topics: events, edits, tune.",
        params: &[
            opt("topics", ARRAY, "[\"events\" | \"edits\" | \"tune\"]"),
            opt("off", BOOLEAN, "unsubscribe"),
        ],
        returns: "{topics}",
    },
    MethodDoc {
        name: "test.list",
        doc: "The scenarios under tests/*.luau: name, file, scene, seeds. Files that fail to load are listed, not fatal.",
        params: &[],
        returns: "{total, rows: [{name, file, scene, seeds, runs}], load_errors: [{file, message}]}",
    },
    MethodDoc {
        name: "test.run",
        doc: "Run scenarios headless in this daemon: each seed loads the scene fresh, scripts input, runs frames, and checks expectations; state goldens live in tests/goldens/ and a mismatch is a field-level diff. A failing run saves a replay the moment it fails (the first failed expectation, or the error) under .argus/replays/<scenario>/seed<n>/, with a clip of the seconds before it for the first five failures; replay.open reproduces one (section 9). The scene that was open is reloaded afterwards. `argus test` is this.",
        params: &[
            opt("name", STRING, "optional; exact or substring"),
            opt("file", STRING, "optional, tests/x.luau"),
            opt("seeds", STRING, "optional override: \"7\", \"1..50\", \"1,2,9\""),
            opt("update", BOOLEAN, "rewrite goldens"),
            opt("replays", BOOLEAN, "default true"),
            opt("software", BOOLEAN, "draw clips with the software rasteriser, as CI does"),
        ],
        returns: "{ok, ms, total_runs, passed_runs, failed_runs, scenarios: [{name, file, scene, ok, seeds: [{seed, ok, ms, tick, error, expectations: [{kind, desc, ok, message, ...}], log, replay: replay.save's summary, for a failing run}]}], load_errors}",
    },
    MethodDoc {
        name: "render.window",
        doc: "Open or close the window the daemon presents into every frame (playable by a human, section 18 phase 1). Device events from it feed the input layer. Its close button emits window.closed; a window opened with --window at start is the person's, and closing it also stops the daemon, while one opened here only closes and can be opened again.",
        params: &[
            opt("open", BOOLEAN, "default true"),
            opt("width", INTEGER, "default 1280"),
            opt("height", INTEGER, "default 720"),
            opt("title", STRING, ""),
            opt("software", BOOLEAN, ""),
            opt("visible", BOOLEAN, "default true; false opens it hidden, taking no focus, for checking the windowed path while someone uses the machine"),
            opt("audio", BOOLEAN, "default true: open the audio output too"),
        ],
        returns: "render.status",
    },
    MethodDoc {
        name: "render.status",
        doc: "Whether a window is open, the adapter in use, and the last frame's statistics.",
        params: &[],
        returns: "{window, size: [w, h], adapter, software, last: {sprites, draw_calls, atlases, interpolated, missing_atlases, materials, missing_materials, material_problems, ms}, atlases: [name], materials: [name]}",
    },
    MethodDoc {
        name: "audio.status",
        doc: "The mixer: output device, buses with gain, ducking, and meters since the last read (peak and RMS dBFS of everything the bus sends on; master is the whole mix before the limiter, and master.limiter_db is how far the limiter turned it down), voices, the music track and its layers, sounds known, sounds asked for that do not exist, and load warnings. Audio is read as data (section 8): the event log has every sound.played.",
        params: &[],
        returns: "{device, rate, voices, played, master: {peak_db, rms_db, limiter_db}, columns, rows: [[bus, parent, gain_db, mute, duck_db, peak_db, rms_db, voices]], music, sounds, missing, warnings, error}",
    },
    MethodDoc {
        name: "audio.play",
        doc: "Play a sound as gameplay would (a sound.play event): the mixer picks the variation from the run seed and answers with sound.played.",
        params: &[
            req("sound", STRING, ""),
            opt("entity", STRING, "optional, spatial"),
            opt("gain", NUMBER, ""),
            opt("pitch", NUMBER, ""),
            opt("bus", STRING, ""),
            opt("pan", NUMBER, "-1..1, when not spatial"),
            opt("loop", BOOLEAN, ""),
        ],
        returns: "the sound.played (or sound.missing) event row",
    },
    MethodDoc {
        name: "audio.stop",
        doc: "Stop a voice, or every voice playing on an entity.",
        params: &[
            opt("voice", INTEGER, "a voice audio.play started"),
            opt("entity", STRING, "every voice playing on this entity"),
            opt("fade_ms", NUMBER, ""),
        ],
        returns: "{stopped: [voice]}",
    },
    MethodDoc {
        name: "audio.bus",
        doc: "Set a bus's gain or mute (tuning). Lasts until the scene reloads; the file is audio.toml.",
        params: &[
            req("bus", STRING, ""),
            opt("gain_db", NUMBER, ""),
            opt("mute", BOOLEAN, ""),
        ],
        returns: "audio.status",
    },
    MethodDoc {
        name: "audio.clip",
        doc: "Write the last seconds of the master output as a WAV, for a human to hear (section 10). Headless, the mixer runs on sim time, so the clip is the same on every run.",
        params: &[
            opt("seconds", NUMBER, "default 5, at most 10"),
            opt("path", STRING, "default .argus/clips/audio-<tick>.wav"),
        ],
        returns: "{path, seconds, frames, peak_db}",
    },
    MethodDoc {
        name: "audio.device",
        doc: "Open or close the system's audio output. render.window opens it; headless runs never need it.",
        params: &[
            opt("open", BOOLEAN, "default true"),
        ],
        returns: "audio.status",
    },
    MethodDoc {
        name: "prof.frame",
        doc: "The last tick: total time and per-system breakdown.",
        params: &[],
        returns: "{tick, clock, total_ms, allocs, bytes, systems: [[name, ms, allocs, bytes]], archetypes: [[id, entities]]}",
    },
    MethodDoc {
        name: "prof.worst",
        doc: "The n slowest recent ticks with per-system breakdown and entity counts by archetype.",
        params: &[
            opt("n", INTEGER, "default 5"),
        ],
        returns: "{rows: [{tick, clock, total_ms, allocs, bytes, systems, archetypes}]}",
    },
    MethodDoc {
        name: "prof.systems",
        doc: "Per-system mean and p95 over the last n ticks, in stage order.",
        params: &[
            opt("last", INTEGER, "default 600"),
        ],
        returns: "{last, tick_p95_ms, rows: [[name, stage, mean_ms, p95_ms, max_ms, samples]]}",
    },
    MethodDoc {
        name: "prof.archetypes",
        doc: "Entity counts and bytes by archetype.",
        params: &[],
        returns: "{rows: [[components, entities, bytes]]}",
    },
    MethodDoc {
        name: "prof.alloc",
        doc: "Heap allocations by system over the last n ticks, in stage order: a Luau system that builds a table per entity shows up here. Counted on the sim thread by argusd's allocator; `counted` is false in a build without it.",
        params: &[
            opt("last", INTEGER, "default 600"),
        ],
        returns: "{last, counted, tick_mean_allocs, tick_mean_bytes, rows: [[name, stage, mean_allocs, mean_bytes, max_allocs, max_bytes, samples]]}",
    },
    MethodDoc {
        name: "prof.capture",
        doc: "Capture the next n ticks as a trace file for a human: every system span on one time base with its allocation counts, in the Chrome trace event format that Tracy imports (tracy-import-chrome) and Perfetto opens. Written to .argus/captures/ when the last tick is in; a `prof.capture` event names the file. Without `ticks`, reports the capture in progress.",
        params: &[
            opt("ticks", INTEGER, "1 to 100000; without it, the capture in progress"),
            opt("path", STRING, "optional, where to write"),
        ],
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
            out.push_str(&params_text(m.params));
            out.push('\n');
        }
        out.push_str("  returns: ");
        out.push_str(m.returns);
        out.push_str("\n\n");
    }
    out
}
