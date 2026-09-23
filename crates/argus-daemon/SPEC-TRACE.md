# argus-daemon: spec trace

Every statement in the design document (v0.3) that constrains the daemon and the phase-0 protocol (§2 "Process model", §5 "Protocol", the L0–L2 layers of §8, §10 "Locks and the change feed", §16 on generated docs), with where it lives and the test that pins it. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §2 Architecture and process model

| Spec | Status | Where | Test |
|---|---|---|---|
| A Rust core process (`argusd`) hosting the ECS world, the fixed-step simulation, and the Luau VM; clients connect over a local socket (Unix domain socket or Windows named pipe) or over stdio when embedded. | Done | `server.rs`: `Transport::Socket` uses `interprocess` (a named pipe on Windows, an abstract or file socket elsewhere); `Transport::Stdio` reads stdin, writes stdout. `src/main.rs` is `argusd`. | `transport::a_client_talks_to_the_daemon_over_the_local_socket`; stdio smoke-tested by hand |
| One daemon per project; the daemon holds exactly one world. | Done | `Daemon { world, sched, session }`; `session.open` replaces the world. `.argus/daemon.json` records the socket name and pid so clients find the one daemon. | `protocol::session_open_gives_the_one_screen_summary` |
| No separate edit mode and play mode: "editing" is the world with sim stages disabled, "playing" with them enabled; entering play takes a snapshot, stopping restores it. | Done | `sim.play` / `sim.stop` over `Scheduler::play` / `stop` (piece 2). | `protocol::sim_and_events_answer_why_the_bat_died` |
| `argusd --headless`. | Done | The flag is accepted; headless is the only mode in phase 0. | `argusd --help` |
| A script fault must not take the daemon down; the world stays inspectable. | Done | Faults are events from piece 2 and 3; the daemon reports them through `obs.events` and `script.errors`. | `protocol::scripts_reload_in_place_and_errors_are_data` |
| Forked worlds for A/B inside the same daemon. | Deferred | Phase 2 (`sim.fork`). | |

## §5 Protocol

