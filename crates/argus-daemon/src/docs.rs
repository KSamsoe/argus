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
/// work through the human's marks, proposals, and review requests; every
/// method stays reachable through the CLI.
pub const TOOLS: &[ToolDoc] = &[
    ToolDoc {
        name: "summary",
        doc: "One-screen orientation: entity counts by archetype, named entities, active systems, frame, seed, clock. The MCP server runs it on connect; run it again after a scene load or a big edit.",
        params: &[opt("scene", STRING, "another scene of the project, from the index; default the one loaded")],
        calls: &["world.summary"],
    },
    ToolDoc {
        name: "query",
        doc: "Read the world: one entity in full, or the entities matching a filter with the fields you select, as compact rows. With entity: that entity, every component and field. With path: the way from one place to another over the navigation grid (nav.path). Otherwise select, where, and order, e.g. select [\"id\", \"Health.cur\"], where {has: [\"Enemy\"], \"Health.cur\": {lt: 3}}, order \"dist(@player)\". Aggregates answer one row a group: select [\"sum(Stockpile.food)\"], or group_by \"Job.kind\" with select [\"count()\", \"mean(Job.priority)\"].",
        params: &[
            opt("entity", STRING, "\"@id\" or \"e:n\": one entity in full"),
            opt("path", OBJECT, "{from, to, grid?}: from and to an entity (\"@player\") or [x, y]; the waypoints, steps, and cost, or why there is no way"),
            opt("strings", ANY, "a language's code, or {lang, prefix, missing}: the game's player-facing strings in that language beside the base's (strings.list)"),
            opt("timelines", ANY, "true, or a timeline's name: the timelines with their tracks and cues in time order, and those playing with where they are (timeline.list)"),
            opt("mods", BOOLEAN, "true: the mods over the game, what each added and changed, those left out and why, and those in the project's mods/ (mods.list)"),
            opt("select", ARRAY, "[\"id\" | \"Component.field\" | \"Component\" | \"count()\" | \"sum(Component.field)\" | mean | min | max]; default [\"id\"]"),
            opt("where", OBJECT, "{has: [Component], not: [Component], within: {of: entity, r: number}, \"Component.field\": value | {eq|ne|lt|lte|gt|gte: value}}"),
            opt("group_by", ARRAY, "[\"Component.field\"]: one row a distinct value, with the aggregates in select"),
            opt("order", STRING, "\"Component.field\", \"-Component.field\", or \"dist(entity)\"; grouped, a column: \"-count()\""),
            opt("limit", INTEGER, "default 50"),
            opt("scene", STRING, "another scene, from the index"),
        ],
        calls: &["world.get", "world.query", "nav.path", "strings.list", "timeline.list", "mods.list"],
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
        doc: "Change the game's authored state (its files) in one transaction by an author with a note, so the change feed says who and why and undo takes it back. ops run in order and all are kept or none: each is {op: set|add|remove|spawn|despawn|tiles|text|def|asset, ...} with that edit method's parameters, e.g. {op: \"set\", entity: \"@player\", path: \"CharacterController.jump_height\", value: 3.4}, or {op: \"asset\", asset: \"anim/hero\", path: \"states.run.fps\", value: 10} for an animator's clip rate. undo: take back the author's last transaction instead.",
        params: &[
            opt("ops", ARRAY, "one of ops, undo; [{op, ...}]: set {entity, path, value}, add {entity, component}, remove {entity, component}, spawn {id, room, prefab, tags, components}, despawn {entity}, tiles {file, layer, rect, cells | tile | terrain | erase}, text {lang, key, text} (a string in strings/<lang>.toml; text null removes it), def {def, path, value} (a def table row's field), asset {asset, path, value | remove} (a value in an asset's sidecar)"),
            opt("undo", BOOLEAN, "one of ops, undo; undo the author's last transaction"),
            opt("note", STRING, "why, for the change feed"),
            opt("author", STRING, "default \"agent:claude\""),
        ],
        calls: &["edit.begin", "edit.set", "edit.add", "edit.remove", "edit.spawn", "edit.despawn", "edit.tiles", "edit.text", "edit.def", "edit.asset", "edit.commit", "edit.abort", "edit.undo"],
    },
    ToolDoc {
        name: "run",
        doc: "Advance the simulation: step ticks, run until something happens, take turns, play, pause, or stop; load a save first, save the game last. step: that many ticks; until: run until an event, a time, or a tick count; turns: take turns on a discrete clock; play, pause, speed, and stop (back to where play started); load and save: a save game by name, in .argus/saves/, through the migrations its components declare. With none, where the sim is. Always answers with sim.status after.",
        params: &[
            opt("step", INTEGER, "ticks to run"),
            opt("until", OBJECT, "{event: name} | {time: seconds} | {ticks: n}; max_ticks: n (default 100000)"),
            opt("turns", INTEGER, "turns to take on a discrete clock"),
            opt("play", BOOLEAN, "start playing in real time (a window, if open, shows it)"),
            opt("pause", BOOLEAN, "pause"),
            opt("stop", BOOLEAN, "stop and go back to the frame play started from"),
            opt("speed", NUMBER, "ticks per real-time frame; 0 pauses"),
            opt("load", STRING, "a save's name, loaded before anything else"),
            opt("save", STRING, "save the game under this name, after everything else"),
        ],
        calls: &["sim.load", "sim.stop", "sim.play", "sim.pause", "sim.speed", "sim.step", "sim.run_until", "sim.advance", "sim.save", "sim.status"],
    },
    ToolDoc {
        name: "seek",
        doc: "Go to any frame of this run, rebuilt from keyframes and the recorded input; stepping on replays what happened next until a new input branches the run. fork: instead build a second world at that frame, named, to try something beside this one (every method then takes fork: name). timeline and time: scrub a timeline, to the first frame at which it had reached that many seconds. With none of these: the run's range, keyframes, and changes (sim.ring). replays: the saved replays, a failing test's among them; replay: open one (its path from that list).",
        params: &[
            opt("frame", INTEGER, "one of frame, turn, time; the tick"),
            opt("turn", INTEGER, "one of frame, turn, time; the turn, on a discrete clock"),
            opt("time", NUMBER, "one of frame, turn, time; seconds into the timeline named by timeline"),
            opt("timeline", STRING, "with time: a timeline's name (its latest play) or its entity (\"e:40\")"),
            opt("fork", STRING, "a name: fork there instead of seeking"),
            opt("replays", BOOLEAN, "list the saved replays"),
            opt("replay", STRING, "open this replay: its path from replays"),
        ],
        calls: &["sim.seek", "sim.fork", "timeline.seek", "sim.ring", "replay.list", "replay.open"],
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
        name: "profile",
        doc: "How fast it runs, as data (section 5: a perf regression is a query result with a system name on it). Default: section 17's targets for this game beside what is measured now, each over-target row naming the system to look at. systems: each system's mean and p95 in stage order, with its allocations; worst: the slowest recent ticks with their systems and archetypes; calls: how long protocol calls took; capture: record the next n ticks as a trace a human opens in Perfetto or Tracy.",
        params: &[
            opt("systems", BOOLEAN, "each system's time and allocations"),
            opt("worst", INTEGER, "the n slowest recent ticks"),
            opt("calls", BOOLEAN, "protocol calls' times"),
            opt("capture", INTEGER, "ticks to record as a trace file"),
            opt("last", INTEGER, "ticks to cover; default 600"),
        ],
        calls: &["prof.targets", "prof.systems", "prof.alloc", "prof.worst", "prof.calls", "prof.capture"],
    },
    ToolDoc {
        name: "events",
        doc: "What happened: the event log (collisions, deaths, sounds, faults, anything a system emitted), by name, entity, or since a sequence number. why: the systems and events that led to one event. trace: an entity's fields frame by frame.",
        params: &[
            opt("name", STRING, "an event name, e.g. \"player.fell\""),
            opt("entity", STRING, "events naming this entity"),
            opt("since", INTEGER, "events after this sequence number"),
            opt("limit", INTEGER, "default 50"),
            opt("why", ANY, "an event's sequence number, or a name for the latest such event; job.idle: why that worker is idle, job by job"),
            opt("trace", OBJECT, "{entity, fields: [Component or Component.field], frames: n or [from, to]}"),
        ],
        calls: &["obs.events", "obs.why", "obs.trace"],
    },
    ToolDoc {
        name: "test",
        doc: "Run the project's scenarios (tests/*.luau): each run's pass or fail with the expectations that failed; a failing run saves a replay of the moment it failed. set tries values without writing them; sweep runs the scenarios once for each candidate value of one field and says which candidates fail (the human tuned by hand; this verifies by machine).",
        params: &[
            opt("name", STRING, "a scenario, exact or a substring; default all"),
            opt("file", STRING, "only tests/x.luau"),
            opt("seeds", STRING, "\"7\", \"1..50\", or \"1,2,9\""),
            opt("update", BOOLEAN, "rewrite the state goldens"),
            opt("set", ARRAY, "[{entity | def, path, value}]: into each run's world after its scene loads, live only"),
            opt("sweep", OBJECT, "{entity | def, path, values: [...]}: every run once for each value"),
        ],
        calls: &["test.run"],
    },
    ToolDoc {
        name: "reload",
        doc: "Take changes made on disk now: scripts are recompiled and swapped in while the world goes on from the current frame, and assets are reimported. Answers with what changed and every script error. With mods: the scene loaded again from its start with those mods from the project's mods/ laid over the game, in that order ([] for none), answering with mods.list.",
        params: &[opt("mods", ARRAY, "mod ids in load order: load the scene again with them (the world starts over)")],
        calls: &["script.reload", "asset.import", "script.errors", "scene.load", "mods.list"],
    },
    ToolDoc {
        name: "marks",
        doc: "The human's marks: moments they flagged while playing, each with a note, in a queue. list (default): the open ones. open: reproduce one (its scene, the seconds before it in the log, the world compared with the one they saw). get; note (replace the mark's note, the human's words); close (say what was done). import: a player's report file; pull: the reports a playtest server collected.",
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
    ToolDoc {
        name: "review",
        doc: "Ask the human to judge a moment by feel: a question and the clip of its frames go into the editor's agent panel, where they scrub it and answer with a key or a sentence, at their pace. Ask after a change whose effect is a feeling (\"does the enemy death read as a kill?\"); keep working, and read the answers later with list. A clip is at most ten seconds; frames default to the last two.",
        params: &[
            opt("question", STRING, "one of question, list"),
            opt("frames", ARRAY, "[from, to]; default the last two seconds"),
            opt("options", ARRAY, "the choices, default [\"yes\", \"no\"]; the human can always write a sentence"),
            opt("note", STRING, "what you changed, or why you ask"),
            opt("list", BOOLEAN, "one of question, list: the requests and their answers"),
            opt("all", BOOLEAN, "with list: the answered ones too (default true)"),
        ],
        calls: &["obs.review", "review.list"],
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
            opt("mods", ARRAY, "mod ids from the project's mods/, in load order (phase 3 item 8); default none"),
        ],
        returns: "the world.summary, plus issues: [] when the scene could not be instantiated, run_from: the seq of the run's first event (after the load's own; what on_load systems emit is the run's), and with mods, mods: {loaded, left_out: [{id, why}], changes}",
    },
    MethodDoc {
        name: "session.status",
        doc: "What is open: project, scene, seed, clock, and whether the sim is playing.",
        params: &[],
        returns: "{project, scene, seed, clock, playing, paused, tick, time, frame}",
    },
    MethodDoc {
        name: "session.identify",
        doc: "Say who this connection is (phase 4): the author the change feed names for this connection's calls that change something, where a call has no author param of its own. The editor says \"human\" and argus mcp \"agent:claude\"; a connection that says nothing is \"cli\".",
        params: &[req("author", STRING, "who, e.g. \"human\" or \"agent:claude\"")],
        returns: "{author}",
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
        doc: "Select field paths from entities matching a filter. Rows are compact arrays in select order; total is always present; next is a cursor for the rest. With aggregates in select (count(), sum(F), mean(F), min(F), max(F)) or a group_by, the answer is one row a group instead of one an entity: select [\"sum(Stockpile.food)\"] is the colony's food; group_by \"Job.kind\" with select [\"count()\"] is the job queue by kind. In a scene with rooms the live world holds only the rooms loaded around the followed entity: the answer says how many it could not see (unloaded_rooms), where.room names rooms, and a query naming a room that is not loaded is answered by the index from the files (source \"index\").",
        params: &[
            opt("select", ARRAY, "[\"id\" | \"room\" | \"Component.field\" | \"Component\" | \"count()\" | \"sum(Component.field)\" | mean | min | max]; default [\"id\"], or [\"count()\"] with group_by"),
            opt("where", OBJECT, "{has: [Component], not: [Component], room: name | [names], within: {of: entity, r: number}, \"Component.field\": value | {eq|ne|lt|lte|gt|gte: value}}"),
            opt("group_by", ARRAY, "[\"Component.field\"] (or one as a string): one row a distinct value, the value leading the row; only entities with those components"),
            opt("order", STRING, "\"Component.field\" | \"-Component.field\" | \"dist(entity)\"; grouped, a column of the answer: \"-sum(Stockpile.food)\""),
            opt("limit", INTEGER, "default 50"),
            opt("cursor", STRING, ""),
            opt("scene", STRING, "optional: another scene, from the index"),
            opt("source", STRING, "\"index\""),
        ],
        returns: "{frame, total, rows: [[...]], truncated, next, unloaded_rooms?, source?}; grouped, also {matched: entities, columns: [...]}",
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
        doc: "Reload the current scene, or switch to another one. Re-reads scripts, prefabs, defs, and files, and rebuilds the world. A scene loaded while playing plays on, paused or at its speed as it was; a scenario's loads start stopped.",
        params: &[
            opt("scene", STRING, "default: current"),
            opt("seed", INTEGER, ""),
            opt("mods", ARRAY, "mod ids in load order; default: the session's"),
        ],
        returns: "the world.summary, plus issues when the scene is not clean, run_from: the seq of the run's first event (after the load's own; what on_load systems emit is the run's), and with mods, mods: {loaded, left_out, changes}",
    },
    MethodDoc {
        name: "mods.list",
        doc: "The mods laid over the game (phase 3 item 8, section 15), in load order: each one's manifest and what it added; the mods left out and every reason (a name outside the mod's namespace, a row or an entity the game's schemas refuse, a script that does not load, a mod it needs left out); what the mods changed of the game's or an earlier mod's (def rows, strings, assets, sounds, tracks, entities, prefabs), the later winning; and every mod in the project's mods/ directory. Everything a mod adds is named with its id first (lanterns.flicker, lanterns_Lantern, lanterns/lantern, lanterns-vigil).",
        params: &[],
        returns: "{enabled, rule, loaded: [{id, name, version, game, requires, doc, kind, from, hash, adds: {scripts, components, def rows, rooms, entities, assets, sounds, strings}}], left_out: [{id, why: [{code, message, hint, at}]}], changes: [{kind, name, by, over, at}], available: [{id, name, version, game, requires, doc, hash}]}",
    },
    MethodDoc {
        name: "scene.issues",
        doc: "Everything wrong with the current scene's files, each with file:line. Empty when the scene is loaded.",
        params: &[],
        returns: "{total, rows: [{code, message, hint, ref}]}",
    },
    MethodDoc {
        name: "scene.rooms",
        doc: "Rooms of the current scene with graph neighbours, bounds, entity counts, loaded state, and which is current. Rooms stream around the scene's followed entity ([rooms] follow) as it moves; always-loaded rooms ([rooms] always) never stream out. room: one room, and what it remembers of its entities while it is not loaded (fields changed from their files, components added or taken away, entities gone), which it puts back when it streams in.",
        params: &[
            opt("room", STRING, "one room: what it remembers"),
            opt("scene", STRING, "optional: another scene, from the index"),
            opt("source", STRING, "\"index\""),
        ],
        returns: "{rooms: [{name, loaded, always, remembered, entities, neighbors, bounds, bulk}], current, radius, follow, live}; with room, {room, loaded, entities, remembered: [{entity, gone, added, removed, set: [[path, value]]}]}",
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
        name: "nav.grids",
        doc: "Every navigation grid (section 12, phase 3 item 2): a NavGrid entity's cells over the world, each blocked or open with a step cost, built from a tilemap's collision (solid tiles blocked), a baked grid, or a plain rectangle of its fill, with the NavCost entities over it (a wall, a building, mud). Each with its origin, cell size, size in cells, open and blocked cells, what it is built from, and its hash (equal hashes, equal grids).",
        params: &[],
        returns: "{columns: [grid, origin, cell, size, open, blocked, from, hash], rows, total}",
    },
    MethodDoc {
        name: "nav.grid",
        doc: "A navigation grid as rows of characters, top row first: # blocked, . cost 1, 2-9 that cost, + above 9. Past max columns or rows it shows every nth cell (step).",
        params: &[
            opt("grid", STRING, "the NavGrid entity; default the first"),
            opt("rect", ARRAY, "[x0, y0, x1, y1] in world units; default the whole grid"),
            opt("max", INTEGER, "most columns or rows drawn (default 120)"),
        ],
        returns: "{grid, origin, cell, size, first: [column, row], step, rows, legend}",
    },
    MethodDoc {
        name: "nav.path",
        doc: "The way from one place to another on a navigation grid, by A* (section 12): the cells' centres after the start and the goal itself last, its steps, and what it costs in steps of cost 1 (a diagonal is 1.4). An entity stands for where it is. Scripts ask the same with q:path; a test with t.expect:path(entity, filter):exists(). found is false with why when there is no way; grid is null when no grid covers from.",
        params: &[
            req("from", ANY, "an entity (\"@player\") or a place [x, y]"),
            req("to", ANY, "an entity or a place"),
            opt("grid", STRING, "the NavGrid entity; default the grid from is on"),
        ],
        returns: "{found, grid, from_cell, to_cell, steps, cost, points, expanded, cached, ms, why?}",
    },
    MethodDoc {
        name: "nav.field",
        doc: "A NavField entity's flow field (section 12, for crowds): every cell's steps to the cell the entity stands in, as rows of characters top row first: 0-9 then a-z, + beyond 35, # blocked, blank where it does not reach. Scripts read it with q:distance and q:downhill; NavAgents with field set walk down it.",
        params: &[
            req("of", STRING, "the entity with the NavField"),
            opt("rect", ARRAY, "[x0, y0, x1, y1] in world units; default the whole grid"),
            opt("max", INTEGER, "most columns or rows drawn (default 120)"),
        ],
        returns: "{of, target: [column, row], reached, first, step, rows, legend}",
    },
    MethodDoc {
        name: "nav.bake",
        doc: "Write a navigation grid as bulk data (section 4): a text index (<name>.navgrid.toml: size, cell, origin, open and blocked cells, a hash) and a binary chunk beside it, one byte a cell. A NavGrid whose baked names the index starts from it instead of deriving its tilemap; NavCost entities still go over it. The scene loader leaves the index alone.",
        params: &[
            req("grid", STRING, "the NavGrid entity"),
            opt("file", STRING, "project-relative *.navgrid.toml; default scenes/<scene>/<grid id>.navgrid.toml"),
        ],
        returns: "{grid, index, chunk, bytes, open, blocked, hash, use}",
    },
    MethodDoc {
        name: "strings.list",
        doc: "The game's player-facing strings (section 12): a language's table beside the base language's, key by key, and every language with how many strings it has and lacks. missing: only the keys the language lacks, a translator's to-do list. A key no table has shows on screen as [key]; one a language lacks shows in the base language.",
        params: &[
            opt("lang", STRING, "a language's code; default the base language"),
            opt("prefix", STRING, "only keys starting with this, e.g. \"menu.\""),
            opt("missing", BOOLEAN, "only the keys lang lacks"),
            opt("limit", INTEGER, "default 50"),
            opt("cursor", STRING, ""),
        ],
        returns: "{base, lang, columns: [key, <base>, <lang>], total, rows, truncated, next, languages: [{code, name, strings, missing}], problems?}",
    },
    MethodDoc {
        name: "jobs.why",
        doc: "Why a worker does what it does (section 12): every job, in the order the worker weighs them (priority, its JobKind's included, then distance), with what it makes of each: could take it, has it, another has it, only for another, needs a skill, out of reach. An idle worker's why says why it has none, as Worker.idle and the job.idle event do. obs.why on a job.* event naming a worker answers with this too. The queue itself is a query: world.query {group_by: \"Job.kind\", select: [\"count()\"]}.",
        params: &[
            req("worker", STRING, "the Worker entity (\"e:12\" or \"@id\")"),
            opt("limit", INTEGER, "default 50"),
            opt("cursor", STRING, ""),
        ],
        returns: "{worker, job, why?, columns: [job, kind, priority, stage, distance, verdict], total, rows, truncated, next}",
    },
    MethodDoc {
        name: "timeline.list",
        doc: "The game's timelines (section 13): each row of the timeline table without a timeline field, its length, and its cues by track in time order (a tween, an animator state, a camera cut, a conversation, or edits and events, about the actor, the other, the drawing camera, or an @id; whether it waits), and the Timeline entities playing, with their time, next cue, and what they wait for. Rows that do not read are in problems (and script.errors, phase timeline). A timeline plays when a script spawns a Timeline {name, actor, other}.",
        params: &[opt("name", STRING, "only this timeline")],
        returns: "{timelines: [{name, length, ref, cue_columns: [n, cue, at, end, kind, who, what, wait, ref], tracks: [{track, cues}]}], playing: [{timeline, name, time, length, next, held, waiting, began, actor, other}], problems: [{ref, message}]}",
    },
    MethodDoc {
        name: "timeline.seek",
        doc: "Scrub a timeline (section 13, \"scrubbable in the editor\"): go to the first frame of this run at which it had reached time seconds, replayed from the keyframe before it began, as sim.seek goes to a frame. A time past its end is the frame it ended; one it has not reached yet in this run is the run's last frame (reached false). Stepping on replays what happened next. The timeline is named by its entity, or by name for its latest play in the log.",
        params: &[
            req("time", NUMBER, "seconds into the timeline"),
            opt("timeline", STRING, "one of timeline, name; its entity (\"e:40\")"),
            opt("name", STRING, "one of timeline, name; its latest play"),
        ],
        returns: "sim.status plus {from, replayed, ms, checked, diverged, reached, timeline, time, next, held, ended}",
    },
    MethodDoc {
        name: "asset.list",
        doc: "Every asset under assets/: sprites (a PNG, or a sidecar describing a shape), sounds (a WAV, or a sidecar naming variations), animators (a sidecar with type = \"animator\": states, transitions, events by frame), particle effects (type = \"emitter\"), materials (a .wgsl shader, with a sidecar of typed parameters), and fonts (a .ttf or .otf, baked into a distance-field atlas of the glyphs text needs; a sidecar may set size, spread, and chars), with the warnings the importer found. A name is the path without the extension; sprites/hero is assets/sprites/hero.png with assets/sprites/hero.toml beside it. For an animator the frames and clips columns are its state count and names; for an emitter its pool size and sprite; for a material its parameter count and names; for a font its glyphs baked. kind = \"material\" also lists the engine's material library (unlit, lit, palette, dissolve, outline, sdf_text), source builtin:<name>, unless a project asset of the same name shadows one.",
        params: &[
            opt("kind", STRING, "\"sprite\" | \"sound\" | \"animator\" | \"emitter\" | \"material\" | \"font\"; optional"),
            opt("limit", INTEGER, "rows per page"),
            opt("cursor", STRING, "the next page, from a previous answer's cursor"),
        ],
        returns: "{total, columns: [name, kind, source, frames, clips, warnings], rows, failed: [{name, message, at}], next}",
    },
    MethodDoc {
        name: "asset.get",
        doc: "One asset as imported: frames with rects, pivots, durations, and properties; clips; the cached texture; a sound's bus, gain, and files with rate, channels, length, and peak; an animator's states; an emitter's rate, burst, and pool; a material's shader, blend, and parameters (type, default, range, from, doc); a font's family, size and spread in atlas pixels, glyphs baked, atlas size, and characters it lacks; every warning with file:line. A library material (unlit, lit, palette, dissolve, outline, sdf_text) answers too, with builtin: true.",
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
        name: "asset.sidecar",
        doc: "An asset's sidecar (its .toml) as data, with the file it is: what edit.asset changes, by the same paths. An animator's states with their clips, rates, wind-ups, holds, events by frame, and transitions; a sprite's frames and clips; an emitter's or a material's parameters.",
        params: &[
            req("name", STRING, ""),
            opt("path", STRING, "also the value at this path: states.run.fps, states.run.events[0].frame"),
        ],
        returns: "{name, kind, file, doc, value?}",
    },
    MethodDoc {
        name: "edit.asset",
        doc: "Queue a change to one value in an asset's sidecar (phase 4 item 8), by its path in the file: dotted keys and [n] for an element (states.run.fps, states.run.events[0].frame, states.idle.transitions[1].when, any[0].to). Tables on the way are made, [n] one past the end appends, remove takes a key or an element out, an object for a table's key makes a table of its own (a new state). At commit the file is written with its layout and comments kept and the asset imported again, so a running game uses it at once; a sidecar the importer refuses is put back and the commit fails with the importer's sentence and line. Undo puts back the file's exact text.",
        params: &[
            req("tx", INTEGER, "the transaction, from edit.begin"),
            req("asset", STRING, "the asset's name: anim/hero"),
            req("path", STRING, "states.run.fps"),
            opt("value", ANY, "one of value, remove"),
            opt("remove", BOOLEAN, "one of value, remove: take the key or element out"),
        ],
        returns: "{queued}",
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
        doc: "Stream the rooms around a room now, between ticks: rooms within the streaming radius (and the always-loaded ones) load, the rest unload remembering what happened in them, and a room.transition event is emitted. In play, rooms.stream does this when the followed entity crosses into another room.",
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
        name: "scene.tiles.list",
        doc: "The tilemaps placed in the world: each Tilemap entity, where its tile (0, 0)'s top-left corner is in world units (rows go down), and its index (size, tile size, layers with their tilesets and terrains). With a frame's view (render.frame's view), a point on screen is a cell: what the editor paints with.",
        params: &[],
        returns: "{rows: [scene.tiles.info plus {entity, origin: [x, y], visible}]}",
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
        name: "edit.text",
        doc: "Queue a string edit in a transaction: the words of one key in one language's table (strings/<lang>.toml, made if there is none; its layout and comments kept), or with text null the key removed. Applied at commit, undoable per author, in the change feed; the UI shows the new words at once.",
        params: &[
            req("tx", INTEGER, "the transaction, from edit.begin"),
            req("lang", STRING, "a language's code, e.g. \"fr\""),
            req("key", STRING, "the string's key, e.g. \"menu.resume\""),
            opt("text", STRING, "the words; null removes the key"),
        ],
        returns: "{queued}",
    },
    MethodDoc {
        name: "edit.def",
        doc: "Queue a def row edit in a transaction (phase 4 item 3): one field of one row of a data table (defs/<table>.toml, its layout and comments kept), checked against the table's schema. Applied at commit, undoable per author, in the change feed; scripts read the new value at once (q:def).",
        params: &[
            req("tx", INTEGER, "the transaction, from edit.begin"),
            req("def", STRING, "the row, prefix:key, e.g. \"item:meal\""),
            req("path", STRING, "the field, e.g. \"food\""),
            req("value", ANY, ""),
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
        doc: "The change feed: every change, from every client, with its author, frame, and the argus command that makes it (command). A commit also has its note, what it touched, and its ops, and its command is the whole transaction as the edit tool; any other call that changed something (play, a seek, a slider, a mark's note) is a row of kind \"command\" with its method and params. Lookups are not in it. Answers who changed this and why.",
        params: &[
            opt("since", INTEGER, "commit id, exclusive"),
            opt("author", STRING, "optional"),
            opt("limit", INTEGER, "rows per page"),
            opt("cursor", STRING, "the next page, from a previous answer's cursor"),
        ],
        returns: "{total, rows: [{id, author, note, frame, kind, touched, files, ops, command, method?, params?}], next}",
    },
    MethodDoc {
        name: "edit.lock",
        doc: "Fence entities or paths for an author, or whole assets and def rows. Another author's write into them fails naming the lock, its owner, and its note.",
        params: &[
            req("author", STRING, ""),
            opt("note", STRING, ""),
            opt("entities", ARRAY, "[entity]; optional"),
            opt("paths", ARRAY, "[\"Component\" | \"Component.field\"]; optional"),
            opt("assets", ARRAY, "[asset name]: its sidecar, whole (edit.asset); optional"),
            opt("defs", ARRAY, "[prefix:key]: a def row, whole (edit.def, a tuning session's commit); optional"),
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
        name: "input.bind",
        doc: "Rebind an action on a device from now on (section 12: rebinding at runtime is a UI over the same table). The log records actions, not keys, so a replay survives it. save also writes it into input.toml. A game's controls menu does the same with InputBinding buttons. Emits input.rebound.",
        params: &[
            req("action", STRING, ""),
            opt("device", STRING, "keyboard (default), mouse, or gamepad"),
            req("bindings", ARRAY, "names: [\"space\", \"z\"]; empty unbinds the device"),
            opt("save", BOOLEAN, "also write it into input.toml (default false)"),
        ],
        returns: "{action, device, bindings, saved}",
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
        name: "input.device",
        doc: "Device events from a client's viewport (phase 4 item 1), down the window's own path: F8 and its note prompt, F9 and F10 for a proposal, then the action map, so gameplay still sees actions and the input log records them as a window's. Keys by the window's names (KeyW, ArrowLeft, Space, F8, Enter), mouse buttons (left, right, middle, back, forward), the pointer in the pixels of this connection's render.stream, and release_all when the viewport loses focus. text is what was typed, for a mark's note. Not in the change feed key by key: the input log has them; what F8, F9, and F10 do is (mark.drop, proposal.show, ...).",
        params: &[
            opt("events", ARRAY, "[{key, pressed} | {mouse, pressed} | {pointer: [x, y]} | {release_all: true}]"),
            opt("text", ARRAY, "strings typed, in order"),
        ],
        returns: "{events, turns}",
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
        doc: "Continuous clock speed: sim ticks per render frame. 0 pauses; 3 runs three ticks per frame. A game reads its speed (q:speed(), for a HUD), so a change is a change between ticks: a keyframe comes before the next tick and a replay never crosses it. A game asks for a speed with q:speed(x), which takes effect at the end of the tick that asked.",
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
            opt("switch", BOOLEAN, "make it the world in use; a new name is made first"),
            opt("drop", BOOLEAN, ""),
            opt("turn", INTEGER, "on a discrete clock, the turn to fork at instead of frame"),
        ],
        returns: "{active, worlds: [{name, tick, frame, entities}]}, plus made: {name, frame, from} when one was made",
    },
    MethodDoc {
        name: "sim.ring",
        doc: "The run a scrubber draws, with no keyframe taken (sim.snapshot takes one): the frames a seek reaches (floor to end), the keyframes' ticks and the barriers' (the changes made between ticks: edits, room changes, reloads), where the stretch with every keyframe begins (a seek from there replays at most `every` ticks, one before it up to a minute), and which run it is: run changes when a scene loads, a replay opens, or another world is switched to.",
        params: &[],
        returns: "sim.snapshot's report plus {run, dense_from, capture_ms: {last, max} of the keyframes taken on ticks, ticks, barrier_ticks (at most 4096 of each, evenly spaced), tick, time, driver, hz, fork: the world in use, worlds: sim.fork's list}",
    },
    MethodDoc {
        name: "sim.snapshot",
        doc: "Take a keyframe now, kept like a change's, and report the ring: keyframes every `every` ticks, all of them for the last five minutes of sim time, then one a minute to an hour, then one per ten minutes, under a byte budget ([ring] every and budget_mb in argus.toml).",
        params: &[],
        returns: "{keyframes, bytes, budget, every, oldest, newest, barriers, end, floor, tick}",
    },
    MethodDoc {
        name: "sim.save",
        doc: "Save the game now (section 6): the world, the clocks and random streams, the particles, and the rooms loaded, with each component's schema version, so a later build loads it through the migrations the components declare. Kept in .argus/saves/<name>.save, or at path. A script asks with q:save(name), saved at the end of the tick that asked. Emits game.saved.",
        params: &[
            opt("name", STRING, "default \"save\""),
            opt("path", STRING, "a file instead, absolute or project-relative (a test's fixture)"),
        ],
        returns: "{name, path, tick, bytes, ms}",
    },
    MethodDoc {
        name: "sim.load",
        doc: "Load a save (section 6): its scene loaded fresh with its seed, then its world, clocks, random streams, particles, and rooms put back. Components saved at an older version come up through their declared migrations (a component's migrate: a table of rename, default, and drop steps by version, or a function), else field by field by name; each is reported as a component.migrated event. A save from a newer version of the game is refused. The change feed, undo, and locks stay. A script asks with q:load(name), done after the step that asked. Emits game.loaded.",
        params: &[
            opt("name", STRING, "default \"save\""),
            opt("path", STRING, "a file instead, absolute or project-relative"),
        ],
        returns: "{name, scene, tick, saved: its identity, entities, migrated: [{component, from, to, entities, declared, left}], remapped, dropped, missing_effects, ms}",
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
        doc: "Set a mark's note, the human's words (\"dash feels late\"), as the window's prompt after F8 does. It replaces the note; an agent says what it did with mark.close's note instead.",
        params: &[
            req("id", INTEGER, ""),
            req("note", STRING, ""),
        ],
        returns: "mark.drop's summary",
    },
    MethodDoc {
        name: "mark.cancel",
        doc: "Take a mark back, as Escape does at the note prompt after F8: its files go, and a mark.cancelled event says so. A prompt open for it closes, and play goes on if the prompt paused it.",
        params: &[req("id", INTEGER, "")],
        returns: "{cancelled}",
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
        doc: "The tune fields (tune = true) with their values, defaults, ranges, and docs: what the editor\'s tuning panel shows (section 10). Every one of an entity; or every one of the entities with an id, of the runtime entities (one by one while a component has 32 or fewer, as Outpost\'s job kinds; else one row with their count, tuned through what they are made from), and of the def tables\' rows (phase 4 item 3: {def: \"item:meal\", path: \"food\"}). label says what each is.",
        params: &[
            opt("entity", STRING, "optional"),
        ],
        returns: "{total, rows: [{entity | def | component, path, value, default, range, doc, label, runtime?, table?}], session: the open tuning session or null}",
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
        doc: "The slider (section 10): set a tune field in the live world now, an entity\'s (entity, path Component.field) or a def row\'s (def \"item:meal\", path the field), with no file written, and log it with its frame. Opens a tuning session if none is open; tune.end commits the session\'s final values, to the entities\' files and the def tables\'. A field that is not a tune field is refused.",
        params: &[
            opt("entity", STRING, "one of entity, def"),
            opt("def", STRING, "one of entity, def: a def row, \"item:meal\""),
            req("path", STRING, "Component.field for an entity; the field for a def row"),
            req("value", ANY, ""),
            opt("author", STRING, "default \"human\", when it opens the session"),
        ],
        returns: "{entity | def, path, from, to, session}",
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
        name: "obs.review",
        doc: "Ask the human for judgement on a moment of this run: a question (\"does the enemy death read as a kill?\") and the clip of its frames, replayed from the keyframe ring and drawn offscreen, go into the editor's agent panel, where the human scrubs the clip and answers with one key (a choice) or a sentence. The queue drains at their pace: review.list reads the answers. Frames default to the last two seconds; at most ten seconds.",
        params: &[
            req("question", STRING, "what to judge by feel"),
            opt("frames", ARRAY, "[from, to]; default the last two seconds (on a discrete clock, the last eight turns)"),
            opt("options", ARRAY, "the choices, one key each (1 to 9); default [\"yes\", \"no\"]; a sentence is always an answer too"),
            opt("note", STRING, "what the agent changed, or why it asks"),
            opt("software", BOOLEAN, "draw with the software rasteriser"),
        ],
        returns: "{id, status: open, question, options, from, to, asked_by, clip, sheet: absolute paths, frames, every, fps, note}",
    },
    MethodDoc {
        name: "review.list",
        doc: "The review requests waiting for the human, oldest first, and with all the answered ones too: each answer, who gave it, and the frame the game was at.",
        params: &[opt("all", BOOLEAN, "default false: open requests only")],
        returns: "{open, answered, rows: [obs.review's summary, with answer, answered_by, answered_at, answered_frame once answered]}",
    },
    MethodDoc {
        name: "review.get",
        doc: "One review request: obs.review's summary, with its scene, seed, and when it was asked.",
        params: &[req("id", INTEGER, "")],
        returns: "review.list's row plus scene, seed, asked_at",
    },
    MethodDoc {
        name: "review.answer",
        doc: "Answer a review request: one of its choices, or a sentence. It leaves the queue, and a review.answered event and notification say so. A request is answered once.",
        params: &[
            req("id", INTEGER, ""),
            req("answer", STRING, "a choice, or a sentence"),
        ],
        returns: "review.list's row, answered",
    },
    MethodDoc {
        name: "replay.list",
        doc: "The replays saved under .argus/replays/, newest first: a failing test's (<test>/seed<n>/, its note the failure) and replay.save's. replay.open opens one.",
        params: &[opt("limit", INTEGER, "default 50")],
        returns: "{total, rows: [{path: relative to the project, for replay.open, dir, frame, scene, seed, note, about: {scenario, file, seed} for a test's, saved_at, picture, clip, sheet, sound}]}",
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
        name: "sim.set",
        doc: "Put one field in the live world now, an entity's (entity, path Component.field) or a def row's (def \"item:meal\", path the field), with no transaction, no file, and no tuning log: a scenario's setup, a sweep's candidate (test.run's set runs it after each load). A client's respects locks and is a row in the change feed. To change the game for good, edit; to tune by feel, tune.set.",
        params: &[
            opt("entity", STRING, "one of entity, def"),
            opt("def", STRING, "one of entity, def: a def row, \"item:meal\""),
            req("path", STRING, "Component.field for an entity; the field for a def row"),
            req("value", ANY, ""),
        ],
        returns: "{entity | def, path, from, to}",
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
        doc: "Receive notifications on this connection: `event` for each new event, `edit.committed` for each commit (and proposal made or turned down), `edit.command` for each other change in the feed, `tune.changed` for each tuning log row, `review.asked` and `review.answered` for review requests. Topics: events, edits (the change feed), tune, reviews.",
        params: &[
            opt("topics", ARRAY, "[\"events\" | \"edits\" | \"tune\" | \"reviews\"]"),
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
        doc: "Run scenarios headless in this daemon: each seed loads the scene fresh, scripts input, runs frames, and checks expectations; state goldens live in tests/goldens/ and a mismatch is a field-level diff. set puts fields in each run\'s world just after its scene loads, live only, as sim.set does (phase 4 item 3). sweep runs the chosen scenarios once for each of its values of one target and reports each candidate (section 10: which ones starve): sweep={entity: @colony, path: Colony.hunger_per_day, values: [0.8, 1, 1.2]}.",
        params: &[
            opt("name", STRING, "optional; exact or substring"),
            opt("file", STRING, "optional, tests/x.luau"),
            opt("seeds", STRING, "optional override: \"7\", \"1..50\", \"1,2,9\""),
            opt("update", BOOLEAN, "rewrite goldens"),
            opt("replays", BOOLEAN, "default true"),
            opt("software", BOOLEAN, "draw clips with the software rasteriser, as CI does"),
            opt("set", ARRAY, "[{entity | def, path, value}]: into each run's world after its scene loads, live only"),
            opt("sweep", OBJECT, "{entity | def, path, values: [...]}: the runs once for each value; then the answer is {ok, sweep, candidates: [{value, ok, runs, failed, failing: [{scenario, seed, error}], ms}], ms}"),
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
        returns: "{window, stream: {path, size, fps, frames, last_ms, error}, size: [w, h], adapter, software, last: {sprites, draw_calls, atlases, interpolated, missing_atlases, materials, missing_materials, material_problems, ms}, atlases: [name], materials: [name]}",
    },
    MethodDoc {
        name: "render.stream",
        doc: "Frames for this connection (phase 4 item 1, section 5's out-of-band channel; the editor's viewport): the daemon draws the game as a window would, into a file both processes map, and sends a render.frame notification for each frame, {path, slot, seq, tick, frame, width, height, view: {center, half, angle} in world units}, naming the slot that holds it (the view turns a point on the frame into a place: y is down on the frame and up in the world). The file is a 64-byte header (ARGUSFR1, version, slots, width, height, slot bytes), then slots of a 64-byte header (seq, tick, frame, width, height) and width x height RGBA8 pixels, rows top first; a slot's seq is odd while it is written, so a copy is whole when its seq was the notification's before and after. Frames come every frame while the game plays, and when something changes while it does not. One stream at a time; the gamepads and the sound open with it. Calling it again resizes; open: false or a disconnect closes it.",
        params: &[
            opt("width", INTEGER, "pixels, default 1280"),
            opt("height", INTEGER, "pixels, default 720"),
            opt("fps", NUMBER, "frames a second at most, default 60"),
            opt("open", BOOLEAN, "false closes it"),
            opt("audio", BOOLEAN, "open the sound output, default true"),
            opt("software", BOOLEAN, "draw with the software rasteriser"),
        ],
        returns: "{stream: {path, width, height, fps, slots, header_bytes, slot_header_bytes, format}, audio, adapter, software}",
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
        doc: "Per-system mean and p95 over the last n ticks, in stage order. series: each tick's whole time too, for drawing against a budget.",
        params: &[
            opt("last", INTEGER, "default 600"),
            opt("series", BOOLEAN, "default false: also ticks, [[tick, ms]] oldest first"),
        ],
        returns: "{last, tick_p95_ms, rows: [[name, stage, mean_ms, p95_ms, max_ms, samples]], ticks?}",
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
        name: "prof.targets",
        doc: "Section 17's performance targets beside what is measured here (section 5: \"a perf regression is a query result with a system name on it\"). Every row in the design's words with the test that gates it; for the open game's rows (and the rows for every game) what this daemon measures now: the tick's p95, a system's time, the last seek, reload, open, save, or load, the last fast-forward's rate, a keyframe's capture. ok is false for a row over its bound; system names the one to look at (for a tick row, the heaviest). build says whether this daemon is a debug build, whose numbers run slower than the release numbers the targets are.",
        params: &[opt("last", INTEGER, "ticks the tick and system rows cover; default 600")],
        returns: "{game, build, last, tick_p95_ms, heaviest: {name, p95_ms}, entities, speed, over: rows over their bound, rows: [{game, measure, target, why, gate, live, value, unit, text, ok, system, context, bound: {below} or {at_least} for a measured row, tick: whether it is the game's tick budget}]}",
    },
    MethodDoc {
        name: "prof.calls",
        doc: "How long clients' calls took, by method, the latest first: the protocol's side of the loop (a seek, a reload, a query). A call's own ms where it reports one, else the daemon's time on it; the harness's calls inside a test run are the test's.",
        params: &[],
        returns: "{columns: [method, count, last_ms, mean_ms, max_ms], rows}",
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
