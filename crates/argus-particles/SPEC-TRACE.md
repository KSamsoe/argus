# argus-particles: spec trace

Every statement in the design document (v0.3) that constrains particles as phase 1 item 10 scopes it (§13 "Particles and VFX", §7 hot-path systems, §3 write sets, §6 determinism, §14 assets, §17 the particle row, §18 phase 1), with where it lives and the test that pins it. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §13 Particles and VFX

| Spec | Status | Where | Test |
|---|---|---|---|
| An Emitter component. | Done | `Emitter { effect, playing, rate_scale, burst, auto_despawn }` plays the effect asset named by `effect` at its entity's position. `particles.update` writes `time`, `live`, `emitted`, `dropped`, `carry`, `started`, and `done`, so how many particles an emitter has out is a `world.get`. Setting `started = false` plays it again from the start. | `a_burst_then_the_rate_and_lifetimes_bound_the_pool`, `a_finished_emitter_says_so_and_can_despawn_itself`, `argus-daemon/tests/particles.rs` |
| Shape. | Done | `shape = "point"`, `"circle"` with `radius`, or `"box"` with `box = [w, h]`: particles are born anywhere in it, evenly, turned with the emitter's rotation. What each particle draws is `particle = "rect"` or `"circle"`, or `sprite = "<atlas>"` with `frame`. | `the_same_seed_emits_the_same_particles`, `argus-render/tests/render.rs::particle_pools_draw_as_one_run_each_between_ticks` |
| Rate or burst. | Done | `rate` per second, times `Emitter.rate_scale`. Each particle is born at the moment the running total crosses a whole number, where the emitter was at that moment, so a moving emitter leaves an even trail and not a clump per tick. `burst` fires when it starts; `duration` ends the rate (0 is for as long as it plays). A burst on demand is `Emitter.burst = n` or the `particles.burst {emitter, count, pos}` event from any system or script, whether or not the emitter is playing. The effect's `max` is the pool's size; what does not fit is counted in `dropped`. | `a_burst_then_the_rate_and_lifetimes_bound_the_pool`, `bursts_come_from_the_field_and_from_events_at_a_position`, `max_caps_the_pool_and_counts_what_was_dropped`, `a_moving_emitter_leaves_an_even_trail_and_local_particles_follow` |
| Lifetime. | Done | `lifetime` is seconds, a number or `[min, max]`. An emitter whose emission has ended and whose last particle has died emits `particles.done {emitter}` once, and with `auto_despawn` its entity goes. | `a_finished_emitter_says_so_and_can_despawn_itself`, `random_lifetimes_leave_holes_that_close_in_birth_order` |
| Curves over lifetime for velocity, size, colour, and rotation. | Done | `velocity` (a speed multiplier), `size` (world units), `rotation` (degrees), and `color` (`#RRGGBB` or `#RRGGBBAA`), each `[[t, value], ...]` with t from 0 to 1 and linear between points, or one value. The velocity curve runs in the update loop exactly, as a start value plus ramps. Size, rotation, and colour are baked into 64-step tables once per import; the renderer reads them by each particle's life. Alongside them: `speed`, `direction`, `spread`, `gravity`, `drag`, `space` (world, or local to follow the emitter), and `z`. | `particles_move_with_gravity_drag_and_the_velocity_curve`, `curves_bake_into_tables_the_renderer_reads` |
| All as TOML with the curves as control-point lists. | Done | An effect is an asset (`argus-assets` `effect.rs`): `type = "emitter"`, or a sidecar with `rate`, `burst`, or `lifetime`. Mistakes fail the import with `file:line`; unknown keys are warnings; `asset.list kind=emitter` lists them. Editing the file changes every emitter that plays it on the next import, particles already out included; a file that stops importing keeps its last good effect. | `argus-assets` `importer::an_emitter_imports_with_its_curves_and_its_mistakes_have_lines`, `effects_come_from_the_importer_and_a_broken_file_keeps_the_last_good_one`, `argus-daemon/tests/particles.rs` |
| Particles are not entities; each emitter owns an SoA pool updated by a Rust system. | Done | A `Pool` is six arrays of f32 (x, y, vx, vy, life used, life per second), 24 bytes a particle, oldest first, in the `ParticleStore` keyed by emitter. `particles.update` is Rust, last in the late stage. Where a particle was a tick ago is its position less one step of its velocity, so it is not stored. The dead are closed up in birth order, so overlapping particles never swap in the draw; a dead front, the usual case because the oldest die first, is skipped rather than moved, and reclaimed once it outgrows the living. | `random_lifetimes_leave_holes_that_close_in_birth_order` |
| And drawn instanced. | Done | `Renderer::render_frame(world, tick, alpha, extras, Some(&store))` draws each pool as one run of instances in the sprite pipeline, from its effect's atlas at its z, culled to the view, and between its last two ticks. The runs join the sorted sprites without sorting any particle. | `argus-render/tests/render.rs::particle_pools_draw_as_one_run_each_between_ticks` |
| So Swarm's fifty thousand sparks cost nothing to the ECS. | Done | Swarm's arena and last-stand scenes throw twelve sparks at each kill through one emitter entity and a `particles.burst` event; the ECS holds one entity however many sparks are out. The three Swarm scenarios still pass, and the stress scene, which logs no kills, throws none. | `examples/swarm` (`argus --project examples/swarm test`) |
| Screen-space effects live in a post stack. | Deferred | Phase 2, as the checklist scopes it. | |

