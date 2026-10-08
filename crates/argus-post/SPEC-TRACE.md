# argus-post: spec trace

Every statement in the design document (v0.3) that constrains the post stack and the impact trio as phase 2 item 6 scopes it (§13 the post stack, §10 tune fields and tuning sessions, §8 the semantic frame's effects, §12 the camera's shake and room transitions, §17 the post stack row, §18 phase 2), with where it lives and the test that pins it. Statuses as in the other traces: **Done**, **Partial**, **Deferred**. The drawing is `argus-render`'s (`post.rs`); this crate holds the components and the two systems.

## §13 The post stack

| Spec | Status | Where | Test |
|---|---|---|---|
| Screen-space effects live in a post stack: an ordered list of passes. | Done | Each pass is a component on the drawing camera with an `order`. When any is doing something, the renderer draws the world into a texture and runs those passes, lowest `order` first, between two textures and into the window or the offscreen target (`argus-render/src/post.rs`). With none on, the world is drawn straight into the target as before. Up to 8 passes a frame. The mark prompt's bar is drawn after the passes, so a shake does not move it. | `argus-render/tests/render.rs::post_passes_change_the_picture_in_order` |
| That a game enables with parameters. | Done | A pass is on when its component is on the camera's entity file (`[PostVignette]`), and its fields are its parameters. Default orders: shake 10, aberration 20, palette 30, flash 40, vignette 50, fade 60, crt 70. With 2D lighting on (phase 5 item 3), the picture is lit before the first pass. | same, `argus-daemon/tests/post.rs` |
| Every one a `tune` field. | Done | Every parameter of every pass is a tune field: `tune.fields` lists them on the camera, a tuning session changes them live, and the tuning log records them. What the systems write (`level`, `offset`, `left`, `carry`, a flash's `amount`) is state, not tuning. | `argus-daemon/tests/post.rs` |
| Flash. | Done | `PostFlash {on, color, peak, decay}`: on its event the screen goes toward `color` by `peak`, fading back at `decay` a second (`amount` is how far now). | `a_hit_stops_the_game_and_not_the_flash_or_the_shake`, render test |
| Palette swap. | Done | `PostPalette {colors, amount}`: each pixel's brightness picks one of up to eight colours, darkest first (a four-green default), blended in by `amount`. | render test |
| Chromatic aberration. | Done | `PostAberration {amount}`: red and blue sampled apart along the line from the centre, `amount` of the width at the edge. | render test |
| Vignette. | Done | `PostVignette {amount, radius, softness}`: the edges darken from `radius` over `softness`. | render test |
| CRT. | Done (phase 5 item 3) | `PostCrt {amount, scanlines, lines, curvature, bleed, mask}`: the picture as an old screen shows it, through a curved glass (black past its edge), with scanlines across the height (one every three pixels unless `lines` says), colours bleeding sideways, and a phosphor mask (each pixel column leaning red, green, or blue). Default order 70, after the others: it is the screen itself. Every field is a tune field; the semantic frame lists it under `effects`. | `argus-render/tests/render.rs::post_passes_change_the_picture_in_order` (the corner past the glass black, the scanlines light and dark down the middle) |
| Colour-blind simulation, so the agent and the human can check readability without guessing (§12 accessibility). | Done (phase 5 item 2, in `argus-render`) | A way of looking at a frame, not a pass a game ships: `argus-render` `see_as`, after the UI, so what is checked is everything the player sees (`obs.frame {see_as}`, the editor viewport's See as). | `argus-render` `see_as` |
| The hit-flash plus hit-stop plus shake combination that makes an impact feel heavy is three tunable systems the human can adjust together in one session. | Done | `PostFlash`, `PostShake`, and `PostHitStop` on the camera, each listening for a game event by name (`on`, `hit` when empty; an `amount` field on the event scales flash and shake), so one event starts all three. They are tune fields of one entity, so one tuning session (`tune.begin`, `tune.set` on each, `tune.end`) adjusts and commits them together. Hollow's fall into the pit is the example. | `argus-daemon/tests/post.rs`, `a_scale_lets_one_tick_in_so_many_through_and_amount_scales_the_hit` |
| Hit-stop. | Done | `PostHitStop {on, frames, scale}`. `post.hitstop` runs first in the input stage and, while `left` counts down, freezes the rest of the tick (`SystemCtx::freeze` in `argus-sim`): the game's systems skip it, the tick still counts, and `unfrozen` systems run (the post effects, and the camera, which holds its view and shakes). `scale` above 0 lets one tick in 1/scale through. Input waits: what was pressed during the stop is applied on the first tick after it, and a tap that began and ended inside it still jumps (`argus-physics`). | `a_hit_stops_the_game_and_not_the_flash_or_the_shake`, `argus-daemon/tests/post.rs`, `argus-camera/tests/camera.rs::a_hit_stop_holds_the_view_and_the_shake_plays_on` |
| (Carried from phase 1.) The camera's shake is in world space; screen-space shake is this pass. | Done | `PostShake {on, trauma, decay, curve, max_offset, frequency}`: trauma through a curve, as `CameraShake`, moving the picture by a share of the screen height, so it looks the same at any window size. `CameraShake` moves the view in the world and stays as it was. | `a_hit_stops_the_game_and_not_the_flash_or_the_shake`, render test |
| (Carried from phase 1.) Fades for room transitions are post passes too. | Done | `PostFade {color, amount}`: a game tweens `amount` when `camera.zone` says the room changed (a script ordered `after = {"camera.update"}` sees the event on the tick it happens). Hollow keeps its camera slide. | `argus-daemon/tests/post.rs` |

## §6 Determinism

| Spec | Status | Where | Test |
|---|---|---|---|
| Replays, seeks, and marks reproduce what was on screen. | Done | The effects are world state advanced on the sim's ticks by `post.hitstop` and `post.update`. The shake's noise is seeded by the entity and sim time, and nothing reads the wall clock. A freeze is decided each tick from `PostHitStop.left`, so a keyframe taken inside a stop resumes it. | `the_same_run_shakes_the_same_way` |

## §8 Observation

| Spec | Status | Where | Test |
|---|---|---|---|
| The semantic frame reports active screen-space effects. | Done | Its `effects` lists each pass doing something: `screen shake {trauma, offset_px}`, `hit-stop {ticks_left, scale}`, `flash {amount, color}`, `fade`, `vignette`, `aberration`, `palette`, beside the camera's own `shake`. | `argus-daemon/tests/post.rs` |

## §17 Performance

| Spec | Status | Where | Test |
|---|---|---|---|
| Hollow: post stack, 5 passes at 1080p, under 1.5 ms of GPU; flash, shake, aberration, vignette, palette all on at once. | Done | 0.164 ms on the RTX 3070 Ti through Vulkan: a 1080p frame with the five on took 1.013 ms against 0.850 ms without, averaged over 200 frames. | `argus-render/tests/render.rs::perf_post_stack_five_passes_at_1080p` (ignored; run by hand) |

## §18 Build plan

| Spec | Status | Where | Test |
|---|---|---|---|
| Phase 2: post stack with the hit-flash and shake passes. | Done | This crate, `argus-render/src/post.rs`, installed in every scene by the daemon and by the web player. | above |

## Known limitations, stated so they are decisions

- The passes are the engine's seven. A game cannot write its own pass yet; a material is per sprite. A game-written pass (WGSL, like a material) is a later item.
- Only the drawing camera's passes run. A split screen would want a stack per camera.
- A hit-stop freezes the whole game, not one entity. The trio needs the whole-game stop; a game that wants one entity frozen writes its own component. The name `HitStop` is left free for that: §3's first example declares one from script.
- The shake moves the picture, and the uncovered strip at the edge repeats the edge pixels. At the default size (1.5% of the height at full trauma) this is a few pixels.
- The flash covers the whole screen, HUD text included, since there is no UI layer yet (phase 3).
