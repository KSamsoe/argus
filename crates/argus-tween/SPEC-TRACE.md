# argus-tween: spec trace

Every statement in the design document (v0.3) that constrains tweens as phase 1 item 9 scopes it (§13 "Tweens and sequencing", §3 write sets, §6 replay, §10 tune fields, §18 phase 1), with where it lives and the test that pins it. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §13 Tweens and sequencing

| Spec | Status | Where | Test |
|---|---|---|---|
| A tween is a component: target path, from, to, duration, easing curve, on-complete event. | Done | `Tween { target, path, from, to, from_current, duration, delay, easing, repeats, yoyo, on_complete, keep }` on an entity of its own, so one target can have several tweens at once. `path` is `Component.field` (a number or a vec2) or `Component.vec.x`/`.y`; a number uses the x of `from` and `to`. `from_current` (the default) starts from the field's value when the tween starts. When the last play ends it emits `on_complete {entity: target, tween, path}` and despawns, unless `keep`. A script makes one in a call: `q:tween(e, "Transform.scale", {to = ..., duration = 0.15, easing = "out_back", on_complete = "squash.done"})`. | `a_tween_drives_its_field_to_the_end_emits_its_event_and_goes`, `argus-daemon/tests/tween.rs` |
| Easing curve. | Done | `ease.rs`: linear, step, and in, out, and in-out of quad, cubic, quart, sine, expo, back, elastic, and bounce (Penner's set). Back and elastic overshoot; the value is held inside the target field's declared range, so an overshoot never fails a write. An unknown name is a `tween.error` listing the names. | `every_easing_starts_at_0_ends_at_1_and_has_its_shape`, `from_current_one_axis_integers_and_ranges`, `bad_tweens_fail_once_and_go` |
| Squash-and-stretch, UI slides, and camera nudges are all tweens. | Done | `Transform.scale` (the renderer scales the sprite), `Transform.pos` or one axis of it, `Camera.offset.y` (the camera aims there). Integer fields round. | `argus-daemon/tests/tween.rs::a_landing_squashes_the_player_and_nudges_the_camera` |
| Because they are data the agent can query what is currently animating. | Done | Every running tween is an entity with a `Tween`: `world.query {select: ["Tween.target", "Tween.path", "Tween.time"], where: {has: ["Tween"]}}`. `time`, `value`, `plays`, `started`, and `done` say where each one is. | `argus-daemon/tests/tween.rs` |
| And the replay reproduces them exactly. | Done | Tweens advance by sim time in the tick, in entity order; delays, repeats, and yoyo are fields; the same run twice gives the same values to the bit. | `the_same_run_twice_is_the_same` |
| Above tweens sits a timeline: a def table of tracks, playable from Luau, scrubbable in the editor. | Done | Phase 3 item 5, `argus-timeline`: a timeline's tween cue starts a `Tween` like any other. A play ends within 1e-4 s of its end (sixty steps of 1/60 in f32 add up to just under 1), so a tween of a second ends on its sixtieth tick, before a timeline's cue at its end time. | `argus-timeline` trace; `argus-daemon` `timeline::a_timeline_is_listed_played_and_scrubbed` |

## §3, §10 Write sets and tuning

| Spec | Status | Where | Test |
|---|---|---|---|
| Each system declares the components it reads and writes, so "what could have changed Health?" is one query. | Done | A tween writes whatever its path names, which no declaration made at install time can know. `SystemCtx::declare` (new in `argus-sim`) lets a data-driven system add to its declared access as it meets its targets; `tween.update` declares each target component the first time it touches it, so `world.who_writes Camera` names it once a camera has been nudged. | `a_tween_drives_its_field_to_the_end_emits_its_event_and_goes`, `argus-sim/tests/stages.rs::a_system_declares_what_its_data_touches_and_who_writes_follows`, `argus-daemon/tests/tween.rs` |
| Feel parameters are tune fields. | Done | `duration` and `delay` are tune fields. | `argus-daemon/tests/tween.rs` |

## Known limitations, stated so they are decisions

- Two tweens on the same field both write it; the later entity wins each tick. Cancel one by despawning it.
- A tween on a field another system also writes (a kinematic body's `Transform.pos`) is overwritten or overwrites, by stage order: tweens run in the late stage, after physics.
- `delay` counts down in place, so a kept tween shows 0 once started.
- A tween is declared as a writer of its target component only after it first runs; before that, `who_writes` does not name it.
- Tweens drive numbers and vec2s. Colours are strings today and cannot be tweened.
- Curves are named easings; a curve of control points (as emitters will have) is not offered for tweens.
