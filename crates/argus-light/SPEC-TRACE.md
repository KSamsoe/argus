# argus-light: spec trace

Every statement in the design document (v0.3) that constrains 2D lighting as phase 5 item 3 scopes it (§12 "Lighting", §13 the post stack and materials, §8 what the agent sees, §10 tune fields, §17 the post stack row, §6 a run as a value), with where it lives and the test that pins it. Statuses as in the other traces: **Done**, **Partial**, **Deferred**. This crate holds the components and what the renderer needs from the world (the lights' places, the occluders' edges, their shadows); the drawing is `argus-render`'s (`light.rs`).

## §12 Lighting

| Spec | Status | Where | Test |
|---|---|---|---|
| "2D lighting as a render pass." | Done | With lighting on, the world is drawn into a texture as with a post pass; a light map the size of the picture is cleared to the ambient light, each light is added into it, and the picture is multiplied by it before the post passes (`argus-render` `light.rs`). The UI is drawn after, unlit. | `argus-render` `tests/light.rs` |
| "Point and cone lights." | Done | `Light`: a colour, an intensity, a radius, a falloff, at the entity's position plus an offset (turned and mirrored with it); a cone when `angle` is under 180 degrees, pointing `direction` from the entity's rotation, its edge fading over `softness`. | `light::a_point_light_lights_near_it_and_the_ambient_elsewhere`, `light::a_cone_lights_where_it_points` |
| "Occluders derived from tiles and sprites." | Done | Tiles: every solid and slope cell of every tilemap's collision layers, the collision the physics uses, as the long edges where a solid cell meets an open one (`tile_edges`, over the rect the lights can reach, `reach`). Sprites: an `Occluder`, a box of the entity's `Sprite.size` by default, turned and scaled with it (`occluder_edges`). A shadow is swept from each edge that faces away from the light (`shadow`), so a wall the light shines on is lit and what is behind it is dark; a light is not shadowed by its own entity's occluder or its parent's. | `lib::tests::a_box_casts_from_its_far_side_only`, `light::a_box_casts_a_shadow_and_is_lit_on_the_light_side`; tiles: Hollow's caverns |
| "Normal-mapped sprites optional." | Done | A sprite asset's `<name>.normal.png` beside its image (`argus-assets`) is its normal map. When one is drawn, the world's sprites are drawn again into a normal buffer, each one's normals turned and mirrored with it, and each light reads it: a pixel facing the light is brighter than a flat one, facing away darker, by the light's `height` over the picture. A sprite without a map is flat. | `light::a_normal_mapped_sprite_is_lit_by_where_its_normals_face` |
| "Off by default and zero-cost when off." | Done | Lighting is on only while the drawing camera has `Lighting`. The components are registered with no systems, so ticks cost nothing; without `Lighting` the renderer makes no texture or pipeline, asks for no tile edges (`reach` is `None`), and draws exactly as before. | `light::without_lighting_nothing_changes_and_nothing_is_made`; every game's suite and image goldens unchanged until the game turned it on |
| "Hollow's caverns." | Done | The hero's camera has `Lighting`, a cool ambient whose level `late.light` glides from room to room (the cave mouth 0.6, the shaft 0.38, the hall 0.34, the deep 0.22 down to 0.1 in its bottom row) under the room fade. The hero carries a lantern (`persistent/lantern.toml`, a warm light with a little flicker, inside the hero's collider) whose light the rock tiles shadow; the hermit's fire, the gem, the exit, and the deep's crystals glow (`Emissive` and lights of their own); the lanterns mod's lanterns light when the keeper lights them. High contrast keeps the level up. | Hollow's `tests/light.luau` (the lantern listed by the hero and following them into the deep, the level gliding, the glows, the lit hollow's two image goldens), `tests/goldens/the_cave_and_the_hall_look_as_they_did` |
| "Outpost's day-night cycle." | Done | `late.colony.light` follows the HUD's own hour (`Colony.clock` over the day's length) through eight keys, smoothed each tick: white at noon, orange at dusk, blue at midnight; the stove and a campfire (`Lamp`) fade in as the light goes. | Outpost's `tests/light.luau` (a whole day tick by tick, no jump when it turns; at midnight the campfire and the stove listed, none at noon; a valley without lamps gets them) |

