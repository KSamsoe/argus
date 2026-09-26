# argus-physics: spec trace

Every statement in the design document (v0.3) that constrains the physics tiers as phase 1 item 6 scopes it (§12 "Physics, in tiers", §3 systems and stages, §6 determinism, §8 events, §10 tune fields, §12 input, §15 the sim-stage rule, §18 phase 1 and its gate), with where it lives and the test that pins it. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §12 Physics, in tiers

| Spec | Status | Where | Test |
|---|---|---|---|
| Three tiers, chosen per entity. | Done (two of three) | The components an entity carries choose: a `Collider` alone is the overlap tier; a `Collider` and a `KinematicBody` is the kinematic tier; a `CharacterController` on a body adds the platformer's run and jump. The dynamic tier is the phase-5 module (below). | `tests/physics.rs` |
| Sharing one broadphase (a spatial hash over the live world, rebuilt incrementally each tick). | Partial | `physics.overlap` rebuilds the world's one spatial hash from where things are after the move (`SystemCtx::rebuild_broadphase`) and finds candidate pairs with `SpatialHash::for_each_within`, which allocates nothing; every later system that tick, and `q:within`, see the same hash. It is rebuilt whole, not incrementally, as in item 0: in a tick with colliders it is built twice, after the move and at the end of the tick. | `overlaps_begin_and_end_once_and_respect_layers`, `perf_a_thousand_characters_and_twenty_thousand_contacts` |
| Overlap: shapes that report contacts and nothing else (Swarm's bullets, Delve's triggers, Outpost's zones). | Done | A box (`size`) or a circle (`radius`), centred on the position plus `offset`, with `layer` and `mask` bits (two colliders meet only when each one's mask has a bit of the other's layer). `physics.overlap` emits `overlap.begin {entity, other}` when a pair starts to overlap and `overlap.end` when it stops, once per pair, the lower entity first. Touching is not overlapping, so a character standing on a platform is not in contact with it. Swarm's bullets keep their phase-0 Luau hit pass; moving them onto colliders is the game's choice. | `overlaps_begin_and_end_once_and_respect_layers`, `a_player_on_a_platform_touches_it_without_overlapping_it` |
| Kinematic: a character controller that moves a shape against a tilemap and other kinematic bodies. | Done | `physics.move`: every Tilemap entity's collision layers (read through the session's `MapStore`, the same one tile edits write) and every solid body. An actor moves x then y, each axis swept to the first thing in the way, so no speed passes through a wall. Solids (`solid = true`) go where their velocity takes them, ignoring tiles, and block actors. Actors do not block each other; they meet through overlap events. | `lands_runs_and_stops_at_walls`, `fast_bodies_stop_at_walls_instead_of_passing_through` |
| With slopes. | Done | A tile's `slope = [left, right]` property (heights as fractions of the tile). A body stands on a slope at its centre and follows it up and down; level ground meeting a slope, at the top or the bottom, is walked across without a pop, and the next slope tile is never a wall. `KinematicBody.slope` reports the rise under the centre. 45-degree and 22.5-degree slopes are tested. | `slopes_are_walked_up_and_down_without_leaving_the_ground_or_popping` |
| One-way platforms. | Done | One-way tiles (`one_way = true`) and one-way solid bodies (`solid` and `one_way`) block only a body whose feet were at or above them before the move. A body jumps up through one and lands on it; down plus a jump press drops through (`body.dropped`). `KinematicBody.on_one_way` says what it stands on. | `one_way_platforms_are_jumped_through_landed_on_and_dropped_through`, `a_one_way_platform_body_is_jumped_through_and_ridden` |
| Moving platforms. | Done | Solids move first each tick. A body standing on one is carried with it, sideways and up and down; a full solid pushes the actors in its way, and one it cannot push free (against a wall) is crushed (`body.crushed {entity, by}`). A one-way platform rising under a body picks it up. Jumping off keeps `platform_boost` of the platform's velocity. `KinematicBody.ground` names the platform. | `moving_platforms_carry_riders_and_push_what_they_hit`, `a_one_way_platform_body_is_jumped_through_and_ridden` |
| Step-up. | Done | On the ground, a ledge no higher than `step_height` is walked onto; a taller one is a wall; a step with no headroom above it is a wall. Small drops within the step height are followed down instead of fallen off. | `small_ledges_are_stepped_onto_and_taller_ones_are_walls`, `a_step_with_no_headroom_is_a_wall` |
| The timing affordances platformers live on (coyote time, jump buffering, corner correction) as tunable fields rather than something every game reimplements. | Done | `CharacterController`: `coyote_time` (a jump still works this long after running off a ledge; `body.jumped {coyote: true}`), `jump_buffer` (a press this long before landing jumps on landing; `buffered: true`), `corner_correction` (a bonked head slides around a ceiling corner, and feet that just miss a ledge in the air are lifted onto it; `body.corner_corrected {dx, dy}`). All are `tune` fields, beside `run_speed`, the accelerations, `jump_height` and `jump_time` (which set gravity and take-off speed, so the apex is the declared height), `fall_gravity`, `jump_cut` (variable jump height), `max_fall`, `step_height`, and `platform_boost`. | `coyote_time_allows_a_late_jump_and_not_a_later_one`, `a_jump_pressed_just_before_landing_fires_on_landing`, `corner_correction_slides_a_head_around_a_corner_and_lifts_feet_onto_a_ledge`, `a_jump_reaches_its_declared_height_and_letting_go_cuts_it` |
| Dynamic: real rigid bodies with joints, from an optional Rapier module, not a default. | Deferred | Phase 5 lists the optional rigid-body module. | |
| The kinematic tier is the one worth getting exactly right; it is the tier the agent can least judge by itself. | Partial | What can be pinned is pinned exactly: the apex equals `jump_height`, feet follow slopes to within 0.002 every tick, walls stop a body flush, runs are bit-identical for the same inputs. Nobody has played it yet: how it feels is the human's call, through the tune fields, and the tuning panel is phase 2. | `tests/physics.rs` |

