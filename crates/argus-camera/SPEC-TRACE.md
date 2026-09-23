# argus-camera: spec trace

Every statement in the design document (v0.3) that constrains cameras as phase 1 item 7 scopes it (§12 "Camera", §3 stages, §4 room transitions, §8 the semantic frame's camera rect, §10 tune fields, §18 phase 1 and its gate), with where it lives and the test that pins it. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §12 Camera

| Spec | Status | Where | Test |
|---|---|---|---|
| Follow. | Done | `Camera.target` names the entity to follow (`target = "@player"` in an entity file). The camera snaps onto it the first tick, then moves by the rules below, writing its own entity's `Transform`. | `a_following_camera_snaps_on_then_holds_while_the_target_is_in_its_dead_zone` |
| With dead zones. | Done | `dead_zone` is the half size of a box around the aim: the target moves inside it without moving the camera, and past its edge drags the camera so the aim sits on the edge. `offset` places the aim relative to the target. | same |
| Lookahead. | Done | The target's velocity comes from its movement between ticks (any mover, not only physics). `lookahead` is seconds of that velocity to look ahead per axis, capped by `lookahead_max` and settled over `lookahead_smoothing`. | `lookahead_leads_a_moving_target_and_is_capped` |
| Room bounds. | Done | The daemon fills `Zones` with every room's `bounds` from `room.toml` (a flat scene's own `bounds` as the zone `""`). A `bounded` camera keeps its view inside the zone the target is in, keeps the last zone while the target is between zones, centres a view wider than its zone, and reports the zone in `Camera.zone` and as `camera.zone {entity, zone}` when it changes. | `room_bounds_keep_the_view_inside_and_a_new_room_slides_in`, `argus-daemon/tests/camera.rs` |
| Smoothing. | Done | `smoothing` is a time constant per axis: after that many seconds the camera has covered 1 − 1/e of the way, at any tick rate. The clamp to bounds happens before smoothing, so crossing into a new room slides the camera over rather than cutting (the "camera slide" of §4's room transitions). | `smoothing_is_a_time_constant`, `room_bounds_keep_the_view_inside_and_a_new_room_slides_in` |
| Shake as a first-class component with a curve and a decay. | Done | `CameraShake`: `trauma` from 0 to 1, raised by writing it or by a `camera.shake {amount, camera}` event from any system earlier in the tick (to the drawing camera when no camera is named); shake is trauma to the power `curve`; trauma falls by `decay` a second; `max_offset`, `max_angle`, and `frequency` shape the motion, from smooth noise seeded by the entity and sim time, so a replay shakes the same way and no gameplay random stream is used. The offset and roll move the view, never the camera's entity. | `shake_follows_trauma_through_its_curve_decays_and_replays_the_same`, `argus-render/tests/render.rs::the_priority_camera_draws_and_its_shake_moves_and_rolls_the_view`, `argus-daemon/tests/camera.rs` |
| All of them tune fields. | Done | `height`, `offset`, `dead_zone`, `lookahead`, `lookahead_max`, `lookahead_smoothing`, `smoothing`, `pan_speed`, `zoom_speed`, `zoom_min`, `zoom_max`; `decay`, `curve`, `max_offset`, `max_angle`, `frequency`. | `argus-daemon/tests/camera.rs` |
| Cameras are entities. | Done | A camera is any entity with a `Camera` and a position; there can be many. | all |
| Cutting between two is an event. | Done | The drawing camera is the active one with the highest `priority`, the older entity on a tie (`argus_camera::current`, used by the renderer and the audio listener alike). When it changes, `camera.cut {from, to}`; the first choice at load is not a cut. | `the_highest_priority_active_camera_draws_and_a_change_is_a_cut` |
| Hollow's rooms and Outpost's free pan-and-zoom are two configurations of the same component. | Done | The same `Camera`: with a `target` and `bounded`, it follows and keeps to rooms; with `pan_action` (a stick) and `zoom_action` (an axis) read from the camera entity's own `Actions`, a player pans it at `pan_speed` view heights a second and zooms it between `zoom_min` and `zoom_max`, still inside bounds. With neither, it sits where its entity is, which is how a camera on the player entity itself (both example games) behaves: the system never moves an entity it does not steer. | `a_free_camera_pans_and_zooms_with_actions`, `a_camera_on_the_player_itself_is_left_where_its_entity_is` |

## §3 Stages, §8 observation, §4 rooms

| Spec | Status | Where | Test |
|---|---|---|---|
| Present runs at render rate and is read-only (interpolation, camera, UI). | Done | `camera.update` runs last in the late stage (after gameplay and physics, and after any late script that asks for a shake), so camera motion is part of the deterministic tick and replays exactly. The present side only reads: the renderer interpolates the drawing camera between the last two ticks like any entity and applies its shake. | `argus-daemon/tests/camera.rs`, `argus-render/tests/render.rs` |
| The semantic frame reports the camera rect (§8). | Done | Every camera's `view_min` and `view_max` (the rect it shows, shake included, at its `aspect`) are fields written each tick, so `world.get` answers headless; `obs.frame` reports `camera` and `view {center, half, angle, rect}` for the picture it drew; the semantic frame (item 12) gives the drawing camera's id, rect, and roll, and a shaking camera is one of its effects. | `argus-daemon/tests/camera.rs`, `argus-daemon/tests/semantic.rs` |
| Transitions between rooms are an engine event with a hook for the game (fade, camera slide). | Partial | The slide is the smoothing toward the new room's clamp, and `camera.zone` says it happened. Fades are the post stack's (phase 2). | `room_bounds_keep_the_view_inside_and_a_new_room_slides_in` |

## §18 Build plan

| Spec | Status | Where | Test |
|---|---|---|---|
| Phase 1: camera component. | Done | This crate. | |
| Gate: a three-room slice of Hollow with a following camera. | Partial | A camera follows a physics player across two rooms of a room scene through the protocol, keeping to each room's bounds; the slice itself is the gate's work. | `argus-daemon/tests/camera.rs` |

## Known limitations, stated so they are decisions

- Room bounds clamp a following or panned camera's position. A camera on another entity (the player itself) is not moved, so it is not clamped either; give the game a camera entity with a target for that.
- Bounds are rectangles, one per room, found by the target's position. Rooms that overlap take the first listed; there are no camera zones inside a room yet, and no per-camera bounds.
- `aspect` is the game's design aspect, used for the bounds and `view_min`/`view_max`. A window of a different shape draws the same height and shows more or less at the sides, which can show past a room's side walls on a wider window.
- A parented target is read through its `GlobalTransform`, which is propagated after the late stage, so the camera sees it one tick late.
- A cut is instant. Blends between cameras, and camera moves on a curve, are the tweens and timelines of items 9 and phase 3 ("camera nudges are tweens").
- Screen-space shake in the post stack is phase 2; this is the camera's shake in world space.