## §13 Materials and what glows

| Spec | Status | Where | Test |
|---|---|---|---|
| (§13) The `lit` material, "lit by an ambient colour and one point light", left the many lights and shadows to this phase. | Done | The pass lights every sprite whatever its material; `lit` stays a one-light material for a sprite that wants its own. | `argus-material` trace |
| Glows the dark should not dim. | Done | `Emissive {amount}` on a sprite draws it again after the light at its own colour; a material sidecar's `emissive = true` draws every sprite and particle with it only after the light (an additive spark). | `light::what_glows_keeps_its_colour_in_the_dark` |

## §8 What the agent sees

| Spec | Status | Where | Test |
|---|---|---|---|
| Lights are visible to the agent: the semantic frame and `world.query` see them, and a screenshot shows them. | Done | The semantic frame's `lighting` (only when on): the ambient light and its level, each light reaching the view with where it is on the screen, how far it reaches in pixels, its colour, strength, a cone's direction and angle, and whether it casts shadows. `obs.frame` (png) says how many lights it drew, from how many occluder edges, and how many sprites glowed. Lights are components, so `world.query {has: ["Light"]}` lists them, and a screenshot is the lit picture. | Hollow's and Outpost's scenarios, `argus-daemon` |

## §10 Tune fields

| Spec | Status | Where | Test |
|---|---|---|---|
| "A light's colour, radius, and intensity are tune fields" (the checklist). | Done | `Light.color`, `intensity`, `radius`, `falloff`, `angle`, `softness`, `direction`, `height`, `flicker`; `Lighting.ambient` and `level`; `Emissive.amount`. A day's cycle tweens `Lighting.level`. | `tune.fields` lists them (the daemon's tests of tune fields read the schema) |

## §6 A run is a value

| Spec | Status | Where | Test |
|---|---|---|---|
| A replay draws what the run drew. | Done | Lights are components and draw from the world as it is; a flicker is three waves of the sim time and the entity, never random (`flicker`); a light moves between ticks as a sprite does. | `lib::tests::a_steady_light_does_not_flicker_and_a_flickering_one_repeats` |

## §17 Performance

| Spec | Status | Where | Test |
|---|---|---|---|
| "Hollow post stack, 5 passes at 1080p < 1.5 ms GPU": lighting must not break it. | Done | On the RTX 3070 Ti through Vulkan, at 1080p: the five passes 0.197 ms; sixteen lights each casting shadows from a hundred boxes (400 edges) 0.189 ms more; 0.386 ms together. | `argus-render` `render::perf_post_stack_five_passes_at_1080p` (release, `--ignored`, CI's perf step) |

## Known limitations, stated so they are decisions

- Shadows are hard: a light's shadow is where its stencil is set, with no penumbra. Soft shadows would blur the light map, at a pass's cost.
- A slope tile occludes as a whole tile, and a one-way tile not at all.
- A sprite's occluder is a box, not its outline.
- The light map is the picture's size. Half floats where the device can draw and blend them; else sRGB bytes holding half the light, which still reaches twice white.
- A normal-mapped sprite keeps a texture of its own: a release pack does not put it on a shared page.
- Up to 255 shadowed lights a pass; more start a new pass (the stencil's numbers run out). `light::the_three_hundredth_shadowed_light_still_lights_and_shadows`.
- A light inside a solid tile is shadowed on every side: every edge round it faces away. Hollow's lantern sits inside the hero's collider for this.
- A child entity is placed through its `GlobalTransform`, which its file must name; a light on a child without one stays at its local position.
- The engine's components' names are the engine's: a game's script cannot declare a `Light`, `Lighting`, `Occluder`, or `Emissive` of its own.
