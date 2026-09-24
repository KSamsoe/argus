# Phase 2 checklist

Section 18 of the design document lists phase 2 as one sentence and a gate. This file breaks the sentence into pieces, attaches every constraint from elsewhere in the document that each piece has to meet, and records status, the way `phase1-checklist.md` did for phase 1. The order was agreed with Kevin on 2026-09-23.

Phase 2 sentence: *Snapshot ring with age thinning, input log, sim.seek, sim.fork; marks with clip export; tuning log for tune fields; proposals; locks, undo, and the change feed; replay export in the test runner; post stack with the hit-flash and shake passes; argus mcp; argus docs --for-agent; first argus build for desktop and web so a playtest link exists before the editor does.*

Gate: *A human plays Hollow, drops three marks with notes ("dash feels late"), walks away. The agent reproduces each from replay, fixes two, and files a proposal for the third. The human returns, A/B toggles the proposal by feel, and commits it. No screen sharing, no "what were you doing". A second human opens the web build from a link and drops a mark that reaches the agent the same way.*

Already built in phase 0, and extended here rather than given items of their own: the input log (`input.record`, `input.log.get`, replay; phase 0 item 7), and locks, per-author undo, and the change feed (`edit.lock`, `edit.undo`, `edit.feed`; phase 0 item 4). Items 4 and 5 exercise them.

Decided with Kevin: the web build and the marks it sends are hosted on klab.

## 0. Seek and fork

**Status: done** (2026-09-23) in the daemon (`ring.rs`: keyframes, thinning, the budget; `sim.seek`, `sim.snapshot`, `sim.fork`, barriers, the log and history cut back on a seek, `fork` on any method), `argus-input` (the command journal, snapshot and restore, branching, `fork_copy`), `argus-sim` (`on_tick_end`, quiet ticks, switched-off systems in `SimState`), `argus-scene` (`StreamState`), `argus-audio` (variations seeded by frame, `silence`), `argus-render` (`forget_motion`), and `argus.toml` (`[ring] every`, `budget_mb`). A seek on Hollow rebuilds the hero's position to the last digit and stepping on replays the fall into the pit exactly; a fork from a past frame replays the same inputs into the same world; a fork after a script edit runs the new code beside the old. Measured in release on Swarm's arena: five minutes of play is 304 keyframes and 22.6 MB, and the worst seek in the last five minutes is 45 ms (the §17 row is 500); a keyframe of the 50,000-entity stress scene is 1.6 ms. `run.toml` carries `code`. The Luau heap question is decided the strict way, and a system that keeps state anyway shows as `diverged` on a seek. Traces: rows in the daemon's, `argus-input`'s, `argus-sim`'s, `argus-luau`'s, `argus-ecs`'s, `argus-audio`'s, and `argus-render`'s. Found while building it: the input log alone replays a toggle wrong, so commands are journaled; a live command after a seek back branches the run even when no recorded command waits; and the audio's variation stream had to be seeded by frame for `sound.played` to replay. Constraints:

- §6: a run is a value. Given the same scene, seed, input log, and code version, the world at tick N is bit-identical. Seek, fork, marks, and tests work identically on the continuous and the discrete clock.
- §6: a snapshot is every SoA column plus the RNG states, the event queue, and the Luau heap. Phase 0 built `SimSnapshot` (the world plus clocks, seed, RNG states, and play state); what lives outside the world is not in it yet: the input state, the particle pools, which rooms are loaded, and which systems a fault switched off.
- §6: in development the daemon keeps a ring of keyframe snapshots (every 60 frames by default, configurable) plus the full input log; `sim.seek {frame}` restores the nearest earlier keyframe and replays inputs forward.
- §6: the ring thins with age (every second for the last minute, every minute for the last hour, every ten minutes beyond), so a mark from three hours ago still reproduces, with a longer replay.
- §17: a full snapshot at 100k entities under 5 ms ("keyframes every second must be invisible during play"); seek to any point in the last 5 minutes under 500 ms ("marks and A/B toggling must feel instant").
- §6: `sim.fork` creates a second world from a snapshot inside the same daemon; forks get their own tick counter and step independently. It is how the harness runs "same inputs, two code versions" and how A/B toggling works (§10, item 4).
- §5: the `sim` namespace has `seek`, `snapshot`, and `fork` (`save` and `load` are save games, phase 3).
- §6: the recorder stores a run's identity as `{scene, seed, code, clock, inputs}`; `code` (the git revision, `+dirty` when edited) is carried in from phase 0.
- §19 open question: include the Luau heap, or forbid Luau systems any per-system state. The design leans to the stricter rule, and phase 0 made globals read-only.

