# argus-input: spec trace

Every statement in the design document (v0.3) that constrains action maps and the input log (§12 "Input actions", §6 "Two clocks" and "Snapshots, seek, fork", §5 input namespace, §9 harness input, §15, §12 accessibility), with where it lives and the test that pins it. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §12 Input actions

| Spec | Status | Where | Test |
|---|---|---|---|
| Gameplay never sees keys. | Done | The only thing gameplay can read is the generated `Actions` component; device events do not exist in phase 0, and when they do (phase 1) they will feed the same `InputState`. The fixture's `sim.move` script reads `Actions.move` and `Actions.fire_pressed`. | `input::injected_actions_reach_gameplay_and_never_name_keys` |
| An action map in `input.toml` declares actions (`move`, `jump`, `confirm`) with bindings per device and per player slot, so two gamepads are two players from the start. | Done | `map.rs`: `[actions.<name>]` with `type` (button, axis, axis2), `doc`, `toggle`, and `[actions.<name>.bindings]` per device (`keyboard`, `mouse`, `gamepad`); `[slots.<n>]` with `devices`. Bindings are parsed, validated (known devices), and reported by `input.bindings`; nothing reads them until devices arrive. | `input::the_map_parses_and_becomes_a_component` |
| The input log records actions, not device events: a replay survives a rebind, a test written against a keyboard runs against a gamepad. | Done | `state.rs`: the log is `tick, slot, action, value` change points. Bindings never appear in it. | `input::injected_input_reaches_the_slot_entity_with_edges`, `input::a_recorded_log_replays_to_the_same_run` |
| The agent's `input.inject` says `{action: "jump"}` rather than naming a scancode. | Done | `input.inject {action, slot, value, frames, hold}`; the value defaults to the action's active value (a button down, an axis at 1). | `input::injected_actions_reach_gameplay_and_never_name_keys` |
| Rebinding at runtime is a UI over the same table. | Deferred | The editor (phase 4); the table is the map and `Devices::resolve` reads it on every poll, so a rebind is an edit to `input.toml` and a `scene.load`. | |
| Accessibility: input-hold and toggle alternatives are action-map options. | Done | `toggle = true` on a button: a press flips the held state; the automatic release does nothing. | `input::toggles_holds_and_scripts` |

## §3, §15 Where actions live

| Spec | Status | Where | Test |
|---|---|---|---|
| The `input` stage: "Input actions land here". | Done | `system.rs`: `input.actions` runs in `Stage::Input`, reads `PlayerSlot`, writes `Actions`. It applies everything scheduled for the tick and writes each slot's fields onto every entity that carries that slot. | `input::injected_input_reaches_the_slot_entity_with_edges` |
| Inputs arrive only as per-player-slot actions. | Done | `PlayerSlot { slot }` on an entity is the only way to receive actions; slots are declared in the map (1 to 8). | `input::injected_input_reaches_the_slot_entity_with_edges` (slots are separate) |
| Nothing in `sim` may read a device. | Done | There are no devices; `Actions` is written before `sim` runs. | |

## §6 Determinism and time

| Spec | Status | Where | Test |
|---|---|---|---|
| A continuous run's input log records inputs by tick; a discrete run's log records commands by turn. | Done | The same log shape for both; `InputState::new(map, discrete)` picks the semantics: continuous injects hold for `frames` ticks, discrete injects are one-turn commands. `input.log.get` reports `clock: "tick"` or `"turn"`. | `input::discrete_runs_take_commands_by_turn`, `input::a_recorded_run_replays_bit_for_bit_and_discrete_runs_log_turns` |
| Given the same scene, seed, input log, and code version, the world at tick N is bit-identical. | Done | `input.record` writes the log and a `run.toml` (scene, seed, clock, inputs, start tick) under `.argus/runs/<stamp>-<seed>/`; `input.replay` schedules the log's changes at their ticks. The test records a run, replays it in a fresh daemon, and compares positions, events, and logs. | `input::a_recorded_run_replays_bit_for_bit_and_discrete_runs_log_turns`, `input::a_recorded_log_replays_to_the_same_run` |
| A run identity as the recorder stores it: `{scene, seed, code, clock, inputs}`. | Partial | `run.toml` carries scene, seed, clock, inputs, started, tick. `code` (the git revision) is added when the recorder proper lands (phase 2 marks). | `input::a_recorded_run_replays_bit_for_bit_and_discrete_runs_log_turns` |

