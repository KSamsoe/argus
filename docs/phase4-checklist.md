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

**Status: done** (2026-09-29) in `argus-client` (new), `argus-editor` (new), the daemon (`feed.rs`), and the CLI (`argus editor`). `argus editor` opens the editor on the project's daemon, starting one if none runs. It has a bar (the project and scene, Play or Pause, Stop, Step, 1x, 3x, 10x, the clock), an overview of the world, the change feed, and Show as command, a panel of everything the person did as `argus` lines with Copy. A failed call shows its sentence, hint, and file:line. Every change from every client is a feed row with its author and its command, and a test types a session's feed into the CLI against a fresh copy of the project and gets the same world. Traces: `crates/argus-editor/SPEC-TRACE.md`, `crates/argus-client/SPEC-TRACE.md`, and the rows in the daemon's and the CLI's.

- §11: a separate process, `argus editor`, finding or starting the project's daemon as the CLI does.
- P1, §11: built on the protocol client only, checked by a test on its dependencies.
- §11: "show as command" prints the CLI equivalent of every action the human takes, "so the human learns the agent's language by using the editor and can paste a command into a chat instead of describing a click". One function turns any method and its params into an `argus` line, shared by the editor and the feed, and the line runs.
- The gate, and the decision above: the change feed records every call that changes something, from any client, with its author and its command. Each connection says who it is.
- §5: every error carries a sentence, a hint, and a file:line reference. The editor shows all three.
- Tests drive the editor headless (`egui_kittest`), clicking by label, as a person would.

Decisions taken while building it:

- **A client crate with no engine in it.** `argus-client` holds the socket, the JSON lines, and the CLI's grammar both ways; the editor depends on it and on egui alone, and a test fails if its dependency tree ever holds an engine crate. The CLI and the daemon use the same crate, so the socket name and the grammar exist once.
- **One command line per change.** A commit's row carries the whole transaction as the agent's `edit` tool (`argus edit author=human note=... ops='[...]'`), since `argus edit commit tx=4` does not say what changed; a proposal's is the `propose` tool's. Undo, redo, picks, and tuning keep the call that made them. The transaction's pieces (`edit.begin`, `edit.set`, ...) are not rows.
- **Every method is classified.** `feed.rs` lists each as a change, a transaction piece, or a read, and a test fails when a new method is in none. Pictures, sound clips, and profiles written to `.argus/` are reads: they observe.
- **Who a connection is.** `session.identify {author}`: the editor is `human`, `argus mcp` is `agent:claude`, the CLI is `cli` unless `ARGUS_AUTHOR` says otherwise. A call's own `author` param wins. The daemon's own calls (the launcher, the harness inside `test.run`) make no rows; `test.run` is one.
- **A parameter a method does not have is refused.** Found by the editor's first test: its 10x button sent `sim.speed {value: 10}`, the parameter is `x`, and the daemon had ignored `value` and set the speed to 1, the default. Now every call is checked against the method's declared parameters (`fork` goes with any), and the error lists the right ones. The feed test had passed with the same mistake, since the replay ignored it too; it compares the speed now.
- **The line is for bash.** A value that needs it is single-quoted as a POSIX shell reads it; the tests and the docs run in bash (Git Bash on Windows).

## 1. Viewport

**Status: done** (2026-09-29) in the daemon (`render.stream`, `input.device`, `mark.cancel`), `argus-client` (`frames.rs`), and the editor (`viewport.rs`). The editor's middle is the game: the daemon draws it into a file both processes map and names each frame's slot in a `render.frame` notification. Click it and the keys, the mouse, and typed text go to the game down the window's own path, so the game's bindings work, F8 drops a mark whose note is typed in the frame's bar, and F9 and F10 try and keep a proposal. A click while the game is not playing, or Alt and a click while it is, selects what is under the pointer. The sound and the gamepads open with it. In release a 1280 x 720 frame costs the daemon 1.55 ms at p95 on Hollow and on Outpost. Traces: `crates/argus-editor/SPEC-TRACE.md`, `crates/argus-client/SPEC-TRACE.md`, and the daemon's.

