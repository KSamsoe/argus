# argus-editor: spec trace

Every statement in the design document (v0.3) that constrains the editor (§11, P1, §5's undo and access, §10's panels, §18 phase 4 and its gate), with where it lives and the test that pins it. Phase 4, built item by item as `docs/phase4-checklist.md` orders it. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §11 A separate process on the protocol (item 0)

| Spec | Status | Where | Test |
|---|---|---|---|
| "The editor is a separate process that speaks only the protocol. This is a constraint, not a convenience: if the editor ever needs a private channel to the daemon, the protocol is missing something the agent also needs." | Done | `argus-editor` is its own executable. `argus editor` makes sure the project's daemon runs and starts it; run directly, it finds the daemon from `.argus/daemon.json` or starts one with the `argus` beside it, a process, not a library. Its dependencies are `argus-client`, eframe (egui 0.31 on the engine's wgpu 24 and winit 0.30), and `serde_json`. | `protocol_only::the_editor_depends_on_the_protocol_client_and_no_engine_crate` |
| (P1) "There is no button that isn't also a command. The editor cannot do anything the CLI can't, because it has no other way to reach the world." | Done | Every action is `Editor::act(method, params)`: a protocol call, recorded as its `argus` line. | `editor::the_editor_does_everything_as_commands_the_agent_could_have_issued` |
| "The first editor is immediate-mode (egui) for build speed and because its panels map naturally onto protocol calls." | Done (item 0's panels) | The bar is `session.status` (polled five times a second) with Play or Pause, Stop, Step, and 1x, 3x, 10x (`sim.*`); the overview is `world.summary`; the change feed is `edit.feed` and the `edits` subscription. The hierarchy, inspector, viewport, and scrubber are items 1, 2, and 5. | `editor` |
| "Show as command" prints the CLI equivalent of every action the human takes. | Done | The Show as command toggle opens a panel of every action as its line, with Copy and Copy all; the lines are `argus_client::command::line`'s, the same as the feed's. | `editor` (the panel's lines are the feed's rows by the person) |
| (§5) Errors carry a sentence, a hint, and a file:line reference. | Done | A failed call shows its message, its hint, its reference, and the line that was tried, until dismissed or the same action succeeds. | `editor` |
| The editor says who the person is. | Done | `session.identify {author: "human"}` on connect (`--author`, or `ARGUS_AUTHOR`); the feed colours the person's rows apart from others'. | `editor` |
| A daemon that goes away is said so, and can be reconnected. | Done | The bar says disconnected and why, with Reconnect. | `editor::a_daemon_that_goes_away_says_so_and_can_be_reconnected` |

## The phase 4 gate, first half

| Spec | Status | Where | Test |
|---|---|---|---|
| "Everything the editor does appears in the change feed as a command the agent could have issued." | Partial (item 0's actions) | The daemon writes a feed row for every call that changes something, from any client, with its author and its `argus` line (`argus-daemon` `feed.rs`); a commit's line is the whole transaction as the agent's `edit` tool. Each item's panel adds its actions to the editor test; the gate drives every panel and replays the feed through the CLI. | `editor`; `argus-cli` `feed::every_change_is_a_command_the_agent_could_have_issued` |

## Known limitations, stated so they are decisions

- The editor polls `session.status` five times a second and `world.summary` once a second rather than subscribing to them; the protocol has no status topic, and at these rates a poll is a few hundred bytes.
- The bar's speeds are 1x, 3x, and 10x, the game's own keys in Outpost; any other speed is `argus sim speed x=...`.
