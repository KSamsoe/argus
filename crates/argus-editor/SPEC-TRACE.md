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

## The phase 4 gate, first half

| Spec | Status | Where | Test |
|---|---|---|---|
| "Everything the editor does appears in the change feed as a command the agent could have issued." | Partial (items 0 to 2) | The daemon writes a feed row for every call that changes something, from any client, with its author and its `argus` line (`argus-daemon` `feed.rs`); a commit's line is the whole transaction as the agent's `edit` tool. The viewport's keys are the input log's, and what F8, F9, and F10 stand for is a feed row by the person (`argus-daemon` `stream`). Each item's panel adds its actions to the editor test; the gate drives every panel and replays the feed through the CLI. | `editor`; `argus-cli` `feed::every_change_is_a_command_the_agent_could_have_issued` |

## Known limitations, stated so they are decisions

- The editor polls `session.status` five times a second and `world.summary` once a second rather than subscribing to them; the protocol has no status topic, and at these rates a poll is a few hundred bytes.
- The bar's speeds are 1x, 3x, and 10x, the game's own keys in Outpost; any other speed is `argus sim speed x=...`.