- §11: "the viewport subscribes to frames."
- §5: frame streaming goes through shared memory the JSON points at.
- The decision above: the human plays in the viewport. Keys and the mouse there reach the game through its bindings and its action log (§12: gameplay never sees keys), so a replay of an editor session is a replay like any other.
- §10: F8 drops a mark and F9 toggles proposals in the viewport as in the game window.
- §6: play, pause, step, and speed on either clock (`sim.*`), 10× included (the gate).
- §8: clicking an entity selects it, through the semantic frame's screen bounds.

Decisions taken while building it:

- **A file both processes map.** The stream is a file in the project's `.argus/streams/` with a header and three slots of RGBA8 pixels (a stream opened clears what earlier ones left there: Windows keeps a file a client still maps, and the first version, in the system's temporary directory, left the tests' behind); each slot's sequence number is odd while it is written, so a copy that raced a write is dropped. The daemon reads the GPU's frame straight into the slot. Slots start on 64-byte boundaries: a frame of odd size put the next slot's sequence number off its alignment, found by the editor's first resize.
- **Frames when they say something.** Every frame while the game plays; paused or stopped, when the world, an edit, or the devices changed, and four times a second besides. The editor asks for the panel's size in pixels and asks again 200 ms after a resize settles.
- **The window's path, not a new one.** `input.device` takes the window's events (key names as winit's physical keys, mouse buttons, the pointer in the stream's pixels) into the same code the window's go through: F8's prompt, a trial's F9 and F10, a controls menu waiting for a key, then the action map. The prompt and the trial bar are drawn into the stream's frames as into the window's.
- **What a key stands for is a command.** F8 is `mark.drop`, Enter at the prompt `mark.note`, F9 `proposal.try` and `proposal.show`, F10 `proposal.pick`, each a feed row by whoever pressed it, in the window as in the viewport. Escape at the prompt took a mark back with no command that did the same (P1); `mark.cancel` is that command now.
- **Selecting is a read.** Alt and a click, or a click while the game is not playing, asks `obs.frame` for the semantic frame at the stream's size and takes the smallest thing under the pointer, the UI before the world. It is not in the feed.
- **One viewport at a time.** A second client's `render.stream` is refused with the reason; a disconnect closes the stream and removes its file.

Found in Kevin's first play in it ("the game has poor performance in the editor"), measured on the debug build he runs and fixed:

- **The daemon drew 37 frames a second of 60.** A frame's next due time was counted from when the last one happened to be drawn, so it often fell a hair after the server loop's next turn and that frame was skipped. The schedule is steady now, and a frame may be drawn up to 4 ms early.
- **An editor frame took 18 ms.** egui ran unoptimized in debug builds, and egui's texture path turned each frame's million pixels into colours one by one. The editor, egui, and wgpu's core are optimized in debug builds now (as the UI and Luau crates were), and in the window a frame goes from the shared file straight into a GPU texture of the editor's own. A debug frame of the editor is 1.6 ms, and it shows 59 of the game's 60 frames a second (`tests/keep_up.rs`, which runs in every test run).
- **The music played before Play.** The stream opens the sound, and a sound device pulls the mixer in real time whatever the game's clock does; the game window never showed it, since a window opened from the command line plays at once. The sound is held now while the game is stopped or paused (before Play, at F8's note prompt): the device gets silence and the mixer waits, so the music goes on from where it was.

## 2. Hierarchy and inspector with override provenance

**Status: done** (2026-09-29) in the editor (`hierarchy.rs`, `inspector.rs`), with no new protocol: everything it needed was there. The left panel is the scene's hierarchy: its top level fifty at a time with the total said, a parent opened for its children, filtered by a component or a room. The right panel has two tabs, the inspector and the change feed. The inspector shows the selected entity (from the hierarchy or a click in the viewport) with each field's name in the colour of where its value comes from, its own file, a prefab, or the default, and the prefab, file, and line on hover beside the field's doc and range. Changing a field, adding or removing a component, is one transaction by the person, in the feed as the `argus edit` line; Undo, Redo, Ctrl+Z, and Ctrl+Shift+Z are the person's own; an entity, a component, or a field (right-click its name) can be locked. Trace: `crates/argus-editor/SPEC-TRACE.md`.

