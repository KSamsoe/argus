# Phase 4 checklist

Section 18 of the design document gives phase 4 as one sentence and a gate. This file breaks the sentence into items, attaches every constraint from the rest of the document that each item has to meet, and records status, as `phase3-checklist.md` did for phase 3. The order and four decisions were agreed with Kevin on 2026-09-29.

Phase 4 sentence: *egui editor: hierarchy, inspector with override provenance, viewport, tilemap painting, animator and timeline editors, replay scrubber, tuning panel, profiler view, agent panel, "show as command". Built exclusively on the protocol.*

Gate: *Everything the editor does appears in the change feed as a command the agent could have issued. A human tunes Outpost's food economy at 10× with sliders; the agent sees the slider path as data, sweeps the candidates, and reports which ones starve.*

## What the whole editor must meet

- §11: the editor is a separate process that speaks only the protocol. "If the editor ever needs a private channel to the daemon, the protocol is missing something the agent also needs." So every gap the editor finds becomes a protocol method, with its doc, its CLI form, and its MCP reach (§16: one schema for all four).
- P1: "There is no button that isn't also a command. The editor cannot do anything the CLI can't, because it has no other way to reach the world." A test checks that the editor's crate depends on the protocol client and on no engine crate that holds a world.
- §11: the first editor is immediate-mode (egui), because its panels map onto protocol calls: the hierarchy is `world.query`, the inspector is `world.describe` plus `edit.set`, the viewport subscribes to frames, the timeline scrubber is `sim.seek` over the keyframe ring.
- P2: no client needs the whole scene in memory. The editor pages through results with totals and cursors, like the agent.
- §5: undo and redo are scoped per author, "so the human's Ctrl+Z in the editor never unwinds the agent's work and vice versa", and both see what the other undid.
- §15: a release build has no editor. `argus-game`'s dependency tree stays as it is.
- §19 risk, editor debt: an editor built only on the protocol will lag a native one in polish. The bet is that the collaboration panels matter more than inspector polish, and that "show as command" builds a shared language.
- §11 open, not v1: a browser editor served by the daemon.

## Already built, and extended here

The protocol already has most of what the panels call: `world.query`, `world.search`, `world.get` with `resolved` provenance, `world.describe`, `world.instances`, `world.refs`; `edit.*` transactions with authors, per-author undo, locks, `edit.feed`, `edit.tiles`, `edit.text`; `obs.subscribe` with the `events`, `edits`, and `tune` topics; `obs.frame` as a picture or a semantic frame; `sim.*` with `seek` and `fork` over the keyframe ring; `tune.*` and the tuning log; `proposal.*` and F9 in the game window; `mark.*`; `replay.save` and `replay.open`; `prof.*`; `timeline.seek`; `scene.tiles.*`; `input.bindings`. The CLI's grammar (`argus <ns> <method> key=value`) is what "show as command" prints.

## Carried in, found while planning

- **Review requests were never built.** §10: "`obs.review {clip, question}` queues a short clip and a question to the editor; the human answers with one keystroke or a sentence." No phase listed it and no trace row has it. It goes in item 4, with the agent panel that shows it.
- **Frames never left the daemon.** §5: "the hot paths (frame streaming, snapshot transfer) use out-of-band shared memory or a sidecar binary channel that the JSON message merely points at." The daemon's trace has this deferred since phase 0. The viewport needs it: item 1.
- **Device input has no protocol route.** The game window feeds keys to the daemon inside its own process. A viewport in another process needs a way in: item 1.
- **Data files are not editable through the protocol**, except strings (`edit.text`) and tiles (`edit.tiles`). Def tables (`defs/*.toml`: Outpost's food and job priorities, every game's timelines) and asset sidecars (animator machines) are written by hand today. The tuning panel, the animator editor, and the timeline editor need them as transactions: items 3 and 8.
- **`tune.fields` covers components on named entities only.** §10 names Outpost's tune fields as "job priorities, consumption rates, and growth curves". Job priorities and food values are def-table fields with `tune = true`, and the panel does not reach them yet: item 3.
- **The test runner cannot try a value without writing it.** The gate's sweep runs the 20-seed, 30-day scenario against each candidate. Item 3 adds field overrides to a test run.
- Trace rows waiting for the editor: "show as command" (`argus-cli`, item 0); the timeline scrubber (`argus-timeline`, item 9); a failing test's replay opened in the editor (`argus-test`, item 5); the tuning panel's sliders (phase 2 item 5, item 3); pending proposals shown in the editor (phase 2 item 4, item 4).