## §17, §7, §3, §6, §14

| Spec | Status | Where | Test |
|---|---|---|---|
| Swarm: 250,000 live particles across emitters at under 1 ms. | Done | Measured in release on the development laptop: 250,000 live particles across 50 moving emitters, each emitting 5,000 a second with gravity, drag, and a velocity curve, at 0.43 ms mean and 0.61 ms p95 for `particles.update`. About half is birth (some 4,200 particles a tick); the update is one loop over the arrays side by side that the compiler vectorises. The first version, which looked the velocity curve up in a table and closed gaps as it went, took 0.88 ms. | `perf_a_quarter_million_particles` (ignored; `cargo test --release -p argus-particles -- --ignored`) |
| Hot-path systems (particle update among them) are Rust (§7). | Done | This crate. | |
| Systems declare what they read and write (§3). | Done | `particles.update` writes `Emitter` and reads `Transform`, `GlobalTransform`, and `Parent`; `world.who_writes Emitter` names it. With no emitters it returns at once and allocates nothing. | `argus-daemon/tests/particles.rs`, `argus-daemon/tests/protocol.rs` (the allocation check) |
| Deterministic from the seed (§6). | Done | Randomness comes from the system's own stream (`particles`) of the run's seed; emitters run in entity order. The same seed gives the same particles in the same places; another seed does not. | `the_same_seed_emits_the_same_particles` |
| Assets are sidecars an agent can write; every import is an event with warnings (§14). | Done | As any sidecar: `asset.imported`, `asset.warning`, `asset.failed`. An emitter naming an effect that does not exist emits `particles.error` once, naming the effects that do. | `an_unknown_effect_is_reported_once`, `argus-daemon/tests/particles.rs` |
| Every knob feel needs is a field the tuning panel can show (§13 decision). | Partial | `Emitter.rate_scale` is a tune field. The effect's own numbers live in its asset file, which reloads on import; showing them in the tuning panel is phase 2's. | `a_burst_then_the_rate_and_lifetimes_bound_the_pool` |

## §18 Build plan

| Spec | Status | Where | Test |
|---|---|---|---|
| Phase 1: emitter pools. | Done | This crate, the effect asset in `argus-assets`, the draw in `argus-render`, and particles in every scene in the daemon. | |

## Known limitations, stated so they are decisions

- Pools are presentation state. They are not in a world snapshot: a restored world starts with empty pools, and `Emitter.live` catches up on the next tick. A replay from the start reproduces them.
- Particles go with their emitter's entity; `auto_despawn` waits for the last one before despawning it.
- Particles collide with nothing.
- A local effect follows its emitter's position, not its rotation or scale.
- An emitter with a `Parent` is placed from its `GlobalTransform`, which the hierarchy pass writes after the late stage, so it trails its parent by a tick. A root emitter uses this tick's `Transform`.
- Particles draw with ordinary alpha blending in the sprite pipeline; there is no additive blending yet (materials are item 11).
- Size, rotation, and colour over life have 64 steps; velocity is exact.
- Between ticks a particle is drawn a fraction of its last velocity step back, so on its first frame it can sit a hair behind where it was born.
- Each `particles.burst` is a named event with fields; thousands a tick cost the event log. `Emitter.burst` costs nothing.
- In a discrete scene particles advance with the turn clock, as animators do.
