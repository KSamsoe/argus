# argus-render: spec trace

Every statement in the design document (v0.3) that constrains the renderer as phase 1 item 1 scopes it (§2 process model, §6 interpolation, §8 L5, §9 CI, §14 placeholders, §17 targets, §18 phase 1), with where it lives and the test that pins it. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §2, §18 The core hosts the renderer; windowed and offscreen

| Spec | Status | Where | Test |
|---|---|---|---|
| The core process hosts the renderer; clients reach it only through the protocol. | Done | The daemon owns the renderer (`argus-daemon`, `render` feature, on by default): `obs.frame` renders offscreen to a PNG, `render.window {open, width, height}` opens or closes the window the daemon presents into, `render.status` reports. `argusd --window [WxH]` and `argus daemon run --window` open it at start. | `argus-daemon/tests/render.rs::obs_frame_writes_a_png_of_the_authored_sprites` |
| Windowed mode. | Done | `window.rs`: a winit window pumped from the daemon thread without blocking it (`with_any_thread` on Windows); the server loop presents every frame while a window is open, playing or not; closing it emits `window.closed`. Device events are queued for item 2. | (manual: `argus daemon run --window`) |
| Offscreen mode; tests and CI render identically with the software adapter (§9). | Done | `Gpu::offscreen` renders to a texture and reads back tightly packed RGBA; `software: true` forces wgpu's fallback adapter (DirectX 12 WARP on Windows, lavapipe or llvmpipe on Linux), and a machine with no hardware adapter takes the fallback on its own. A machine with no adapter at all reports error 5001 and the crate's tests skip with a message rather than fail. | `render::described_shapes_render_where_the_camera_puts_them` |

## §6 Interpolation

| Spec | Status | Where | Test |
|---|---|---|---|
| Rendering is decoupled from the fixed-rate sim and interpolates between the last two sim states. | Done | `Clock::alpha` (how far the next tick is owed, 1 on the discrete clock) reaches the renderer as `render(world, tick, alpha)`; `Renderer` keeps every sprite's position and rotation from the previous tick by entity index and lerps when exactly one tick passed. A tick gap draws the current state. Fixed in item 7: the previous poses used to be replaced on every frame, so only the first frame after a tick interpolated and later frames of the same tick snapped; now they move on only when the tick does. The drawing camera is interpolated the same way. | `render::positions_interpolate_between_the_last_two_ticks` |

## §17 Targets

| Spec | Status | Where | Test |
|---|---|---|---|
| 200,000 instanced sprites per frame; one draw call per texture atlas; GPU-side culling. | Done | One pipeline per material (since item 11), one instance buffer, six vertices per instance from `vertex_index`; instances sorted by z, then material, then atlas, and drawn as runs, one `draw` per run, so an atlas is one call unless z interleaves atlases or materials (then one per run, reported in `draw_calls`). The vertex shader collapses an instance whose bounding circle misses the view to a degenerate triangle, so nothing is culled on the CPU. Measured: 200k sprites over two atlases and four z values, 8 draw calls, 19.6 ms to build and submit on the development laptop (RTX 3070 Ti, Vulkan; it was 23 ms at item 1 and 17.1 ms before item 11 made each instance 16 bytes bigger). The build is the CPU cost; it iterates the columns directly. | `render::perf_200k_sprites_per_frame` (release, ignored), `render::z_orders_and_atlas_runs_make_the_draw_calls` |
| Tiles draw in the same pass as sprites (item 5). | Done | `Renderer::render_with(world, tick, alpha, extras)` takes sprites that are not entities (`Extra`: atlas, frame, centre, size, z) and sorts them with the entity sprites, so tiles and sprites share the z order and the atlas runs. `Renderer::view` and `view_for` give the camera's view in world units, which is what the daemon asks the tilemap for. A missing atlas draws magenta and is reported like a sprite's. | `argus-daemon/tests/tiles.rs` |
| Instanced sprites from atlases. | Done | Since item 4 the importer owns `assets/`: `Renderer::load_catalog(&Importer)` loads every sprite asset's cached texture with its frame rects and pivots, `reload_sprites` swaps changed ones while running, and `load_atlases(root)` imports and loads in one call. `Sprite.atlas` names the asset (`sprites/hero`) and `Sprite.frame` picks the frame; the frame's pivot sits on the entity's position (mirrored by `flip_x`). | `render::a_named_atlas_loads_from_assets_and_a_missing_one_is_reported`, `render::a_placeholder_asset_draws_with_its_pivot_and_the_art_replaces_it` |

## §14 Placeholders

