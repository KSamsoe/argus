# argus-anim: spec trace

Every statement in the design document (v0.3) that constrains sprite animation as phase 1 item 8 scopes it (§13 "Sprite animation", §8 events, §10 tune fields, §14 assets, §17 the animator row, §18 phase 1 and its gate), with where it lives and the test that pins it. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §13 Sprite animation

| Spec | Status | Where | Test |
|---|---|---|---|
| An Animator component runs a state machine declared in TOML. | Done | The machine is an asset: `assets/<name>.toml` with `type = "animator"` (or `[states.*]`), imported, validated, and watched like any sidecar (`argus_assets::animator`). `Animator.machine` names it; `anim.update` runs it. `Animator.play` enters a state on request; `state`, `time`, `frame`, `loops`, and `ended` say where it is. | `states_follow_field_conditions_and_clips_play_at_their_rates`, `argus-assets` `importer::an_animator_imports_with_its_states_and_its_mistakes_have_lines` |
| States reference clips. | Done | `clip` names a clip of the entity's sprite (`Sprite.atlas`, or `Animator.sprite`), from the sprite's sidecar (item 4): frames, fps or per-frame durations, loop, and direction (forward, reverse, pingpong). A state may override `loop` and `fps`. A clip the sprite lacks is reported, and that animator waits. | `states_follow_field_conditions_and_clips_play_at_their_rates`, `problems_are_reported_once_and_other_animators_run` |
| Transitions have conditions on component fields (Velocity.y < 0 → fall). | Done | `when` is terms joined by `and`, each `[not] operand [op value]`: a field path (`KinematicBody.velocity.y`, `abs(...)`), or the state's own `end`, `time`, `frame`, `loops`. The importer checks the syntax with the line; the system resolves each path once to its column and reads rows without building values. `[[any]]` transitions apply from every state, first. | `states_follow_field_conditions_and_clips_play_at_their_rates`, `argus-daemon/tests/animation.rs` |
| And blend windows. | Done | Read two ways, both built. `window = [first, last]` lets a transition fire only while the clip is on those frames (a cancel window). `blend = seconds` crossfades: the outgoing frame is drawn over the new one and fades out, through `Sprite.blend_frame` and `Sprite.blend`, which the renderer draws as a second instance. Skeletal blending is the phase-5 module's. | `a_one_shot_ends_a_window_gates_a_cancel_and_play_enters_a_state`, `a_blend_crossfades_the_outgoing_frame_out` |
| Every state can carry events by frame: the foot lands and fires sfx/step, the sword is out and enables the hitbox, the eyes flash for the telegraph. | Done | `events = [{ frame = 1, name = "footstep", sound = "sfx/step" }]`. The event fires on every frame the clip passes, even when a tick passes several, and on each loop. With `sound`, it also asks the mixer (`sound.play {sound, entity}`), which answers with `sound.played`. What the hitbox event does is gameplay: a late script reads it with `q:events("hitbox.on")` the tick it fires. | `frame_events_fire_on_every_frame_passed_and_ask_for_sounds`, `argus-daemon/tests/animation.rs` |
| Events are ordinary engine events, so the agent sees "hitbox enabled at frame 4 of slash" in the log and a test can assert it. | Done | `hitbox.on {entity, state: "slash", frame: 3}` in `obs.events`, and `anim.entered {entity, state, from}` on every state change. | `a_one_shot_ends_a_window_gates_a_cancel_and_play_enters_a_state`, `argus-daemon/tests/animation.rs` |
| Clip rates, anticipation frame counts, and hold frames are tune fields; a human adjusting a wind-up while playing is the canonical tuning session. | Done | A state's `fps`, `anticipation`, and `hold` are each a number or a field path read every tick. `windup = N` marks the wind-up art; `anticipation` is how many frames at the clip's rate it lasts; `hold` adds frames to the hold frame (`hold_frame`, default the last). Pointing them at a game's component declared with `tune = true` (`anticipation = "HeroFeel.slash_windup"`) makes them tune fields, per entity, logged and committed like any other. `Animator.speed` is a tune field for every clip. | `anticipation_and_hold_are_tunable_from_fields` |
| Aseprite tags import directly as clips. | Partial | The sidecar's clip shape (frames, per-frame durations, direction, loop) is the Aseprite tag's (item 4); reading `.aseprite` files is the phase-5 importer's, which will fill the same clips. | `argus-assets` `importer::the_sidecar_carries_what_aseprite_and_tiled_carry` |
| Skeletal 2D via a Spine-compatible runtime is a phase-5 module; the state machine and events are the same either way. | Deferred | The machine names clips and reads fields; a skeletal module would supply clips and blend them. | |

## §17, §8, §14, §6

| Spec | Status | Where | Test |
|---|---|---|---|
| Hollow: 2,000 animators evaluated per tick under 0.3 ms. State machines are data; transitions are field comparisons, not script. | Done | Machines compile once per import to column slots and field offsets; each archetype's columns are fetched once per tick, the machine and sprite binding looked up once per run of rows, and conditions read numbers straight from the columns. Measured in release on the development laptop: 0.175 ms mean per tick for 2,000 animators; ticks where a third of them change state at once reach 0.43 ms (p95), from building the change events. | `perf_two_thousand_animators` (ignored; `cargo test --release -p argus-anim -- --ignored`) |
| Audio is observed as events (§8). | Done | A footstep is `footstep`, then `sound.play`, then the mixer's `sound.played`. | `argus-daemon/tests/animation.rs` |
| Assets are sidecars an agent can write; every import is an event with warnings (§14). | Done | An animator imports like a sprite: mistakes are failures with `file:line` (an unknown state, `or`, `=`, a bad path, a missing clip name), unknown keys are warnings, `asset.list kind=animator` lists them. A file that stops importing keeps its last good machine playing. | `argus-assets` `importer::an_animator_imports_with_its_states_and_its_mistakes_have_lines`, `argus-daemon/tests/animation.rs` |
| Deterministic time (§6). | Done | Clips advance by sim time, in entity order; the same inputs give the same frames and events. | `tests/anim.rs` |

## §18 Build plan

| Spec | Status | Where | Test |
|---|---|---|---|
| Phase 1: sprite animation state machines with frame events. | Done | This crate. | |
| Gate: the player has a run, jump, and land animation with a footstep event that fires a sound. | Done (the mechanism) | A physics player whose machine goes idle, fall, land, idle, run, jump, fall, land from its body's fields, with footsteps that fire `sfx/step` and are answered by the mixer. The three-room slice is the gate's work. | `argus-daemon/tests/animation.rs::the_player_runs_jumps_and_lands_and_its_footsteps_fire_a_sound` |

## Known limitations, stated so they are decisions

- `anim.update` declares its writes (`Animator`, `Sprite`) but not its reads: conditions read whatever components the project's animator files name, which are not known when the system is installed. `world.describe` lists it as a writer of `Sprite`, not as a reader of `KinematicBody`.
- Conditions cannot read `Animator` or `Sprite` fields (the state's own clock is `time`, `frame`, `loops`, and `end`), and a condition on a component the entity lacks reads 0.
- One transition per tick, and none on the tick a state is entered; the entering tick shows the state's first frame for its full duration.
- A crossfade draws the outgoing frame over the new one; both come from the same sprite. There is no blending of positions or of different atlases.
- In a discrete scene, animators advance with the turn clock. Animations that play between turns need the scheduler to run continuous-bound systems on a discrete scene's frames, which it does not yet do.
- Changing state often in many animators at once costs event strings: each change is an `anim.entered` event.