## §3, §15 Systems, stages, and the sim-stage rule

| Spec | Status | Where | Test |
|---|---|---|---|
| The sim stage is the only stage that may mutate gameplay state. | Done | All three systems run in `sim`: `physics.control` first (registered before scripts load), then gameplay, then `physics.move` and `physics.overlap`, which are `SystemDesc::last` so they run after every other sim system that does not order itself after them. A late-stage script reacts to the tick's contacts. | `argus-sim/tests/stages.rs::a_last_system_waits_for_the_rest_of_its_stage_and_events_are_seen_the_same_tick`, `argus-daemon/tests/physics.rs` |
| Each system declares the components it reads and writes. | Done | `world.who_writes KinematicBody` answers `physics.control, physics.move`; the controller declares its read of `Actions`. | `argus-daemon/tests/physics.rs` |
| Hot fields should be declared in Rust. | Done | `Collider`, `KinematicBody`, and `CharacterController` are Rust components with native columns. | |
| Nothing in the sim stage reads a device; inputs arrive as per-slot actions. | Done | The controller reads the entity's `Actions` component (`move_action`, default `move`, an axis or a stick; `jump_action`, default `jump`), or the intent fields a script wrote for an entity without one, never a key. | `argus-daemon/tests/physics.rs` |

## §6, §8, §10, §12 Determinism, events, tuning, input

