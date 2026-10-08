# argus-rigid: spec trace

Every statement in the design document (v0.3) about the dynamic physics tier (§12, with §6 and §15 where they touch it, and §18's phase 5 sentence), with where it lives and the test that pins it. Statuses: **Done**, **Partial**, **Deferred**.

## §12 Physics, in tiers

| Spec | Status | Where | Test |
|---|---|---|---|
| "Dynamic: real rigid bodies with joints, from an optional Rapier module for the games that need a physics toy, not a default." | Done | `RigidBody` (velocity, spin, gravity scale, damping, density, friction, restitution, fixed or kinematic, locked rotation, continuous collision, sleep) on an entity with argus-physics' `Collider` (a box, or a circle with a radius); `Joint` on any entity: revolute, fixed, prismatic, rope, or spring, between two bodies or a body and the world, with limits, a motor, contacts between the two on or off, and a force that breaks it (`joint.broken`); `RigidWorld`: gravity and solver iterations. `physics.rigid` steps Rapier 0.36 once a tick. The example game is `examples/toybox`: a stack, a pendulum, a plank bridge pinned at both ends, and balls a Luau system drops. | `rigid::a_box_falls_lands_and_sleeps_and_the_landing_is_an_event`, `rigid::a_stack_stands_and_a_hinge_swings`, `rigid::a_weak_joint_breaks_under_a_load`, `rigid::a_throw_and_a_teleport_are_gameplays_to_make`; toybox's three scenarios |
| Three tiers "chosen per entity, sharing one broadphase". | Done | The components choose the tier. The kinematic tier's bodies are kinematic bodies in Rapier, moved to where `physics.move` put them, so they push rigid bodies (they are not pushed back); every Tilemap's solid tiles are fixed colliders (merged into as few boxes as rows allow, a polygon per slope, rebuilt when a map's cells change). Rigid bodies are in the world's one spatial hash like any entity, so `within`, `nearest`, and `physics.overlap` (which runs after this tier: `Scheduler::order_after`) see where the step left them. Rapier keeps its own broadphase for its solver. | `rigid::a_moving_platform_of_the_kinematic_tier_pushes_a_box`, `rigid::a_box_lands_on_a_tilemaps_solid_tiles`, `rigid::rigid_bodies_are_in_the_shared_broadphase_and_the_overlap_pass` |
| (§8) What happens is an event. | Done | `contact.begin {entity, other, impulse}` and `contact.end {entity, other}` for every pair that starts or stops touching where one is a rigid body (the other may be a body, a kinematic body, or a Tilemap), sorted, the lower entity first; `joint.broken {joint, a, b, force}`; `joint.refused {joint, why}` for a kind that is not one, once. | `rigid::a_box_falls_lands_and_sleeps_and_the_landing_is_an_event`, `rigid::a_box_lands_on_a_tilemaps_solid_tiles` |
| "Costs nothing for a game that doesn't attach the relevant components." | Done | A scene without a `RigidBody` pays one look at its archetypes a tick. | `rigid::a_scene_without_rigid_bodies_pays_nothing` |

## §6 Determinism and time

| Spec | Status | Where | Test |
|---|---|---|---|
| The same run reaches the same world. | Done | Bodies, colliders, and joints enter Rapier in entity order; contacts are reported sorted; Rapier's `enhanced-determinism` feature makes its arithmetic the same on every platform (its transcendentals through libm), which item 11 measures. | `rigid::the_same_run_reaches_the_same_world` |
| A run replayed from a keyframe is the run it replays. | Done | Rapier carries contact caches and solver warm-start from one tick to the next, outside the ECS. `RigidStore::state` is all of it (bodies, colliders, joints, islands, both phases, and what ties them to entities); a keyframe clones it, a seek puts it back, and a replay file or a player's report carries it as bytes (`RigidState::encode`, version 6 of the replay file). Without it a replay parts from its run within a few ticks; the test shows that too. | `rigid::a_run_replayed_from_a_keyframe_is_the_run_it_replays`; argus-daemon `rigid::a_seek_replays_the_rigid_bodies_to_the_same_world`; argus-player `release_rigid` |
| Gameplay changes the world through components. | Done | A velocity or a Transform that is not what this tier wrote last tick is a throw or a teleport; a body's settings and shape are applied when they change; a body that leaves the world (despawned, or its components gone) leaves Rapier with its joints. | `rigid::a_throw_and_a_teleport_are_gameplays_to_make` |

## §15 Shipping

| Spec | Status | Where | Test |
|---|---|---|---|
| Optional, not a default. | Done | A game names the module in `argus.toml` (`modules = ["rigid"]`). The daemon installs it only then, so the daemon and a release agree; `argus build` builds the game's executable (and a web build) with the `rigid` feature only then, so a release of a game without rigid bodies does not carry Rapier (argus-game without the feature has no rapier2d in its tree). The daemon and the CLI, which run any game, always have it compiled in. | argus-daemon `rigid::a_game_that_does_not_list_the_module_has_none_of_it`; `argus build` of the toybox (the same world from the project, the packs, and the executable) |

## §17 What it costs

| Measure | Value |
|---|---|
| 500 boxes in a pit, all falling (release) | 1.03 ms a tick mean, 1.12 p95 |
| The same piled up, nearly all asleep | 0.86 ms mean, 0.89 p95 |

Most of it is the engine's side (reading and writing 500 entities' components a tick), not Rapier's step. Gated loosely in `rigid::perf_five_hundred_boxes_in_a_pile` (release, ignored). No §17 row names rigid bodies.

## Known limitations, stated so they are decisions

- A one-way tile is solid to a rigid body: it has no "from above".
- The kinematic tier does not see rigid bodies: a character pushes a crate but is not stopped by one, and does not stand on one.
- A rigid body with a `Parent` is left out: rigid bodies are roots, in world space.
- A save keeps the bodies' components but not Rapier's caches: a loaded game goes on from the same positions and velocities, and its first ticks' contacts are found again (`contact.begin` fires for what was already touching).
- The `impulse` of `contact.begin` is Rapier's for the step's last substep, a measure of how hard, not a physical unit; a joint's force for breaking is worked out from its substep's impulse.
- Shapes are a box or a circle (the Collider's); polygons and compound shapes wait for a game that needs them.
- Rapier 0.36 is 29 crates and 6.6 MB of source (downloaded with Kevin's approval, 2026-10-08).