Decisions taken while planning it, confirmed in the build:

- **The ring keeps one keyframe a second for the last five minutes, not the last minute.** §6's schedule and §17's seek target disagree: with one keyframe a minute from the second minute on, a seek four minutes back replays up to 3,600 ticks, and §17's own fast-forward gate (2,000 ticks a second) makes that 1.8 s. Beyond five minutes the schedule is §6's. A byte budget thins further when snapshots are large; the run's first keyframe is always kept, so any frame stays reachable.
- **Inputs are journaled as commands.** The phase-0 log records the value each action ended at, which replays a toggle wrong (a recorded release flips it again) and cannot rebuild the input state between keyframes (the releases an inject schedules for itself). The input state keeps a journal of the commands it was given, each stamped with the tick it came before; a keyframe holds the input state, and a seek re-issues the journal from there. The value log stays the recorded, replayable artifact of phase 0.
- **After a seek, the recorded future plays on until someone acts.** Stepping after `sim.seek` replays what happened next; a new command (an inject, a key) branches the run, and the recorded future after that tick is dropped.
- **Out-of-band changes take a keyframe.** An edit commit, an undo, a room transition, or a script reload changes the world between ticks and is not an input, so a replay across it would miss it. A keyframe right after each one means a replay never crosses one.
- **Seek rewinds the simulation, not authored files.** Edits written to files stay; undo is the tool for those.

## 1. Web spike

