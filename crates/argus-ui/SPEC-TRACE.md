# argus-ui: spec trace

Every statement in the design document (v0.3) that constrains the UI tree (§12 "UI and text", "Input actions", and "Accessibility"; §3 hierarchy; §8 L4; §13 tweens; §17 the layout target), with where it lives and the test that pins it. Phase 3 item 1. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §12 UI and text

| Spec | Status | Where | Test |
|---|---|---|---|
| A retained UI tree built from the same entities and hierarchy as everything else. | Done | A node is an entity with a `UiNode`; its children are entities whose `Parent` is a node (the one hierarchy, §3). A top node may carry a `UiRoot` (layer, whether it takes input, a world entity to follow). Nodes are authored in scene TOML like any entity (Hollow's `rooms/a/pause*.toml`, Delve's `menu*.toml`). | `ui::a_tree_lays_out_as_a_flexbox_on_the_canvas` |
| A flexbox-style layout system. | Done | `layout.rs`: `ui.layout` (late stage, last, after the camera) lays each tree out with taffy 0.14's flexbox on the canvas of `ui.toml` (default 960 x 540): direction, justify, align, wrap, gap, padding, margin, size, min, max, grow, shrink, order, absolute placement by anchor and pivot. It writes each node's `UiRect` (place, size, draw depth, shown, alpha, clip). The taffy tree is kept between ticks and only what changed is marked. | `ui::a_tree_lays_out_as_a_flexbox_on_the_canvas`, `ui::text_is_as_big_as_its_words_wrapped_and_its_holes_are_filled` |
| Bitmap text. | Done | `text.rs`: `PixelFont`, the built-in 6 x 8 pixel font (the renderer's `argus/font` atlas), scaled to the text size. `UiText` holds the words (with `{Component.field}` holes filled each tick, as `Describe` does), size, colour, font, alignment, and wrapping; `lines` breaks at spaces and newlines and cuts a word wider than a line. | `ui::text_is_as_big_as_its_words_wrapped_and_its_holes_are_filled`, `text::tests::text_breaks_at_spaces_and_newlines_and_cuts_a_long_word` |
| SDF text with shaping. | Done | A `.ttf` or `.otf` under `assets/` imports as a distance-field atlas of the glyphs text needs (`argus-assets` `font.rs`), and `SdfFont` shapes text with rustybuzz (kerning, composition, scripts and directions as the font has them) and draws it from the atlas with the built-in `sdf_text` material, crisp at any size. A project picks it in `ui.toml` (`font`) or per text (`UiText.font`). Delve's menu and HUD use Liberation Sans (OFL, in `examples/delve/assets/fonts` with its licence). | `ui::sdf_text_is_shaped_so_it_kerns_and_composes`, `ui::a_font_asset_lays_out_by_its_advances_and_draws_from_its_field`, `argus-render` `ui::a_font_asset_draws_crisp_from_its_distance_field_at_any_size` |
| Nine-slice panels. | Done | `UiPanel`: a colour (a theme name or `#RRGGBB[AA]`), an image (a sprite asset, frame 0) stretched or nine-sliced (`slice`, the edge in pixels, drawn `scale` canvas pixels each), a border and its colour. `draw.rs` makes the nine quads. | `draw.rs` is exercised by `argus-render` `ui::a_panel_and_its_words_draw_over_the_world_and_its_flash_scaled_to_fit` (panel and border); the slicing arithmetic is checked by hand in the Hollow and Delve pictures |
| Input focus. | Done | `input.rs`: `ui.input` (sim stage, first, and still running while a game is frozen) keeps one focused `UiButton` per root: it moves to the nearest button in the direction pushed (with repeat), or to an explicit neighbour (`up`, `down`, `left`, `right`); the pointer focuses what it is over; confirm or a click presses it. A press is the event `ui.pressed {entity, event, by}` and the button's own event; focus moving is `ui.focused`, cancel `ui.cancel`. A menu that opens with nothing focused gets its first button (fixed in layout, the tick it shows). | `ui::a_menu_is_moved_through_and_pressed_with_actions`, `ui::the_pointer_focuses_what_it_moves_over_and_a_click_presses_it` |
| Not a separate world: a menu is a query away like anything else. | Done | Every UI component is an ordinary component: `world.query`, `world.get`, `edit.set`, a Luau system, and a test's `t.expect:field("@pause.resume", "UiButton.focused")` all reach it. | Hollow's `tests/pause.luau`, `tests/controls.luau`, `tests/settings.luau`; Delve's `tests/menu.luau` |
| The semantic frame (§8) reads it directly. | Done | `argus-render` `semantic.rs`: the frame's `ui` section lists each shown text and button in draw order with its words (a button's own, else its children's), its bounds in the frame, and whether it is focused or disabled; `argus render` prints it as `UI` lines. | Delve's `tests/menu.luau` (the HUD line); the web check reads the pause menu from it |
| Delve's menus. | Done | Delve's menu (Escape or Start): Resume and New run, with the floor in its title; no turn passes while it is open. Its HUD line is a UI text. | Delve's `tests/menu.luau` |
| Outpost's inspector panels and overview screens. | Deferred | Item 3 builds Outpost's core and its inspector, the gate's "menu-driven in-game inspector". | |
| Every game's pause menu. | Partial | Hollow (Resume, Restart the run, Controls, Settings) and Delve. Swarm and Outpost have none yet; Outpost's comes with its inspector (item 3). | Hollow's `tests/pause.luau`; Delve's `tests/menu.luau` |

## §12 Input actions

| Spec | Status | Where | Test |
|---|---|---|---|
| Menus are driven by actions (confirm, cancel, move) from any device and player slot; gameplay never sees keys. | Done | `UiRoot` names the actions it reads (`nav`, `confirm`, `cancel`, `pointer`, `click`; defaults `ui_move`, `ui_confirm`, `ui_cancel`, `pointer`, `ui_click`) and the slot. The mouse is a `pointer` action (a place on the canvas) and a button action. | `ui::a_menu_is_moved_through_and_pressed_with_actions`, `ui::the_pointer_focuses_what_it_moves_over_and_a_click_presses_it` |
| Rebinding at runtime is a UI over the same table. | Done | A button with an `InputBinding` (`argus-input` `rebind.rs`): pressed, it waits for the next press on its device and binds its action to it; its `keys` field says what the action is bound to, for a text hole. Hollow's pause menu has a Controls page of them. | `ui::a_controls_button_rebinds_an_action_to_the_next_key`, Hollow's `tests/controls.luau` |

## §12 Accessibility

| Spec | Status | Where | Test |
|---|---|---|---|
| Text scaling and high-contrast palettes are UI theme tokens. | Done | `ui.toml` (`theme.rs`): canvas size, font, text size, `text_scale`, `high_contrast`, `[colors]`, and `[high_contrast_colors]`; every theme has text, muted, panel, border, focus, accent, and disabled. `UiSettings` (the first in the world) overrides scale and contrast at run time; Hollow's Settings page writes it. | `ui::a_players_settings_scale_the_text_and_swap_the_palette`, `theme::tests::colours_parse_and_a_theme_overrides_the_defaults`, Hollow's `tests/settings.luau` |
| Rebinding comes with the action map. | Done | As in the input section above. | |
| The semantic frame is, without modification, a screen-reader feed. | Done | Its `ui` section is in reading order, with each button's words and focus. | Delve's `tests/menu.luau` |

## §3, §13, §17, §8

| Spec | Status | Where | Test |
|---|---|---|---|
| §3: the transform hierarchy is first-class, and UI trees use it. | Done | Children by `Parent`; a tree can also hang from a world entity (`UiRoot.follow`, `follow_offset`): it sits at that entity's place on screen through the drawing camera, sized to its content. Hollow's signs are such trees. | `ui::a_tree_hung_from_the_world_sits_at_its_place_on_screen`, Hollow's `tests/signs.luau` |
| §13: UI slides are tweens. | Done | `UiNode.offset` and `opacity` are ordinary fields, so `argus-tween` animates them. | `ui::a_tween_slides_a_node` |
| §17: a UI tree of 2,000 nodes with text lays out in under 1 ms. | Done | Measured in release, 2,000 nodes with text: a tick where nothing changed about 0.9 ms (the first layout, from nothing, about 7.5 ms). What got it there: columns read in bulk instead of a component lookup per node, keywords interned once, text measured without building lines, and the taffy tree kept between ticks so only a changed node is laid out again. | `ui::perf_2000_nodes_with_text_lay_out_in_a_millisecond` (ignored in debug; run with `--release -- --ignored`) |
| §8 L4: a menu the semantic frame lists; §8 L5: the frame drawn. | Done | The renderer draws the UI in its own pass after the post passes (a flash or a shake leaves the UI alone), through the canvas scaled to fit the target and centred, with a scissor per clipping node. | `argus-render` `ui::a_panel_and_its_words_draw_over_the_world_and_its_flash_scaled_to_fit` |

## The stopgaps it replaced

| Stopgap | Status | Where | Test |
|---|---|---|---|
| The window's one-line overlay bar (the F8 note prompt, the proposal bar). | Done | `draw::bar`: the window's words across the bottom of the canvas in the theme's font, wrapped, four lines at most (the first line, "...", then the end). `Renderer::set_bar` puts it in the UI pass; the daemon's window and the release game use it. The picture laid over the frame (`overlay.rs`, `status_bar`) is gone. | `argus-render` `render::the_windows_bar_draws_over_the_bottom_of_the_frame`, `render::post_passes_change_the_picture_in_order` (the bar is not shaken) |
| `Describe.shown` text drawn in the world. | Done | Hollow's signs and HUD and Delve's HUD are UI texts; `Describe.shown` and the renderer's world text are gone. `Describe` is the semantic frame's again, only. | Hollow's `tests/signs.luau` |
| The post flash covering HUD text. | Done | The UI pass comes after the post passes. | `argus-render` `ui::a_panel_and_its_words_draw_over_the_world_and_its_flash_scaled_to_fit` |
| No settings screen or fullscreen toggle in a release. | Done | `UiSettings.fullscreen`: the release game and the web page read it each frame and fill the screen (`argus_ui::wants_fullscreen`). Hollow's pause menu has a Settings page (text size, high contrast, fullscreen) and a Controls page. | Hollow's `tests/settings.luau`; the window switching is checked by hand |
| No pointer action for the mouse. | Done | `argus-input`: the `pointer` action type (a place on the canvas; the mouse's `position`). | `ui::the_pointer_focuses_what_it_moves_over_and_a_click_presses_it` |
| The web page draws it the same way as the desktop. | Done | The page draws the UI through the same renderer, and hands the mouse over as the pointer. | `tools/web/check.mjs`: Escape opens Hollow's pause menu, the real mouse focuses a button and a click presses it, on WebGPU and WebGL2 |

## Known limitations, stated so they are decisions

- The canvas is one fixed size per project (`ui.toml`, 960 x 540 by default), scaled to fit the window whole and centred. A wider window shows the world beside it; the UI stays in the canvas.
- Layout runs in the tick (late stage), so a `UiRect` is world state: a seek, a fork, or a replay lays out the same, and a test can read it. It costs a relayout of what changed each tick, which the §17 number covers.
- A font's atlas is one size (32 pixels an em by default) with one distance channel, so very large text rounds a glyph's sharp corners a little. A multi-channel field is later work if a game needs it.
- Glyphs are baked for printable ASCII, Latin-1, common punctuation, a sidecar's `chars`, and whatever the font's substitutions make of them. Text in another script needs its characters listed in `chars`; a glyph the atlas lacks draws the font's missing-glyph box.
- Settings last for the session. Keeping them between launches needs a place outside the world, since a replay must not change with them; that is the accessibility pass of phase 5.
- Hollow's HUD, pause menu, Controls, and Settings live in room `a`, which the scene's radius keeps loaded. The 200 rooms of item 6 will need them somewhere always loaded.
- Right-to-left shaping is rustybuzz's and was not checked here: Delve's font has no Hebrew or Arabic.