| Spec | Status | Where | Test |
|---|---|---|---|
| JSON-RPC 2.0 over a local socket, with server-initiated notifications for subscriptions. | Done | `rpc.rs` frames one JSON object per line; `obs.subscribe` registers a connection for `event` and `edit.committed` notifications, delivered by the server thread from `Daemon::outbox`. | `transport::a_client_talks_to_the_daemon_over_the_local_socket` |
| Hot paths use out-of-band shared memory the JSON points at. | Deferred | Frame streaming and snapshot transfer are phase 1 and 2. | |
| A small fixed method set, mirrored one-to-one by CLI subcommands and MCP tools; help generated from the same schema. | Done | `docs::METHODS` is the single table; `session.capabilities` and `session.docs` serve it and the CLI (piece 9, `crates/argus-cli`) generates every subcommand and help page from it. 65 methods in phase 0. | `protocol::session_open_gives_the_one_screen_summary` |
| `session`: `open`, `capabilities`, `docs`. | Done | Plus `status`, `close`, and `shutdown` (exit the process after replying; what `argus daemon stop` calls). `launch.rs` is the one way a daemon process opens a project and serves, used by `argusd` and `argus daemon run`; on the socket transport it writes `.argus/daemon.json` at start and removes its own record at exit. | same; `argus-cli/tests/cli.rs::the_binary_finds_starts_and_stops_a_project_daemon` |
| `world`: `summary`, `query`, `get`, `describe`, `who_writes`, `defs`. | Done | `daemon.rs`, `query.rs`. | `protocol::world_query_obeys_the_read_contract`, `protocol::world_get_describe_who_writes_and_defs` |
| `scene`: `list`, `load`, `rooms`, `tiles.get`. | Done | Plus `issues` and `transition`. `scene.save` is not needed: authored edits write their files on commit. Since phase 1 item 5, `tiles.get` also reads tilemaps (`*.tilemap.toml`), and `tiles.create`, `tiles.info`, and `tiles.collision` make a map, describe it without loading chunks, and show its collision as rows of text. The session keeps each map open and reopens it when its index changes on disk. | `protocol::rooms_stream_through_the_protocol`, `argus-daemon/tests/tiles.rs` |
| `edit`: `begin`, `set`, `spawn`, `despawn`, `commit`, `abort`, `lock`; `edit.undo`, `edit.redo`. | Done | Plus `add`, `remove`, `unlock`, `locks`, `feed`, and (phase 1 item 5) `tiles`: rect edits of a tilemap layer by cells, a fill, a terrain, or an erase, applied at commit with an inverse like any other op. `touched` names `file#layer` and `files` the chunks written. | `protocol::edits_are_atomic_authored_edits_write_files_and_the_feed_answers_why`, `argus-daemon/tests/tiles.rs` |
| `sim`: `play`, `pause`, `step`, `advance`, `speed`, `run_until`. | Done | Plus `stop` and `status`. `seek`, `snapshot`, `fork`, `save`, `load` are phase 2 and 3. While playing on the continuous clock, the server thread runs frames at the clock rate; `run_until` fast-forwards headless as fast as the CPU allows. | `protocol::sim_and_events_answer_why_the_bat_died` |
| `script`: `reload`, `systems`, `errors`. | Done | Reload also re-reads def schemas and re-resolves the scene. | `protocol::scripts_reload_in_place_and_errors_are_data` |
| `obs`: `events`, `subscribe` (§8 L2). | Done | The daemon drains world events into a sequence-numbered log (capped at 200,000); `obs.events` filters by name, entity, and frame, and pages with `since`. `frame`, `diff`, `annotate` are phase 1. | `protocol::sim_and_events_answer_why_the_bat_died` |
| `asset`: `list`, `get`, `import` (phase 1, item 4). | Done | The session owns an `argus_assets::Importer`: a scan on open (logged as `assets.scanned` plus each warning and failure), a poll twice a second from the server loop and before `obs.frame`, and `asset.import` on demand. What changed reloads in the window's and the offscreen renderer and, for sounds, in the mixer; each import, warning, failure, and removal is an event. `asset.move` rescans after a rename. | `argus-daemon/tests/assets.rs` |
| `audio`: `status`, `play`, `stop`, `bus`, `clip`, `device` (phase 1, item 3). | Done | `pump_events` hands every `sound.play` and `sound.stop` to the mixer and logs its answer right after the request, in the same frame; then it logs what the music and moving sources did and advances the mixer by sim time when no device is open. The engine is rebuilt with every scene from the run seed. `render.window` opens the audio device and closing the window closes it. A malformed `audio.toml` is `audio_error` on the open and `error` in `audio.status`; the world runs without sound. | `argus-daemon/tests/audio.rs` |
| `test`: `list`, `run` (the §9 runner, item 10). | Done | `test.list` reads `tests/*.luau` without running; `test.run {name, file, seeds, update}` runs scenarios through this daemon's own methods (`Daemon` implements `argus_test::Driver`) and reloads the scene that was open afterwards. Opening a scene now clears the event log's records, since they refer to the old world's component ids; sequence numbers keep counting. | `argus-test/tests/runner.rs` |
| `prof`: `frame`, `worst`, `systems`, `alloc`, `archetypes`, `capture`. | Done | Over piece 2's profiler. `prof.frame` and `prof.worst` rows carry `allocs` and `bytes` per system; `prof.alloc {last}` reports per-system allocation means, maxima, and whether counting is on (`argusd` installs `argus_sim::alloc::Counting`); `prof.capture {ticks, path}` arms the scheduler's capture, and when the last tick is in the daemon writes `.argus/captures/<stamp>-tick<N>.json` and emits a `prof.capture {path, ticks, from_tick, bytes}` event, so a client can wait on `obs.events` or `sim.run_until {event: "prof.capture"}`. Without `ticks` it reports progress. | `protocol::sim_and_events_answer_why_the_bat_died` |
| Read contract: default `limit` 50, `total` always present, a cursor for continuation, compact row arrays, default response about 2 KB. | Done | `query::page` / `query::paged` are shared by every list method; cursors are `c:<offset>` and a foreign cursor is error 4706. `world.summary` is asserted under 2 KB on the fixture. | `protocol::world_query_obeys_the_read_contract` |
| `world.query` shape: `select` field paths, `where` with `has` and `within {of, r}`, `order` including `dist(@player)`, `limit`. | Done | `query.rs`; also `not`, field predicates (`eq`, `ne`, `lt`, `lte`, `gt`, `gte`), and `-path` descending. Spatial predicates go through the broadphase (`WorldSource::near`); the daemon rebuilds the hash before a `within` query when the world changed outside a tick (commit, undo, redo, scene load) or a tick ran since, so a query between ticks sees current positions. | `protocol::world_query_obeys_the_read_contract` |
| `world.summary`: entity counts by archetype, named entities and their key fields, active systems, current frame and seed. | Done | Named entities carry their component lists (the first 50, with `named_total`); systems are split into enabled and disabled. | `protocol::session_open_gives_the_one_screen_summary` |
| `world.get {entity, resolved}`: overrides only, or merged with per-field provenance. | Done | `overrides` from `Scene::overrides`, `resolved` from `Scene::resolved` (piece 5), live values always. | `protocol::world_get_describe_who_writes_and_defs` |
| Edits are transactions with `author` and `note`; authored-entity edits write files on commit; runtime-entity edits are live-only; atomic. | Done | Ops queue in a `Tx` and apply at commit; a failure reverts the ops already applied with their inverses. Files are written through `Scene::set_override` and friends. | `protocol::edits_are_atomic_authored_edits_write_files_and_the_feed_answers_why` |
| Every committed transaction records its inverse; undo and redo are per author. | Done | `Commit::inverse`; `edit.undo` applies it as a new commit tagged `{undo: id}`; stacks are keyed by author. | same |
| The change feed answers "who changed this and why" and emits `edit.committed`. | Done | `edit.feed {since, author}` and the `edit.committed` notification. | same; `transport::*` |
| `edit.lock` fences entities or paths; a write into a locked path fails naming the lock and its owner. | Done | `edit::Lock::covers`; the error (4701) names owner, lock id, and note; the owner may still write. | same |
| Errors carry code, message, hint, `file:line`. | Done | `json::error_json` puts the engine code and message in the JSON-RPC error and hint and ref in `data`. Standard codes for parse errors and unknown methods. | `protocol::session_open_gives_the_one_screen_summary` |
| The local socket needs no auth; remote access via tokens is later. | Done (phase-0 half) | Nothing listens beyond the local socket. | |
| Identical result shapes from the live world or the index. | Deferred | Piece 6 will serve `world.*` and `scene.*` from the index when no world is loaded, with these shapes. | |