- §11: the hierarchy is `world.query`; the inspector is `world.describe` plus `edit.set`.
- §3: `world.get {resolved: true}` gives each field's provenance (own, a prefab path, or default), and "the editor's inspector shows the same provenance as a colour".
- §3: prefabs and overrides: per-field inheritance, removed components, child paths, and instance counts when a prefab changes.
- §5: edits are transactions with the human as author; authored entities write their files on commit, runtime ones are live only; Ctrl+Z undoes only the human's own work.
- §5, §10: locks. The human can lock an entity, a component, or a path ("camera feel is mine"), and an agent's write into it fails with the owner's name and note.
- §3: ranges and docs from the schema; the transform hierarchy as a tree; rooms (§4).
- P2: pages with totals, never the whole scene.

Decisions taken while building it:

- **The tree one level at a time.** The top level is `world.query` of the entities with no `Parent`; opening a parent queries `where: {"Parent.entity": ...}`; which entities have children is one grouped query (`group_by Parent.entity`). Nothing asks for the whole scene. It looks again every two seconds, and at once after any change in the feed.
- **Live values, file provenance.** The values shown are the world's now (`world.get`'s components), read four times a second while the game plays; where each came from is the resolved file view (`resolved`), and what the entity's own file takes away from its prefab is its overrides (`removed`).
- **A change is committed when the person is done with it**: a drag let go, text entered, a box ticked. A drag is not a transaction a pixel. The editor sends `edit.begin`, the ops, and `edit.commit`; if the daemon refuses an op the transaction is aborted and the reason shown. Show as command lists it as the one `argus edit author=human note=... ops='[...]'` line the feed carries.
- **Undo is the person's own**, from the buttons or Ctrl+Z and Ctrl+Shift+Z (Ctrl+Y), when neither the game nor a text field has the keys.
- **Locks as the daemon draws them.** An entity's lock is shown once, at its name; a component's in its header; a field's by underlining its name, with who holds it on hover. A lock is taken with an optional note ("camera feel is mine"), and only its holder is offered Unlock.
- **Prefabs are read, not edited, here.** The inspector names an entity's prefab chain and how many entities use the prefab; editing a prefab's own file is a data edit, item 8's.

## 3. Tuning panel, and sweeping candidates