| Spec | Status | Where | Test |
|---|---|---|---|
| A placeholder sprite as a described shape (`kind = "rect"`, `size = [8, 8]`, `color = "#E8703F"`) so gameplay never waits on art. | Done | `Sprite { kind, size, color }` with no atlas draws from the built-in `rect` (1×1 white) or `circle` (64×64) atlases tinted by the colour; the colour is `#RRGGBB` or `#RRGGBBAA`, parsed once per distinct string. A sprite naming an atlas that is not loaded draws as a magenta rect and is listed in `missing_atlases`. | `render::described_shapes_render_where_the_camera_puts_them`, `render::a_named_atlas_loads_from_assets_and_a_missing_one_is_reported` |

## §8 Observation (the part this item ships)

| Spec | Status | Where | Test |
|---|---|---|---|
| L5 `obs.frame {format: "png"}`. | Partial | The PNG; `annotate: true` and `format: "semantic"` are item 12, and `obs.frame` refuses them with a hint until then. The path defaults to `.argus/frames/frame-<tick>.png`. | `argus-daemon/tests/render.rs` |

## §12 Camera (the part this item ships)

| Spec | Status | Where | Test |
|---|---|---|---|
| Cameras are entities. | Done | Since item 7 `Camera` and `CameraShake` live in `argus-camera` (re-exported here). The view is the drawing camera's (`argus_camera::current`: active, highest priority), its position interpolated plus its shake offset, rolled by the shake's angle in the shader (the view uniform carries the roll; culling uses the rolled view's box). `FrameReport` says which camera drew and the view's centre, half size, and angle; `to_pixel` accounts for the roll. `view_for` gives a rolled view's bounding box for tile queries. | `render::described_shapes_render_where_the_camera_puts_them`, `render::the_priority_camera_draws_and_its_shake_moves_and_rolls_the_view` |
| Particles drawn instanced (§13, item 10). | Done | `render_frame` takes the particle store beside the tile extras: each pool is one run of instances from its effect's atlas (or the rect and circle shapes) at its z, sized, turned, and coloured from the effect's tables by each particle's life, culled to the view's box, and drawn between ticks from its velocity. Pools are merged into the sorted sprites as whole runs, so no particle is sorted. `FrameReport.particles` counts what was drawn. Building the runs now compares atlas indices instead of cloning a name per instance. | `render::particle_pools_draw_as_one_run_each_between_ticks` |
| Sprites reference a material (§13, item 11). | Done | `Sprite.material`. The shader is `argus_material::compose`'s output (the prelude's vertex shader, the material's fragment function, its generated parameters), the same text the importer checked, compiled once per material inside a validation error scope so a refusal is an error, not a crash; a changed material recompiles in place under the same index. Each instance carries four numbers for its material, read from the sprite's entity where a parameter says `from`; each material's uniform is written once a frame from its defaults, named entities, and its textures' frames. The view uniform carries the sim time for `in.time` (`Renderer::set_time`). `FrameReport` has `missing_materials` and `material_problems`, `DrawStats` the materials drawn with. Particles draw with their effect's material. | `materials::a_material_draws_its_parameters_from_each_sprites_fields`, `materials::a_changed_material_reloads_in_place`, `materials::particles_draw_with_their_effects_material` |
| An animator's crossfade (§13, item 8). | Done | `Sprite.blend_frame` and `Sprite.blend` (0 to 1): the renderer draws that frame of the same atlas over the sprite's frame at that weight, as a second instance just above it in draw order, flipped and pivoted like the first. The animator writes both. | `argus-anim/tests/anim.rs::a_blend_crossfades_the_outgoing_frame_out` |

## Known limitations, stated so they are decisions

- The tests that need a GPU skip only when the machine has no adapter at all. Before item 11 they skipped on any failure to start the renderer, so a renderer that could not build its pipeline passed every test; item 11 found that when a binding change broke the pipeline and every render test still passed.

- Colours are sRGB in the file and linear on the GPU; the clear colour is given linear. The tests know.
- The frame build is single-threaded and costs about 0.1 µs per sprite; at 200k that is 19.6 ms, over a 16.6 ms frame. Building instances in parallel across archetypes, or keeping a persistent instance buffer updated by the sim, are the two ways down, and both are later work; the target sentence is about instancing and draw calls, which are met.
- z is a sort key, not a depth buffer: overlapping sprites with equal z draw in atlas order, then entity order.
- One sampler (nearest magnification, linear minification) for every atlas; per-material samplers come with item 11.
- The window's device events are collected but nothing reads them until item 2.
- The render ring for marks and clips (§10) is phase 2; `obs.frame` renders on demand.