## Phase 1, item 12: semantic frames and annotated screenshots

| Spec | Status | Where | Test |
|---|---|---|---|
| L4 `obs.frame {format: "semantic"}` (§8). | Done | The renderer's semantic frame plus the tick, the time, what the tilemaps in view collide with (`tiles.maps[].rows`, one character a cell, top row first, with the first cell and where it falls in the frame's pixels; every second or third cell past 64 by 36), its own `bytes`, and `ms`. It builds no renderer, so it answers on a machine with no adapter; it runs the present stage first, like the picture. | `semantic::the_semantic_frame_says_what_the_player_sees_and_the_picture_is_marked` |
| L5 `obs.frame {format: "png", annotate: true}` (§8). | Done | The picture with the same frame's ids and bounds drawn over it, from the view it was drawn with, and the marks returned. `obs.annotate` (the method table's name) is the same call. `obs.frame` now draws the current tick at alpha 1: a stepped world used to be pictured part way from the tick before, a tick behind what `world.query` said, once the offscreen renderer had drawn a frame. | same |
| Delve lives at L4: the semantic frame carries the map, the log, and the menus (§8). | Partial | Delve's hero, monsters (name and HP), and stairs describe themselves and its turn counter is a HUD line, so a turn reads in under 1 KB. Its map is drawn as tile entities, so the frame gives it as one group with its bounds; a map on the engine's tilemaps would come as collision rows. The log is `obs.events`; menus wait for the UI tree. | `examples/delve` (`argus obs frame format=semantic`) |
| (Found while building it.) | Done | The shader time the renderer was given ran a tick ahead of what the picture showed; it is now the drawn moment. | |