## §5 Protocol

| Spec | Status | Where | Test |
|---|---|---|---|
| `input`: `inject`, `script`, `record`, `log.get`, `bindings`. | Done | All five, plus `input.replay` and `input.state`, in the daemon; documented in `docs::METHODS`. | `input::*` (daemon) |

## §9 Test harness

| Spec | Status | Where | Test |
|---|---|---|---|
| `t.input:hold("move", {1, 0}, frames = 60)`, `t.input:tap("fire")`. | Partial | `inject {frames}` and `inject` with the default single frame are exactly these; the Luau wrappers are piece 10. | `input::injected_actions_reach_gameplay_and_never_name_keys` |

## Phase 1 item 2: real devices

| Spec | Status | Where | Test |
|---|---|---|---|
| Bindings per device and per player slot; two gamepads are two players (§12). | Done | `devices.rs`: `Devices` holds keys, mouse buttons, and pads by connection order; `resolve(map, slot)` reads the slot's `devices` (`keyboard`, `mouse`, `gamepad`, `gamepad:N`) and each action's bindings into a value. Keyboard: key names (winit's `KeyW` and a binding's `w` are the same), `neg/pos` pairs for an axis, `wasd`/`arrows`/`ijkl`/four names for an axis2. Gamepad: a fixed set of buttons, axes, and sticks, with a 0.2 dead zone and triggers as buttons past 0.5. Mouse: buttons. Unknown mouse and gamepad names are refused at load; keyboard names are open-ended. | `input::keys_and_pads_resolve_through_the_bindings_per_slot`, `input::unknown_gamepad_and_mouse_names_are_refused_at_load` |
| The input log records actions, not device events: a replay survives a rebind (§6). | Done | `InputState::apply_devices(now, devices)` schedules only values that changed since the last poll, through the same schedule injects use, so `begin_tick` logs and records them as actions. A device-driven log replays with no devices at all. | `input::device_changes_are_logged_as_actions_and_replay_without_devices`; `argus-daemon/tests/input.rs::keys_move_the_player_and_land_in_the_log_as_actions` |
| Injected and real input merge per slot. | Done | Both schedule changes; the last change applied wins. A device only schedules on its own changes, so a held inject is not overwritten by an idle keyboard. | same |
| A discrete run records commands by turn (§6). | Done | On a discrete clock a value becoming active is a one-turn command, exactly like an inject; a release is nothing. The daemon advances one turn per poll that produced commands, while playing, so a human plays Delve by pressing keys. | `input::in_a_discrete_scene_a_press_is_one_command_for_the_next_turn`, `argus-daemon/tests/input.rs::a_key_press_takes_a_turn_on_a_discrete_scene_that_is_playing` |
| Input-hold and toggle alternatives (§12 accessibility). | Done | Unchanged: a toggle action flips on the device press and ignores the release. | `input::toggles_holds_and_scripts` |
| Recording from real devices. | Done | `input.record` captures the log however the values arrived. | the log tests above |

In the daemon: `Daemon::device_input(events)` is the one entry point; `poll_devices` pumps the window (keys, mouse buttons, focus loss as release-all) and polls gamepads through gilrs (opened with the window, pads already connected announced on the first poll), once per frame before the frame's ticks. `input.devices` reports what is held and connected. `argusd --window --play` and `argus daemon start --window --play` open a window already playing.

## Known limitations, stated so they are decisions

- Keyboard and mouse arrive only through the daemon's window; gamepads are polled only while a window is open. Mouse position is not an action yet; a pointer action type comes with the UI (phase 3).
- Several device commands in one poll on a discrete scene are one turn, not several.
- Pad indices are connection order within one daemon's life; a pad that disconnects and reconnects gets its old index back only if gilrs reuses its id.
- A `frames` hold that overlaps a later inject of the same action ends at the earlier release: both releases schedule the rest value, and the log records what actually happened. Use `hold: true` and an explicit release for precise control.
- Edge fields (`_pressed`, `_released`) are true for exactly the tick of the change, computed from the scheduled log, so they are deterministic under replay.
- `Actions` is one component with a field per action; a map with many actions makes a wide row. Fine for a game's handful of actions; a per-action component is the alternative if a map grows past dozens.
- Slots are 1 to 8, matching couch co-op; no runtime slot creation.
