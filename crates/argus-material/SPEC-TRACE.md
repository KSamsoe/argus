# argus-material: spec trace

Every statement in the design document (v0.3) that constrains materials as phase 1 item 11 scopes it (§13 "Materials and shaders", §7 hot reload, §17 the hot-reload row, §14 assets, §10 tune fields, §18 phase 1), with where it lives and the test that pins it. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §13 Materials and shaders

| Spec | Status | Where | Test |
|---|---|---|---|
| Sprites reference a material. | Done | `Sprite.material` names one: a project asset (`materials/glow`) or a library material (`dissolve`). Empty draws the plain sprite, which is the library's `unlit`. Each material is its own pipeline; runs of instances split by material as well as atlas, and a sprite naming a material that is not loaded draws plain and is reported in the frame (`missing_materials`). A particle effect names one too (`material = "fx/glow"`). | `argus-render/tests/materials.rs::a_material_draws_its_parameters_from_each_sprites_fields`, `particles_draw_with_their_effects_material` |
| A material is a WGSL shader plus parameters. | Done | The shader is the fragment half: it defines `fn material(in: MaterialInput, p: Params) -> vec4<f32>`. The engine composes it with its prelude (the instanced sprite vertex shader; `MaterialInput` with uv, place on the sprite, world position, tint, frame rect, texel size, and sim time; `sample_atlas`; sRGB helpers), the `Params` struct generated from the sidecar, a `sample_<name>(uv)` for each texture parameter, and the entry point. Blend is `alpha`, `add`, or `multiply`. | `material::parameters_are_typed_and_laid_out_by_wgsl_rules`, `argus-render/tests/materials.rs::add_and_multiply_blend_into_what_is_drawn` |
| Declared in a sidecar with typed uniforms. | Done | `[params.<name>]` with `type` (`f32`, `vec2`, `vec4`, `color`, `texture`), `default`, `range`, `doc`, and `from`. The uniform follows WGSL's layout rules. A parameter is the same for every sprite unless `from` names a field: `"Dissolve.amount"` reads each sprite's own entity (up to four numbers a sprite, carried in the instance), `"@torch:Transform.pos"` reads a named entity once a frame. A colour is linear in the shader. Mistakes fail the import with `file:line`; unknown keys are warnings. A sidecar can take a library material's shader and change its defaults (`shader = "outline"`, `[params.width] default = 3`): a variant. | `material::sidecar_mistakes_have_lines`, `material::a_bare_shader_and_a_variant_of_a_built_in`, `argus-render/tests/materials.rs::a_parameter_from_the_wrong_kind_of_field_is_reported` |
| The engine ships a small library: lit, unlit, palette, dissolve, outline. | Done (lit Partial) | `materials/*.wgsl` and `*.toml` in this crate, read by the same parser a game's files go through and checked by the same compiler. `unlit` is the plain sprite. `dissolve` breaks the sprite into noise with a glowing edge as `Dissolve.amount` goes from 0 to 1. `outline` draws a line of colour around the opaque pixels (`Outline.color` per sprite, `width` in texels). `palette` recolours indexed or greyscale art from a palette texture, the row per sprite (`Palette.row`). `lit` is lit by an ambient colour and one point light that can follow an entity (`from = "@torch:Transform.pos"`); lighting with many lights and shadows is phase 5's 2D lighting. The per-sprite components are ordinary components, registered with `Sprite`. | `material::the_library_is_five_materials_that_compile`, `argus-render/tests/materials.rs::the_library_dissolves_outlines_recolours_and_lights` |
| A game adds its own as text. | Done | `assets/<name>.wgsl`, with `assets/<name>.toml` beside it for parameters, or a `.wgsl` alone. A project asset named like a library material takes its place. `asset.list kind=material` lists the project's and the library's; `asset.get` shows a material's shader, blend, and parameters. | `argus-assets` `importer::a_material_imports_checked_and_its_shader_errors_have_lines`, `argus-daemon/tests/materials.rs` |
| Shader authoring is a natural agent task: compile errors come back through `script.errors` with line numbers. | Done | `check` runs the composed shader through naga's WGSL parser and validator at import, with no GPU needed, and maps the error back to the line and column of the author's own file (`assets/materials/glow.wgsl:4:24: invalid field accessor powr`). A missing `material` function is named as such. The import fails with that `file:line`; `script.errors` lists it as a row with `phase: "material"`, and the last good version keeps drawing. What the GPU still refuses after that is returned by the renderer and listed too. | `material::shader_errors_name_the_line_in_the_authors_file`, `argus-daemon/tests/materials.rs` |
| And the annotated screenshot path lets the agent check the result. | Done | `obs.frame` draws materials and reports how many drew, which were missing, and which parameters could not be read (`material_problems`); with `annotate: true` (item 12) each sprite's box and id is drawn over the picture, and the semantic frame lists each sprite's material in its look and the materials in use among its effects. | `argus-daemon/tests/materials.rs`, `argus-render/tests/semantic.rs::effects_report_shake_and_materials` |
| Materials hot-reload like everything else. | Done | The watcher (or `asset.import`) re-imports a changed `.wgsl` or sidecar; the renderer recompiles only what changed, in place, keeping the material's index, so every sprite naming it draws the new version on the next frame. | `argus-render/tests/materials.rs::a_changed_material_reloads_in_place`, `argus-daemon/tests/materials.rs` |
| Every knob feel needs is a field the tuning panel can show (§13 decision, §10). | Done | Per-sprite parameters come from component fields, so they are tune fields where the component says so (`Dissolve.amount` and `Palette.row` are), tweenable (`q:tween(e, "Dissolve.amount", ...)`), and logged like any other write. Material-wide parameters are in the asset file, which reloads. | `argus-daemon/tests/materials.rs` |