## Phase 1, item 11: materials

| Spec | Status | Where | Test |
|---|---|---|---|
| Compile errors come back through `script.errors` with line numbers (§13). | Done | A material whose import failed (a shader that does not compile, a sidecar that does not read) is a `script.errors` row with `phase: "material"`, the file and line, and the asset; what a renderer's GPU refused is listed too. Fixing the file clears it. | `materials::materials_draw_reload_and_report_their_errors_with_lines` |
| Materials hot-reload like everything else (§13, §7). | Done | Every import reloads changed materials in the window's renderer and the offscreen one (sprites first, so a material naming one binds its new texture). Measured through the protocol in a debug build: edit, `asset.import`, and the GPU compile in 20.5 ms. | same |
| The library and the project's materials are listed (§13). | Done | `asset.list kind=material` gives the project's and the library's (`builtin:<name>`) unless a project asset shadows one; `asset.get` answers a library name with `builtin: true`. Shaders get the sim time, interpolated into the next tick. `obs.frame` and `render.status` report the materials drawn with, missing ones, and parameters that could not be read. | same |
| (Found while testing.) | Done | The daemon tests that took a frame skipped on any `obs.frame` error, not just a missing adapter; they share a `frame` helper now that skips only on `codes::NO_ADAPTER` (the renderer's 5001, exported here for clients built without it). The event pump's headless build warned of an unused value; it no longer does. | `render::the_no_adapter_code_matches_the_renderer` |

## Phase 1, item 10: particles in every scene

| Spec | Status | Where | Test |
|---|---|---|---|
| An Emitter plays an effect asset from a pool outside the ECS, drawn instanced (§13). | Done | Opening a scene installs `argus-particles` after the tweens, so `particles.update` runs last in the late stage, after the camera. Its effects come from the importer and are refreshed after every import. `asset.list kind=emitter` lists the effects (their pool size in the frames column). `obs.frame` and the window draw every pool, and `obs.frame` reports how many particles it drew. With no emitters the system returns at once and allocates nothing. | `particles::a_torch_burns_sparks_fly_and_an_edit_to_the_effect_shows_next_import`, `protocol::sim_and_events_answer_why_the_bat_died` |

## Phase 1, item 9: tweens in every scene

| Spec | Status | Where | Test |
|---|---|---|---|
| A tween is a component, queryable, reproduced exactly by replay (§13). | Done | Opening a scene installs `argus-tween` after the animator, so `tween.update` runs in the late stage before scripts and the camera; what is animating is `world.query {where: {has: ["Tween"]}}`, and `world.who_writes` names `tween.update` for every component a tween has touched. With no tweens it returns at once and allocates nothing. | `tween::a_landing_squashes_the_player_and_nudges_the_camera`, `protocol::sim_and_events_answer_why_the_bat_died` |

## Phase 1, item 8: animators in every scene

| Spec | Status | Where | Test |
|---|---|---|---|
| An Animator runs a TOML state machine with events by frame (§13). | Done | Opening a scene installs `argus-anim` after the renderer registers `Sprite` and before scripts, so `anim.update` runs first in the late stage; its machines and sprite clips come from the importer and are refreshed after every import. `asset.list kind=animator` lists the machines (state count and names in the frames and clips columns). With no animators the system returns at once and allocates nothing. | `animation::the_player_runs_jumps_and_lands_and_its_footsteps_fire_a_sound`, `protocol::sim_and_events_answer_why_the_bat_died` |

## Phase 1, item 7: cameras in every scene

| Spec | Status | Where | Test |
|---|---|---|---|
| Cameras follow within room bounds (§12); the semantic frame reports the camera rect (§8). | Done | Opening a scene installs `argus-camera` whether or not the renderer is built, and fills its zones from every room's `bounds` (or the flat scene's). `obs.frame` returns `camera` (which one drew) and `view {center, half, angle, rect}`. With no cameras, `camera.update` returns at once and allocates nothing. | `camera::a_camera_follows_the_player_through_two_rooms_and_keeps_to_each`, `protocol::sim_and_events_answer_why_the_bat_died` |