| Spec | Status | Where | Test |
|---|---|---|---|
| Same-platform f32 determinism. | Done | Bodies move in entity order; contacts are sorted; nothing reads the clock or a hash's iteration order. | `the_same_inputs_give_the_same_run` |
| Collision is an event (§8 L2). | Done | `overlap.begin`, `overlap.end`, `body.landed {entity, on, speed}`, `body.jumped {entity, coyote, buffered}`, `body.dropped`, `body.wall {entity, side}`, `body.ceiling`, `body.corner_corrected {entity, dx, dy}`, `body.crushed {entity, by}`. Each fires on a change, not every tick, so the log stays readable. In a script, `q:events(name)` returns the ones emitted so far this tick. | `argus-daemon/tests/physics.rs`, `argus-luau/tests/systems.rs::a_script_reads_the_events_emitted_earlier_in_its_tick` |
| `tune = true` marks feel parameters for the tuning panel and the recorder (§3, §10). | Done (the flag) | Every feel field of the controller is a tune field; the panel and the tuning log are phase 2. | `argus-daemon/tests/physics.rs` |
| The agent's `input.inject` says `{action: "jump"}`, not a key (§12 input). | Done | The daemon test runs and jumps the player with injected actions. | `argus-daemon/tests/physics.rs` |
| (Phase 2 item 6.) A jump pressed during a hit-stop is not lost. | Done | Input waits through a stop and lands on the first tick after, where a press and its release can arrive together. The controller counts `Actions.jump_pressed` as a press even when the button is already up again, so that tap jumps (a short jump, since it was released). The same holds for any tap shorter than a tick. | `argus-daemon/tests/post.rs` |

## §17 Performance

§17 names no physics row; these are the measured costs, in release on the development laptop.

| Case | Mean per tick | Worst |
|---|---|---|
| 1,000 characters running and jumping over a 256 × 64 map with slopes and one-way platforms | 1.1 ms | 2.0 ms |
| 20,000 overlap circles moving through 200 boxes, about 800 contact events a tick | 3.2 ms | 3.8 ms |
| A scene with no bodies or colliders | nothing allocated | |

Test: `perf_a_thousand_characters_and_twenty_thousand_contacts` (ignored; `cargo test --release -p argus-physics -- --ignored`).

## §18 Build plan

| Spec | Status | Where | Test |
|---|---|---|---|
| Phase 1: a kinematic character controller. | Done | This crate. | |
| Gate: a three-room slice of Hollow with tiles, a kinematic player, and one-way platforms. | Partial | The pieces exist and meet through the protocol (a map carved with `edit.tiles`, a player driven by actions, events a script reads); the slice itself is the gate's work, with the camera of item 7. | `argus-daemon/tests/physics.rs` |

## Phase 3, item 5: overlaps across a seek

| Spec | Status | Where | Test |
|---|---|---|---|
| (§6) A seek replays the run exactly. | Done | `physics.overlap` measures this tick's pairs against last tick's; those pairs are not the world's, so they are `Contacts`, shared with whatever keeps keyframes (`install_with`), and a keyframe takes and puts them back. Found scrubbing Hollow's outro: from a hero standing in the doorway, a seek back said the doorway was left (`overlap.end`) on the first tick it replayed, and the replay was reported as diverged. A loaded save starts with none: what touches is met again. | Hollow's `tests/outro.luau` (scrubs with `diverged` null); `argus-daemon` `timeline::a_timeline_is_listed_played_and_scrubbed` |

## Known limitations, stated so they are decisions

- A kinematic body is a box, unrotated and unscaled; circles are overlap-only. A parented collider is placed by its `GlobalTransform` as last propagated, one tick behind a parent that moved this tick; kinematic bodies should not have parents.
- Slopes are floors read at the body's centre, so a box's corner overhangs a slope's low side, as in most tile platformers. A single tile rises at most 45 degrees; there are no ceiling slopes; a flat part-height block is read at the centre too.
- Speed on a slope is the run speed along x, so a steep slope is climbed a little faster along its surface than level ground is walked.
- Actors pass through each other. An enemy that should block the player is a solid; contact damage is an overlap event.
- Tiles always collide with actors; layer masks filter bodies, not tiles. Solids ignore tiles.
- `KinematicBody.wall` is set when a move is stopped by a wall this tick, so holding toward a wall keeps it set and standing still next to one does not.
- The controller is a run and a jump. Wall slides, wall jumps, double jumps, and dashes are game systems over the same fields.
- Dropping through a one-way platform is down on the move stick plus a jump press.
- A crushed body gets an event; what happens to it is the game's decision.