## §17, §7, §14

| Spec | Status | Where | Test |
|---|---|---|---|
| Hot reload under 100 ms (the Luau row; the checklist holds materials to it). | Done | Measured on the development laptop in a debug build, through the protocol: an edit to a `.wgsl`, then `asset.import` (read, compose, naga check, GPU compile) in 20.5 ms; in the renderer alone, reload and redraw in 14 ms. | `argus-daemon/tests/materials.rs` (asserts under 100 ms), `argus-render/tests/materials.rs::a_changed_material_reloads_in_place` |
| Assets are sidecars an agent can write; every import is an event with warnings (§14). | Done | A material imports like any asset: `asset.imported`, `asset.warning`, `asset.failed` with `file:line`. | `argus-assets` `importer::a_material_imports_checked_and_its_shader_errors_have_lines` |

## §18 Build plan

| Spec | Status | Where | Test |
|---|---|---|---|
| Phase 1: materials with hot reload. | Done | This crate, the material asset in `argus-assets`, the pipelines in `argus-render`, and the daemon (`script.errors`, `asset.list`, `asset.get`, reload on import). | |

## Known limitations, stated so they are decisions

- A sprite carries four numbers for its material. A per-sprite colour takes all four, which is why `outline`'s width is per material (make a variant for a thicker line).
- A parameter's `from` reads numbers, vec2s, and colour strings. There is no vec4 field type for it to read yet.
- `outline` draws inside the frame, so art needs a clear margin as wide as the line.
- Texture parameters sample frame 0 of a sprite asset. The cache pads textures to a power of two, and `sample_<name>` maps 0 to 1 onto the frame, not the padded texture.
- `lit` has one light. Several lights and shadows are phase 5.
- Screen-space passes (the post stack) are phase 2 and are not materials.
- Materials are drawn with `textureSampleLevel` at level 0 (there are no mipmaps), so a shader may sample anywhere, branches included.
- The per-sprite values made every instance 80 bytes, from 64. Building 200,000 sprites a frame went from 17.1 ms to 19.6 ms in release.