## Phase 1, item 6: physics in every scene

| Spec | Status | Where | Test |
|---|---|---|---|
| The kinematic tier runs in the sim stage and its contacts are events (§12, §8 L2). | Done | Opening a scene installs `argus-physics` before scripts load, with the session's `MapStore`, so every scene has `physics.control`, `physics.move`, and `physics.overlap`; with no bodies or colliders they return at once and allocate nothing. Tile edits, `scene.tiles.*`, and drawing go through the store, and a re-import of a tileset rebuilds its collision table. | `physics::a_player_runs_jumps_and_lands_on_carved_tiles`, `protocol::sim_and_events_answer_why_the_bat_died` |

## §8 Observation

| Spec | Status | Where | Test |
|---|---|---|---|
| L0 summary, L1 query, L2 events. | Done | `world.summary`, `world.query`, `obs.events` / `obs.subscribe`. | `protocol::*` |
| L3 diff, L4 semantic frame, L5 pixels, `obs.trace`, `obs.why`. | Partial | L4 and L5 are phase 1's items 1 and 12; L3, `obs.trace`, and `obs.why` are item 13. See the decision below. | |

## §10 Human–agent collaboration

| Spec | Status | Where | Test |
|---|---|---|---|
| The human can lock any entity, component, or path; the agent's write fails with the owner's name and note. | Done | `edit.lock {entities, paths}`; a path is a component or a field. | `protocol::edits_are_atomic_authored_edits_write_files_and_the_feed_answers_why` |
| Both parties see one author-tagged change feed. | Done | `edit.feed`. | same |

## §18 Build plan

Phase 0 item 4, "JSON-RPC daemon with session, world, edit, sim (play, pause, step, advance, speed, run_until), script": this crate.

**Decision on the gate's "events and traces".** The phase-0 gate asks that the agent answer "why did the player die" from events and traces, while `obs.trace` is listed under phase 1. The daemon ships `obs.events` (with entity and name filters) and `world.who_writes`, which together answer the question the gate poses: the event says what happened and when, `who_writes` says which systems could have done it. `obs.trace` (one entity's component history over a window) stays in phase 1 with the snapshot ring it needs. The scenario runner (item 10) will show whether that is enough; if it is not, `obs.trace` moves.

## Known limitations, stated so they are decisions

- One open scene per daemon; `scene.load` rebuilds the world (scripts, prefabs, defs, files) from scratch rather than diffing.
- Undo of a runtime despawn spawns a new handle; the string id (if any) is preserved, the `e:n` handle is not. Authored despawns come back from their former file text.
- Locks are in memory; they do not survive the daemon.
- The event log keeps the last 200,000 events; `obs.events {since}` beyond that is empty, not an error.
- Real-time play runs on the server thread with `recv_timeout` pacing; jitter is a few milliseconds and the sim is unaffected (ticks are counted, not timed).
- The world and the Luau VM are not `Send`, so the daemon lives on one thread and connections forward lines to it. That is the spec's single-process, single-world model; it also means a long `run_until` blocks other clients until it returns.