## Decided with Kevin (2026-09-29)

- **The order below.** The editor's frame and "show as command" first, then the viewport, the hierarchy and inspector, the tuning panel, the agent panel, the scrubber, the profiler, tile painting, the animator editor, and the timeline editor.
- **The human plays inside the editor.** The game's picture streams into the viewport, and keys and the mouse there drive it through the game's own bindings, F8 and F9 included. The game window stays for playing without the editor.
- **The change feed records every change.** Every call that changes something appears in the feed with its author and its `argus` command: edits, play, pause, speed, seek, slider moves, proposal picks, notes, and answers. Lookups do not. Input goes into the input log as a window's does, not key by key into the feed.
- **Remote access waits.** `argus token`, TLS, and a phone page for review requests wait for the first remote client. The editor connects on the local socket. (§5 access, §19's open question on a phone client.)
- **New crates:** eframe and egui 0.31, the release built on the engine's wgpu 24 and winit 0.30; `egui_kittest` for headless editor tests; `memmap2` for frames shared between processes.

## 0. The editor's frame, and "show as command"

**Status: to do.**

- §11: a separate process, `argus editor`, finding or starting the project's daemon as the CLI does.
- P1, §11: built on the protocol client only, checked by a test on its dependencies.
- §11: "show as command" prints the CLI equivalent of every action the human takes, "so the human learns the agent's language by using the editor and can paste a command into a chat instead of describing a click". One function turns any method and its params into an `argus` line, shared by the editor and the feed, and the line runs.
- The gate, and the decision above: the change feed records every call that changes something, from any client, with its author and its command. Each connection says who it is.
- §5: every error carries a sentence, a hint, and a file:line reference. The editor shows all three.
- Tests drive the editor headless (`egui_kittest`), clicking by label, as a person would.

## 1. Viewport

**Status: to do.**

- §11: "the viewport subscribes to frames."
- §5: frame streaming goes through shared memory the JSON points at.
- The decision above: the human plays in the viewport. Keys and the mouse there reach the game through its bindings and its action log (§12: gameplay never sees keys), so a replay of an editor session is a replay like any other.
- §10: F8 drops a mark and F9 toggles proposals in the viewport as in the game window.
- §6: play, pause, step, and speed on either clock (`sim.*`), 10× included (the gate).
- §8: clicking an entity selects it, through the semantic frame's screen bounds.

## 2. Hierarchy and inspector with override provenance

**Status: to do.**

- §11: the hierarchy is `world.query`; the inspector is `world.describe` plus `edit.set`.
- §3: `world.get {resolved: true}` gives each field's provenance (own, a prefab path, or default), and "the editor's inspector shows the same provenance as a colour".
- §3: prefabs and overrides: per-field inheritance, removed components, child paths, and instance counts when a prefab changes.
- §5: edits are transactions with the human as author; authored entities write their files on commit, runtime ones are live only; Ctrl+Z undoes only the human's own work.
- §5, §10: locks. The human can lock an entity, a component, or a path ("camera feel is mine"), and an agent's write into it fails with the owner's name and note.
- §3: ranges and docs from the schema; the transform hierarchy as a tree; rooms (§4).
- P2: pages with totals, never the whole scene.

## 3. Tuning panel, and sweeping candidates

**Status: to do.**

- §4, §10: "Any field with `tune = true` appears in the editor's tuning panel as a slider while the game runs."
- §10: "Every movement of every slider is logged with its frame"; the agent subscribes and sees the search path, not only the final value.
- §10: "When the human ends the session, the final values are committed as an ordinary transaction with the human as author."
- §10: in Outpost the tune fields are job priorities, consumption rates, and growth curves: def-table fields as well as components (carried in).
- §10: "the human nudges a value while watching the colony at 10×, the agent sees the path, and then does what a human can't: runs the twenty-seed, thirty-day sweep from §9 against each candidate value and reports which ones starve." A test run takes field overrides, so a candidate is tried without writing it.
- §13: clip rates, anticipation, and hold frames, the post stack's passes, and emitters' knobs are tune fields too, so the panel is not Outpost's alone.
- §17: Outpost's fast-forward is over 2,000 ticks a second, "a 30-day balance scenario in under a minute per seed".

## 4. Agent panel

**Status: to do.**

- §11: the agent panel shows "pending proposals, review requests, marks, the change feed".
- §10 proposals: the human plays with F9 toggling between the current world and a fork with the proposal applied, "at the same replay point, with the same inputs"; variants cycle on one key; "the pick is the commit".
- §10 review requests (carried in): `obs.review {clip, question}` queues a clip and a question; the human answers with one keystroke or a sentence; the queue "drains at the human's pace"; the clip is rendered from a deterministic replay, so the human can scrub its exact frame range.
- §10 marks: each is a reproducible report, with its note.
- §10 the change feed: author-tagged, so the human returning after an hour reads what the agent did as a sequence of notes.

## 5. Replay scrubber

**Status: to do.**

- §11: "the timeline scrubber is `sim.seek` over the keyframe ring."
- §17: seek to any point in the last five minutes under 500 ms: "marks and A/B toggling must feel instant to the human".
- §9: a failing test's replay file "the human can open in the editor".
- §6: `sim.fork` for trying something from a point without losing the run.

## 6. Profiler view

**Status: to do.**

- §5: `prof.systems` (per-system mean and p95 in stage order), `prof.worst` (the slowest ticks with a per-system breakdown and the archetype counts), `prof.alloc`, `prof.archetypes`, and `prof.capture` (a Tracy trace for the human).
- §5: "a perf regression is a query result with a system name on it." The view shows the §17 targets beside what is measured.

## 7. Tilemap painting

**Status: to do.**

- §12: tiles are "editable through the protocol at the rect level, so an agent can carve a room without touching pixels and a human can paint in the editor and both land in the same chunk". Hollow and Outpost.
- §12: layers, auto-tiling rules, collision from tile properties.
- §17: a 4,096 × 4,096 map of 8 layers edits "without pauses".

## 8. Data edits and the animator editor

**Status: to do.**

- Carried in: def tables and asset sidecars edited through transactions, with layout and comments kept, undoable per author, in the feed, reloaded at commit (as `edit.text` does for strings).
- §13: an animator is a state machine in TOML: states reference clips, transitions have conditions on fields and blend windows, and states carry events by frame ("hitbox enabled at frame 4 of slash").
- §13: clip rates, anticipation frame counts, and hold frames are tune fields: "a human adjusting a wind-up while playing is the canonical tuning session".
- The editor shows a selected entity's animator live: its state, its frame, the events it fires.

## 9. Timeline editor

**Status: to do.**

- §13: a timeline is "a def table of tracks (tweens, animator states, camera cuts, dialogue lines, events) against time, playable from Luau, scrubbable in the editor".
- The scrub is `timeline.seek` (phase 3 item 5); edits are item 8's data edits.
- Customers: cutscenes, boss intros, Outpost's event vignettes.

## The gate

**Status: to do.**

- Everything the editor does appears in the change feed as a command the agent could have issued. Checked by driving every panel headless, then running the feed's commands through the CLI on a fresh daemon and getting the same world.
- Kevin tunes Outpost's food economy at 10× with the sliders. The agent reads the slider path from the tuning log, sweeps the candidates (20 seeds, 30 days each), and reports which ones starve.
- Every earlier gate's tests still pass.