**Status: done** (2026-09-30) in the editor (`tuning.rs`), the daemon (`tune.fields`, `tune.set`, `tune.end` over def rows; `edit.def`; `sim.set`; `test.run`'s `set` and `sweep`), `argus-scene` (a def row's value set and written back), and the test harness. The right panel's Tuning tab has a slider for every tune field: the named entities' (Outpost's colony), the data tables' rows (a meal's food, each job's priority and work, a recipe's), and the few of a kind made at run time (the job kinds); the many of a kind are folded at the bottom, tuned through what makes them. Every move of a slider is a `tune.set` the daemon logs with its frame, while the game runs; the session's search path shows above the sliders; Keep saves the values as one change by the person (the colony's field to its scene file, a meal's food to `defs/items.toml`), Put back returns them. A test run takes values without writing them (`set`), and a sweep runs the chosen scenarios once for each candidate value and reports which fail: `argus test name=... seeds=1..20 sweep='{entity: @colony, path: Colony.hunger_per_day, values: [0.8, 1, 1.2]}'`. Traces: `crates/argus-editor/SPEC-TRACE.md`, the daemon's, `argus-scene`'s, and `argus-test`'s.

- §4, §10: "Any field with `tune = true` appears in the editor's tuning panel as a slider while the game runs."
- §10: "Every movement of every slider is logged with its frame"; the agent subscribes and sees the search path, not only the final value.
- §10: "When the human ends the session, the final values are committed as an ordinary transaction with the human as author."
- §10: in Outpost the tune fields are job priorities, consumption rates, and growth curves: def-table fields as well as components (carried in).
- §10: "the human nudges a value while watching the colony at 10×, the agent sees the path, and then does what a human can't: runs the twenty-seed, thirty-day sweep from §9 against each candidate value and reports which ones starve." A test run takes field overrides, so a candidate is tried without writing it.
- §13: clip rates, anticipation, and hold frames, the post stack's passes, and emitters' knobs are tune fields too, so the panel is not Outpost's alone.
- §17: Outpost's fast-forward is over 2,000 ticks a second, "a 30-day balance scenario in under a minute per seed".

Decisions taken while building it:

- **Def tables come with this item, not item 8.** A tuning session's end has to write a meal's food to its table, so `edit.def` (one field of one row, in a transaction, the file's layout and comments kept, undoable) is here; item 8 keeps asset sidecars (animators). A def row's value is changed in memory and given to the scripts again (`q:def` reads it at once), and to the conversations and timelines built from the tables.
- **What the panel lists.** `tune.fields` has the named entities' tune fields, every def row's (`{def: "item:meal", path: "food"}`), and the runtime entities': one by one while a component has 32 or fewer (a label from the component's first word, `JobKind cook`), else one row with their count.
- **A candidate is tried, not written.** `sim.set` puts one value in the live world with no transaction, no file, and no tuning log; a client's respects locks and is a row in the feed. `test.run {set}` runs it after each scenario's scene loads; `test.run {sweep: {target, values}}` runs the scenarios once per value and answers with each candidate's runs, failures, and first errors. The agent reads the candidates off the tuning log's path.
- **One command per drag.** Every step of a drag is a `tune.set` (the log keeps the path); Show as command lists the value let go at.
- **The panel does not move under a hand.** A session opening at a drag's first step put its line above the sliders, pushed them down, and the slider dropped the drag (found by this item's test, which drags the way a person does); the session's area is the same height open or not, and each slider's id is its row's.

## 4. Agent panel

**Status: done** (2026-09-30) in the editor (`agent.rs`), the daemon (`reviews.rs`: `obs.review`, `review.list`, `review.get`, `review.answer`, the `reviews` topic), and the MCP server (the `review` tool). The right panel's Agent tab, with a count of what waits for the person in its name and a button in the bar ("The agent asks: 1 question, 1 proposal"): the agent's questions, each with its clip to play or scrub frame by frame and its choices on keys 1 to 9, or a sentence; its proposals, to try (A and each variant at the same moment), switch, keep, or turn down; the person's open marks with their pictures and notes, to reword, reproduce, close, or take back; and what the agent did, newest first, as notes. The Change feed tab filters by author. Traces: `crates/argus-editor/SPEC-TRACE.md`, the daemon's, and `argus-cli`'s.

- §11: the agent panel shows "pending proposals, review requests, marks, the change feed".
- §10 proposals: the human plays with F9 toggling between the current world and a fork with the proposal applied, "at the same replay point, with the same inputs"; variants cycle on one key; "the pick is the commit".
- §10 review requests (carried in): `obs.review {clip, question}` queues a clip and a question; the human answers with one keystroke or a sentence; the queue "drains at the human's pace"; the clip is rendered from a deterministic replay, so the human can scrub its exact frame range.
- §10 marks: each is a reproducible report, with its note.
- §10 the change feed: author-tagged, so the human returning after an hour reads what the agent did as a sequence of notes.

Decisions taken while building it:

- **A review request is a file, like a mark.** `.argus/reviews/<id>/`: `review.json` (the question, the choices, the frames, who asked, the answer once there is one), the clip, and its contact sheet. The queue outlives the daemon, and the agent reads the answers whenever it next looks (`review.list`, or the `review` tool with `list`).
- **The clip is exact frames, replayed.** `frames: [from, to]` (the last two seconds by default, ten at most), drawn from the keyframe ring's replay as a mark's clip is, so the live run is untouched; frames older than the ring are refused. The panel's scrubber names each picture's frame.
- **One answer.** A request is answered once, by a choice or a sentence; the agent asks again for another look. Asking and answering are events, feed rows with their authors, and `reviews` notifications, so the editor shows a question the moment it is asked.
- **The clip is drawn at the person's view.** A mark's frame and a clip were drawn at 960 x 540 when there was no game window; with the editor's viewport streaming they are its size, so the clip frames the scene as the person sees it (found by looking at a Swarm review in the panel).
- **The editor reads the clip from disk.** It runs beside the daemon, and the answer names the file; the decoding runs off the UI thread. A remote editor (remote access waits) would need the daemon to send the bytes.
- **Asking pauses the game briefly.** The clip is drawn in the daemon's loop, as a mark's is: about 0.3 s in the debug build (0.75 s the first time, while the offscreen renderer starts), measured on Swarm through the CLI. The same holds for F8; drawing clips off the loop is for a later item if it is felt.
- **Marks are shown, not only listed.** Their frame, note, and state are there, and the person can reword a note, reproduce the moment, close a mark they no longer care about, or take one back; closing with what was done stays the agent's.

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

- Carried in: asset sidecars edited through transactions, with layout and comments kept, undoable per author, in the feed, reloaded at commit (as `edit.text` does for strings, and `edit.def` for def tables since item 3).
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
