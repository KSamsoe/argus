# argus-editor: spec trace

Every statement in the design document (v0.3) that constrains the editor (§11, P1, §5's undo and access, §10's panels, §18 phase 4 and its gate), with where it lives and the test that pins it. Phase 4, built item by item as `docs/phase4-checklist.md` orders it. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §11 A separate process on the protocol (item 0)

| Spec | Status | Where | Test |
|---|---|---|---|
| "The editor is a separate process that speaks only the protocol. This is a constraint, not a convenience: if the editor ever needs a private channel to the daemon, the protocol is missing something the agent also needs." | Done | `argus-editor` is its own executable. `argus editor` makes sure the project's daemon runs and starts it; run directly, it finds the daemon from `.argus/daemon.json` or starts one with the `argus` beside it, a process, not a library. Its dependencies are `argus-client`, eframe (egui 0.31 on the engine's wgpu 24 and winit 0.30), and `serde_json`. | `protocol_only::the_editor_depends_on_the_protocol_client_and_no_engine_crate` |
| (P1) "There is no button that isn't also a command. The editor cannot do anything the CLI can't, because it has no other way to reach the world." | Done | Every action is `Editor::act(method, params)`: a protocol call, recorded as its `argus` line. | `editor::the_editor_does_everything_as_commands_the_agent_could_have_issued` |
| "The first editor is immediate-mode (egui) for build speed and because its panels map naturally onto protocol calls." | Done (item 0's panels) | The bar is `session.status` (polled five times a second) with Play or Pause, Stop, Step, and 1x, 3x, 10x (`sim.*`); the overview is `world.summary`; the change feed is `edit.feed` and the `edits` subscription. The viewport is item 1 (below); the hierarchy, inspector, and scrubber are items 2 and 5. | `editor` |
| "Show as command" prints the CLI equivalent of every action the human takes. | Done | The Show as command toggle opens a panel of every action as its line, with Copy and Copy all; the lines are `argus_client::command::line`'s, the same as the feed's. | `editor` (the panel's lines are the feed's rows by the person) |
| (§5) Errors carry a sentence, a hint, and a file:line reference. | Done | A failed call shows its message, its hint, its reference, and the line that was tried, until dismissed or the same action succeeds. | `editor` |
| The editor says who the person is. | Done | `session.identify {author: "human"}` on connect (`--author`, or `ARGUS_AUTHOR`); the feed colours the person's rows apart from others'. | `editor` |
| A daemon that goes away is said so, and can be reconnected. | Done | The bar says disconnected and why, with Reconnect. | `editor::a_daemon_that_goes_away_says_so_and_can_be_reconnected` |

## §11 The viewport (item 1)

| Spec | Status | Where | Test |
|---|---|---|---|
| "The viewport subscribes to frames." | Done | `viewport.rs`: asks `render.stream` for the panel's size in pixels (again 200 ms after a resize settles), maps the file a `render.frame` names, and copies the named slot into a texture drawn fitted into the panel. | `viewport::a_person_plays_outpost_in_the_viewport` |
| (Phase 4 decision) The human plays in the viewport, with the game's bindings. | Done | A click in the frame gives it the keys: key presses (egui's physical keys as winit's names), modifier changes, mouse buttons, the pointer in the frame's pixels, and typed text go as `input.device`; Tab, the arrows, and Escape are the game's while it has them. A click outside gives them back (and releases what was held), as does the window losing focus. | `viewport` (Tab opens Outpost's inspector) |
| (§10) F8 marks and F9 toggles proposals in the viewport as in the game window. | Done | They are keys like any other: the daemon's window path handles them, the prompt shows in the frame's bar, and the note is typed there. | `viewport` (F8, a typed note, Enter) |
| (§8) Clicking an entity selects it, through the semantic frame's screen bounds. | Done | While the game is not playing a click, and while it plays Alt and a click, asks `obs.frame {format: semantic}` at the frame's size and selects the smallest thing under the pointer, UI before the world; the bar above the frame names it. | `viewport` (a colonist) |
| The game's sound plays while it is shown. | Done | `render.stream {audio: true}`, unless `Editor::audio` is off (the tests turn it off). | |
| (Kevin's first play) The viewport keeps up with the game, in the debug build too. | Done | In the window a frame goes from the shared file straight into a GPU texture of the editor's own (`viewport::Gpu`, eframe's wgpu render state); headless, egui's texture takes the frame's bytes as they are. The editor, egui, and wgpu's core are optimized in debug builds. A debug editor frame is 1.6 ms, and 59 of 60 frames a second are shown. | `keep_up::the_viewport_shows_every_frame_the_game_plays`, `frames_go_straight_into_the_windows_gpu` |

## §3, §5, §11 The hierarchy and the inspector (item 2)

| Spec | Status | Where | Test |
|---|---|---|---|
| (§11) "The hierarchy is `world.query`." (P2) No client needs the whole scene; pages with totals. | Done | `hierarchy.rs`: the top level is a page of the entities with no `Parent` (fifty, the total said, "N more" for the next), a parent's children a query of their own when it is opened, which entities have children one `group_by Parent.entity`; a filter by component or room. Named by `Describe.name` where there is one. | `inspector::a_person_finds_inspects_edits_undoes_and_locks` (a branch, sixty-four entities in pages, a filter) |
| (§3) The transform hierarchy as a tree; rooms (§4). | Done | Children under their parent, indented; the room of each row, and the room filter from `scene.rooms`. | `inspector` |
| (§11) "The inspector is `world.describe` plus `edit.set`." | Done | `inspector.rs`: the schemas from `world.describe` give each field its widget (a box, a number held to its range, two numbers, text), its doc and range on hover; the values are `world.get`'s, the world's now, read four times a second while the game plays. | `inspector` |
| (§3) `world.get {resolved: true}` gives each field's provenance, and "the editor's inspector shows the same provenance as a colour". | Done | A field's name, and a component's, in blue (its own file), green (a prefab of its chain), or grey (the default), with a key; hover names the prefab and its file and line. The prefab chain and how many entities use it; the tags and where they come from; what its own file takes away from the prefab (`overrides.removed`). | `inspector` (the bat's health from `enemy/bat`, its place its own, a component taken away) |
| (§5) Edits are transactions with the human as author; authored entities write their files on commit, runtime ones are live only. | Done | A field when its drag is let go or its text entered, a box ticked, a component added or removed: `edit.begin`, the ops, `edit.commit`, aborted with the reason if an op is refused. Show as command has it as the feed's `argus edit` line. A runtime entity is said to change in memory only. | `inspector` (the bat's max written to its file; the door's box ticked from the widget; a runtime entity) |
| (§5) "The human's Ctrl+Z in the editor never unwinds the agent's work and vice versa." | Done | Undo and Redo buttons, and Ctrl+Z, Ctrl+Shift+Z, Ctrl+Y when neither the game nor a text field has the keys: `edit.undo {author: human}`. | `inspector` (the agent's change stands through two undos) |
| (§5, §10) Locks on an entity, a component, or a path, with a note; the agent's write fails naming the owner. | Done | Lock at the entity's name, in each component's header, and on a field by right-clicking its name; shown as the daemon's `Lock::covers` draws them; Unlock for the holder. | `inspector` (the agent refused with "human" in the error, then allowed; a field's lock fences only that field) |
| (§3) Instance counts when a prefab changes. | Partial | The inspector says how many entities use the entity's prefab (`world.instances`). Editing a prefab's own file is a data edit, item 8's. | |

## §4, §10 The tuning panel (item 3)

| Spec | Status | Where | Test |
|---|---|---|---|
| "Any field with tune = true appears in the editor's tuning panel as a slider while the game runs." | Done | `tuning.rs`: `tune.fields` read twice a second while the tab is shown; a slider in its range (a number where none), grouped by what it tunes: named entities, the data tables' rows, the few of a kind made at run time; the many of a kind folded, tuned through what makes them. A filter by label or field. | `tuning::a_person_tunes_the_food_economy_at_10x_and_keeps_it` |
| "Every movement of every slider is logged with its frame"; the agent sees the search path. | Done | Each step of a drag is a `tune.set`; the session's path (`tune.log`'s story) shows above the sliders. Show as command lists the value let go at, once a drag. | `tuning` (steps at different frames, at 10x; one command) |
| "When the human ends the session, the final values are committed as an ordinary transaction with the human as author." | Done | Keep (`tune.end`, with an optional note) and Put back (`commit: false`). The session's area is the same height open or not, so no slider moves under a hand; each slider's id is its row's. | `tuning` (the colony's file and `defs/items.toml` written; the feed's row with the note) |
| (§10) Outpost's tune fields: job priorities, consumption rates, growth curves. | Done | The colony's fields, the def rows (a meal's food, each job's priority and work), and the job kinds made at run time are all rows. | `tuning`; argus-daemon `tune_defs` |

## §10, §11 The agent panel (item 4)

| Spec | Status | Where | Test |
|---|---|---|---|
| (§11) The agent panel shows "pending proposals, review requests, marks, the change feed". | Done | `agent.rs`, the Agent tab (a count of questions and proposals in its name; the bar's "The agent asks: ..." button): `review.list`, `proposal.list`, and `mark.list` read once a second, and at once on a `reviews` notification. | `agent::a_person_answers_the_agent_tries_its_proposal_and_closes_a_mark` |
| (§10) Review requests: "the human answers with one keystroke or a sentence"; the queue "drains at the human's pace". | Done | The oldest question shows, the rest a line each; its choices are buttons and keys 1 to 9 (while the Agent tab shows and neither the game nor a text field has the keys); a sentence and Send, or Enter. `review.answer`, by the person. | `agent` (key 3; a sentence) |
| (§10) "The agent can attach the exact frame range and the human can scrub it." | Done | The clip (an animated PNG the daemon drew from the replay) decoded off the UI thread; Play clip loops it at its rate; the scrubber moves a picture at a time and names its frame. | `agent` (frame 280, frame 340, playing); `agent::tests::an_animated_png_decodes_to_every_frame` |
| (§10) Proposals: F9 toggles, variants cycle, "the pick is the commit". | Done | Try it (`proposal.try`, then B), a button per world (`proposal.show`), Keep (`proposal.pick` by the person), Stop trying, Turn down; the trial's switches counted. | `agent` (A and B switched, B kept, `Health.max` 5, the feed's pick) |
| (§10) Marks, with their notes. | Done | Each open mark with its frame's picture, its note to reword (`mark.note`), Reproduce (`mark.open`), Close, and Take back (`mark.cancel`). | `agent` (the note, closed) |
| (§10) "The human returning after an hour reads what the agent did as a sequence of notes." | Done | What the agent did: every other author's feed rows, newest first, the note or, without one, the command. The Change feed tab filters by author. | `agent` (the agent's rows only, then the person's) |

## §6, §9, §11 The replay scrubber (item 5)

| Spec | Status | Where | Test |
|---|---|---|---|
| (§11) "The timeline scrubber is `sim.seek` over the keyframe ring." | Done | `scrubber.rs`, a strip under the viewport: `sim.ring` read four times a second; the track from its floor to its end, the stretch with every keyframe lighter, the barriers (changes between ticks) in amber, the tick on screen. A drag sends one `sim.seek` at a time, the latest position next; a drag while playing pauses and plays on; a click lands; Frame -1, Frame +1 (`sim.advance` on a discrete clock), To the end, and the arrows over the strip. | `scrubber::a_person_scrubs_the_run_tries_from_a_frame_and_opens_a_replay`; `scrubber::tests::one_seek_in_flight_and_the_latest_next` |
| (§10) Marks surface where they happened. | Done | Markers from `obs.events` (`mark.dropped`, `review.asked`, and the names typed in Mark events), kept while the run goes on the same and dropped when it branches or is another run (`run`). | `scrubber` (the mark at 300 stays after a seek to 150); `scrubber::tests::markers_stay_across_a_seek_and_go_with_a_branch_or_a_new_run` |
| (§17) Seek to any point in the last five minutes under 500 ms: "marks and A/B toggling must feel instant". | Done | Release: `argus-daemon` `perf_seek`. Debug, the build the person runs: 12 to 36 ms for the longest seek (59 ticks replayed) on Swarm, Hollow, and Outpost; a click on the fixture's run to its frame on screen, 14 to 28 ms. Further back a seek replays up to a minute: shaded, and the tooltip says how many ticks. | `scrubber` (under 1 s end to end, asserted) |
| (§6) "`sim.fork` ... how the editor offers live A/B toggling"; trying something from a point without losing the run. | Done | Try from here: `sim.fork {name: try-N, switch: true}`; the World list switches between worlds and drops them. The run's world waits where it was. | `scrubber` (a step in try-1, main unmoved, back to main with its run) |
| (§9) A failing test's replay "the human can open in the editor". | Done | Replays: `replay.list`, each with its scenario, seed, frame, and failure; Open is `replay.open`, paused at its frame with its lead-in to scrub. | `scrubber` (a test's replay opened, its lead-in on the strip) |
| P1: the scrubber's moves are commands. | Done | A drag is one `argus sim seek frame=...` in Show as command and one feed row (the daemon updates the row); the pause and play around a drag, the steps, the forks, and the replays opened are rows and lines too. | `scrubber` |

## §5, §17 The profiler view (item 6)

| Spec | Status | Where | Test |
|---|---|---|---|
| (§5) `prof.systems`, `prof.worst`, `prof.alloc`, `prof.archetypes`, `prof.capture` in the editor. | Done | `profile.rs`, the Profile tab, read while shown: the tick's p95 and heaviest system; the last 600 ticks drawn against the budget; every system's p95, mean, worst, and allocations a tick (slowest first or in stage order); the five slowest ticks with their top systems and Go there (`sim.seek` to the tick); Capture (`prof.capture {ticks}`), its progress, and the file's path to copy; the archetypes. | `profile::a_person_sees_the_tick_over_outposts_target_and_the_system_that_did_it` |
| (§5) "A perf regression is a query result with a system name on it." The view shows the §17 targets beside what is measured. | Done | §17's rows for the game open and for every game, two lines each: the measure and target, then what is measured now (red, with "look at" the system, when over) or the test that gates it; the other games' rows folded. A debug daemon says so. | `profile` (Outpost's tick over, "look at sim.slow"; the budget 1.6 ms) |
| Protocol call times. | Done | Protocol calls: each method's count, mean, last, and worst (`prof.calls`), the slowest on average first. | `profile` |

## §12, §17 Tilemap painting (item 7)

| Spec | Status | Where | Test |
|---|---|---|---|
| (§12) "A human can paint in the editor and both land in the same chunk." | Done | `tiles.rs`: with the Tiles tab open and a map and layer chosen, the viewport's pointer paints (`viewport.rs` paint mode: no keys to the game, no picks). A stroke is one transaction by the person of `edit.tiles` ops, one per run of a row (a rectangle, one op), sent when the pointer lets go and drawn over the game until it lands; Ctrl+Z takes it back. | `tiles::a_person_paints_the_map_the_agent_carved_and_both_land_in_one_chunk` (the agent's carving and the person's stroke in `ground/0_0.bin`) |
| (§12) Layers, auto-tiling rules, collision from tile properties. | Done | The map's layers (their tilesets, whether bodies hit them); the palette cut from the tileset's texture (`asset.get`), each tile's properties on hover; its terrains as brushes (`edit.tiles {terrain}`); the eraser; pick; "What bodies hit" draws `scene.tiles.collision` over the game (solid, one-way, slopes). | `tiles` (terrain auto-tiled: edges differ from the middle; the collision rows; erase; pick) |
| A map made in the editor. | Done | New tilemap: `scene.tiles.create`, then its entity spawned (in a room, for a room scene) at the view's top left. | `tiles` (the file, its entity's file, the person's commit) |
| P1: the painting is commands. | Done | A stroke's line is `argus edit author=human note='painted N cells of ground with tile 16' ops='[{op: tiles, ...}]'`. | `tiles` |

## §13 The animator editor (item 8)

| Spec | Status | Where | Test |
|---|---|---|---|
| The editor shows a selected entity's animator live: its state, its frame, the events it fires. | Done | `animator.rs`, the Animator tab: the selected entity's `Animator` (from the inspector's `world.get`): its state, frame of the clip's frames, seconds, loops, blending; what it fired (`obs.events {entity}`: `anim.entered`, its events by frame) newest first; a strip of the state's clip frames with its events marked and the live frame lit. | `animator::a_person_shapes_the_heros_run_while_it_plays` (idle, then run and a footstep) |
| (§13) States reference clips; transitions have conditions on fields and blend windows; states carry events by frame. | Done | Each state's clip, loop, rate, wind-up, anticipation, hold, events (frame, name, sound), transitions (to, when, blend; a window shown), the transitions from any state, where it starts, new states, each taken out with x. Each change is `edit.asset` ops in one transaction by the person (`asset.sidecar` read again after). | `animator` (rate dragged, an event, a transition, the loop, a state; the comment kept; one change a drag) |
| (§13) Rates, wind-ups, and holds as tune fields: "a human adjusting a wind-up while playing". | Done | A number lands when the drag lets go, and the game plays it at once; a field path shows as a field (the Tuning tab moves it). | `animator`; `argus-daemon` `animation` |
| Undoable per author. | Done | Ctrl+Z, once a change: the file as it was, byte for byte. | `animator` (five undos) |

## The phase 4 gate, first half

| Spec | Status | Where | Test |
|---|---|---|---|
| "Everything the editor does appears in the change feed as a command the agent could have issued." | Partial (items 0 to 8) | The daemon writes a feed row for every call that changes something, from any client, with its author and its `argus` line (`argus-daemon` `feed.rs`); a commit's line is the whole transaction as the agent's `edit` tool. The viewport's keys are the input log's, and what F8, F9, and F10 stand for is a feed row by the person (`argus-daemon` `stream`). Each item's panel adds its actions to the editor test; the gate drives every panel and replays the feed through the CLI. | `editor`; `argus-cli` `feed::every_change_is_a_command_the_agent_could_have_issued` |

## Known limitations, stated so they are decisions

- The editor polls `session.status` five times a second and `world.summary` once a second rather than subscribing to them; the protocol has no status topic, and at these rates a poll is a few hundred bytes.
- The bar's speeds are 1x, 3x, and 10x, the game's own keys in Outpost; any other speed is `argus sim speed x=...`.