**Status: done** (2026-09-24). Findings, numbers, and how to run it are in `web-spike.md`. Hollow's player runtime runs in Chrome from a pack, draws with WebGPU (WebGL2 forced runs on ANGLE, same picture), plays its sounds through Web Audio, and takes keys from the canvas. Browser and desktop runs agree bit for bit: the world and event digests of Hollow's run scenario match at every checkpoint, across a release build, a size-tuned build, and wasm under Node. So a web mark is the run's identity plus its input log and two digests (239 bytes), and the desktop replays it from load to the same world; §6's fallback (replaying web marks in the web build run headless) is not needed for Hollow. Built for it: `argus-os` (every file and clock access of the eight runtime crates, an in-memory file system in the browser), `argus-web` (packs, a daemon-less `Player`, the page's bindings, the `mark_replay` example), `Renderer::web`, argus-audio's device over cpal's Web Audio backend, and `tools/web/` (build, serve, and a headless Chrome check that replays its own mark natively). The wasm module is 8.3 MB as built and 4.25 MB tuned for size (1.5 MB gzipped); a tick of Hollow is 0.04 ms in Chrome. Found while building it: the planks test was writing Hollow's `player.toml` through `edit.commit` (the gate commit carried the hero under the plank), so `test.run` now edits the live world only; and the first frame showed the camera before it settled, so the camera also runs at load. Traces: `argus-os`'s and `argus-web`'s, and rows in `argus-render`'s, `argus-audio`'s, `argus-camera`'s, and `argus-test`'s. Constraints:

- §15: the web target is wasm + WebGPU with a WebGL2 fallback, from the start, "how most 2D games get playtested".
- §15: nothing in the player runtime listens on any socket; the protocol is compiled out of release builds as a Cargo feature.
- §6: cross-platform bit-determinism is a non-goal. The spike measures whether Hollow's run from the same inputs matches between the browser and the desktop; if not, web marks are replayed by the web build run headless.
- Questions it answers: Luau (C++) in wasm through mlua, wgpu's WebGPU and WebGL2 backends with the renderer as it is, audio through the browser, the input path from a canvas, and what a mark from the browser can carry and send.

## 2. Marks and clips

**Status: not started.** Constraints:

- §10: while playing, the human presses one key (F8 by default) to drop a mark. A mark captures the frame, a snapshot, the last five seconds of input, a two-second clip rendered from the ring, and an optional one-line note typed afterward ("dash feels late").
- §10: marks land in `.argus/marks/` and surface to the agent as a queue; each is a fully reproducible bug report. The agent runs `sim.seek` to the mark's frame, traces the relevant entities, and proposes a change without asking "what were you doing".
- §16: `marks` is one of the MCP tools.
- The clip is rendered offscreen from replay (seek to two seconds before, step, capture), so marks need no GPU frame ring and work headless.
- The note is typed in the window before the UI tree exists (phase 3): one line drawn with the annotation font from phase 1 item 12.

## 3. Replay export in the test runner

**Status: not started.** Constraints:

- §9: run a failing test and dump a replay file the human can open (in the editor, phase 4; with `sim.seek` until then), and export a short clip of the failing window so the human can see it without reproducing it.
- §9: everything runs identically in CI with the software render adapter.

## 4. Proposals and A/B by feel

**Status: not started.** Constraints:

- §10: the agent can submit a transaction as a proposal rather than a commit. The human plays with F9 toggling between the current world and a fork with the proposal applied, at the same replay point, with the same inputs. Several variants become several forks and one key cycles them. The pick is the commit; nothing requires the human to read a diff.
- §10: the human can lock any entity, component, or path; the agent's write fails with the owner's name and note. Both parties see one author-tagged change feed. (Phase 0 built these; proposals join the feed.)
- §16: `propose` is one of the MCP tools.
- Before the editor (phase 4), pending proposals are listed through the protocol and toggled in the game window.

## 5. Tuning log

**Status: not started.** Constraints:

- §3, §10: a field with `tune = true` is logged by the recorder whenever it changes, with its frame: "human set HitStop.scale 0.0→0.15 at frame 3011, →0.08 at 3090, →0.1 at 3140". The agent subscribes to the stream and sees the search path, not only the final value.
- §10: when the human ends a session, the final values are committed as one transaction with the human as author.
- The slider panel is the editor's (phase 4). Until then tune fields change through `edit.set` and proposals, and the log records those.

## 6. Post stack

**Status: not started.** Constraints:

- §13: screen-space effects live in a post stack, an ordered list of passes (flash, palette swap, chromatic aberration, vignette, CRT, colour-blind simulation) a game enables with parameters, every one a tune field. Phase 2 builds the stack and the hit-flash and shake passes.
- §13: the hit-flash, hit-stop, and shake combination that makes an impact feel heavy is three tunable systems the human adjusts together in one session.
- Carried from phase 1: the camera's shake is in world space; screen-space shake is this pass. Fades for room transitions are post passes too.
- Semantic frames (§8) report active post effects, as they report the camera's shake.

## 7. argus build, desktop and web

**Status: not started.** Constraints:

- §15: a release build is the same core with the protocol server compiled out, the editor and index absent, Luau precompiled to bytecode, and assets packed; no socket, no `.argus/`, no way to attach. The dev surface is a Cargo feature, not a runtime flag.
- §15: `argus build --release --target windows` produces one directory: the executable, one asset pack per language, and a save location per platform convention.
- §15: the recorder stays in release, bounded: the last thirty seconds of actions and a keyframe, so a report key or a crash produces a replay file `sim.seek` can open. A player bug report becomes a mark.
- §15: packs are built from the import cache, content-addressed so a patch ships only changed chunks; localization packs are separate.
- §15: Windows, macOS, and Linux, plus the web (wasm + WebGPU, WebGL2 fallback).
- The gate: a second human opens the web build from a link and drops a mark that reaches the agent the same way. The build is served from klab, and marks from the browser are posted to klab and pulled into the project's `.argus/marks/`.
- Carried from phase 1: the binary links its C and C++ runtimes statically, so a release starts from any shell.

## 8. argus docs --for-agent and argus mcp

**Status: not started.** Constraints:

- §16: `argus mcp` starts a stdio MCP server exposing about a dozen tools: summary, query, describe, edit, run, seek, observe, events, test, reload, marks, propose. Each tool's description comes from the same schema the CLI help uses; the CLI mirrors the tool set.
- §16: `argus docs --for-agent` emits an ARGUS.md from the live daemon: component schemas with doc strings, the system list with stages and write sets, the prefab catalog, and the current scene summaries. Regenerated on demand, so it never drifts.
- §16 decision: one schema generates the protocol, the CLI help, the MCP tool descriptions, and the agent-facing docs. The phase-0 CLI trace notes that `params` is prose today; this item makes it structured.
- §5: the MCP shim runs `world.summary` on connect.

## Gate

**Status: not started.** Hollow is played by Kevin and by a second person through the web build.
