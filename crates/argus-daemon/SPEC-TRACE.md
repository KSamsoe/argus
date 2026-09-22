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
| `scene`: `list`, `load`, `rooms`, `tiles.get`. | Done | Plus `issues` and `transition`. `scene.save` is not needed: authored edits write their files on commit. | `protocol::rooms_stream_through_the_protocol` |
| `edit`: `begin`, `set`, `spawn`, `despawn`, `commit`, `abort`, `lock`; `edit.undo`, `edit.redo`. | Done | Plus `add`, `remove`, `unlock`, `locks`, `feed`. | `protocol::edits_are_atomic_authored_edits_write_files_and_the_feed_answers_why` |
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

## §8 Observation

| Spec | Status | Where | Test |
|---|---|---|---|
| L0 summary, L1 query, L2 events. | Done | `world.summary`, `world.query`, `obs.events` / `obs.subscribe`. | `protocol::*` |
| L3 diff, L4 semantic frame, L5 pixels, `obs.trace`, `obs.why`. | Deferred | Phase 1. See the decision below. | |

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
