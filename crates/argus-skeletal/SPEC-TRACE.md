# argus-skeletal: spec trace

Every statement in the design document (v0.3) about skeletal animation (§13, with §6 and §8 where they touch it, §15 for shipping, and §18's phase 5 sentence), with where it lives and the test that pins it. Statuses: **Done**, **Partial**, **Deferred**.

## §13 Animation

| Spec | Status | Where | Test |
|---|---|---|---|
| "Skeletal 2D via a Spine-compatible runtime is a phase-5 module" (decided with Kevin, 2026-10-04: our own runtime reading Spine's JSON export). | Done | `argus_assets::spine` reads a Spine 4 JSON export (`assets/<name>.json`, its images unpacked into the export's `images` folder, each an ordinary sprite): bones with their inheritance (normal, onlyTranslation, noRotationOrReflection, noScale, noScaleOrReflection), slots with colours and setup attachments, skins (an attachment a skin lacks comes from the default skin), region attachments, events, one- and two-bone IK, and animations: every bone timeline (rotate, translate, scale, shear, and each axis alone), slot attachments and colours (rgba, rgb, alpha), events, draw order, and IK mix and bend, with linear, stepped, and bezier curves. `pose.rs` is the runtime: the setup pose, an animation sampled over it, bones to world matrices with Spine's arithmetic, IK (Spine's one- and two-bone solutions), and each slot's attachment placed. `Skeleton` on an entity poses it every tick (`skeletal.pose`); each slot is drawn by a part entity under it (`Transform`, `Parent`, `Sprite`, `SkeletonPart`). No Spine code. | `skeletal::a_spine_export_imports_as_a_skeleton_and_its_images_as_sprites`, `skeletal::the_setup_pose_draws_each_slot_as_a_sprite_under_the_skeleton`, `skeletal::two_bone_ik_reaches_the_target_bending_the_way_it_is_told`, `skeletal::an_animation_plays_its_keys_over_the_setup_pose`, `skeletal::skins_attachment_keys_colours_and_draw_order_change_the_parts`; `pose::tests` (bezier, stepped, inheritance); the puppet example's three scenarios |
| "...the state machine and events are the same either way." | Done | A skeleton is a sprite to argus-anim: each of its animations is a clip (`AnimStore::sprites`, [`argus_anim::skeleton_frames`] frames at the rate that plays it in its own length), and an `Animator` on an entity with a `Skeleton` plays its clips (`Animator.sprite` empty). So states, transitions with windows, frame events, anticipation, holds, and crossfades work unchanged; the pose follows the animator's frame exactly (`Animator.frame_len`, written by `anim.update`, gives how far through a stretched frame it is). A crossfade mixes the pose it leaves, held, into the new one at the animator's blend weight. Without an Animator, `Skeleton.animation` plays at `speed`, looping or not, crossfading for `blend` seconds. | `skeletal::an_animator_plays_the_skeleton_and_the_pose_follows_its_frame` |
| Clip rates and events by frame stay tune fields and ordinary events. | Done | `Skeleton.speed`, `blend`, `scale`, and `z` are tune fields, as the animator's are. A Spine event is an engine event named as in Spine, `{entity, animation, time, int, float, string}` (the key's values, else the event's defaults), fired when the animation passes its time: every loop, once for a one-shot, from the start on entering. | `skeletal::spine_events_fire_as_the_animation_passes_them`; puppet `walk`, `wave`, `knight` |

## §6 Determinism and time

| Spec | Status | Where | Test |
|---|---|---|---|
| The same run reaches the same world. | Done | The pose is a function of components (the Skeleton, its Animator, the asset): bones are posed in order, parts spawned in slot order, events in archetype order. | `skeletal::the_same_run_poses_the_same_and_a_seek_replays_it` |
| A run replayed from a keyframe is the run it replays. | Done | Everything a pose depends on is in components (`Skeleton.time`, `shown`, `from`, `from_time`, `mix`, `loops`, `state_time`; each part's `SkeletonPart`), so a keyframe of the world is all of it; nothing is kept outside the ECS but caches of strings, dropped when the world's frame jumps. | `skeletal::the_same_run_poses_the_same_and_a_seek_replays_it` (a keyframe in the middle of a crossfade, a blink's new tints after it); argus-daemon `skeletal::a_seek_replays_the_puppets_to_the_same_world` |

## §8 What happens is an event

| Spec | Status | Where | Test |
|---|---|---|---|
| A problem is said, once, with what to do. | Done | `skeletal.error {skeleton, message}` for a skeleton asset that is not one, a skin or an animation it does not have, once each; the skeleton still draws, in the default skin or the setup pose. The importer refuses a Spine 3 export with its version and a broken file with its line, and warns, with the line, for what it does not draw (meshes, paths, clipping, transform, path, and physics constraints, deform, IK softness and stretch). A damaged file is an error or a pose, never a panic, and a clip is at most 65,536 frames however long a key's time. | `skeletal::parts_come_and_go_with_their_skeleton`, `skeletal::a_skin_or_an_animation_the_skeleton_lacks_is_said_once`, `skeletal::a_skeleton_from_another_spine_or_not_spine_at_all_is_refused_with_its_place`, `skeletal::a_spine_export_imports_as_a_skeleton_and_its_images_as_sprites`, `skeletal::a_damaged_skeleton_is_an_error_or_a_pose_never_a_panic` |

## §15 Shipping

| Spec | Status | Where | Test |
|---|---|---|---|
| A release plays what the project plays. | Done | The skeleton's JSON ships as a file; its images go through the sprite catalog and its pages like any sprite. The module is always compiled in: it is the engine's own code, without a dependency, and costs nothing without a Skeleton. | argus-player `release_skeletal::the_puppets_ship_and_play_from_their_packs_as_from_their_folder` |
| "Costs nothing for a game that doesn't attach the relevant components." | Done | A scene without a `Skeleton` or a part pays one look at its archetypes a tick. | `skeletal::a_scene_without_skeletons_pays_nothing` |

## §17 What it costs

| Measure | Value |
|---|---|
| 100 skeletons of 7 slots walking, IK on (release) | 0.36 ms a tick mean, 0.40 p95 |

The tick includes the hierarchy placing the 700 parts. Gated loosely in `skeletal::perf_a_hundred_skeletons_walking` (release, ignored) at 1.5 ms. No §17 row names skeletons.

## Known limitations, stated so they are decisions

- One animation plays at a time, with a crossfade between two: Spine's tracks (an animation layered over another) are not there; the animator is the one state machine.
- Region attachments only: a mesh, and so deform, cannot be drawn by a sprite. A part with shear is drawn by its rotation and scale (a sprite has none).
- Images unpacked (Spine's "images folder" export); a packed `.atlas` and the binary `.skel` are not read.
- A skin cannot hide an attachment the default skin has; it falls back to the default.
- IK's softness, stretch, and compress, and transform, path, and physics constraints, are not applied (a warning names each).
- An event's audio path is in the asset, not played: a game plays a sound from the event.
- Spine's arithmetic has thresholds in its own units, so the runtime poses in them and scales the parts after (`Skeleton.scale`); found when IK at 0.01 world units a pixel saw every hip as degenerate.
- Parts are entities: the hierarchy panel, `within`, and the semantic frame see them.
- `skeletal.error` is said once a session, as `anim.error` is, so a seek back before it does not say it again.
- A restored world (a seek, a fork) brings its own string table; the system's caches of interned ids are dropped whenever the world's frame does not follow the last one. Found by the daemon's seek test: a tint first seen after the keyframe drew as no tint.
