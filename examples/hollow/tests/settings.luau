-- The settings (phase 3 item 1): from the pause menu, a page that cycles
-- the text size (the whole UI grows) and turns high contrast and
-- fullscreen on and off, in the UiSettings the UI and the window read.
return scenario "the settings page scales the text and turns contrast and fullscreen on" {
  scene = "hollow", seed = 1,
  run = function(t)
    t:step(2)
    local hud = t:call("world.get", { entity = "@hud" }).components.UiRect.size
    t.input:hold("pause", true, 1)
    t:step(2)
    -- Down to Settings, the fourth button.
    for _ = 1, 3 do
      t.input:hold("ui_move", { 0, -1 }, 1)
      t:step(2)
    end
    t.expect:field("@pause.settings", "UiButton.focused"):eq(true)
    t.input:hold("ui_confirm", true, 1)
    t:step(2)
    t.expect:field("@settings", "UiRect.shown"):eq(true)
    t.expect:field("@settings.text", "UiButton.focused"):eq(true)
    t.expect:field("@settings.text", "UiText.shown"):eq("Text size: 100%")
    -- Text size: 125%, and the HUD line grows with it.
    t.input:hold("ui_confirm", true, 1)
    t:step(2)
    t.expect:field("@settings", "UiSettings.text_scale"):near(1.25, 0.001)
    t.expect:field("@settings.text", "UiText.shown"):eq("Text size: 125%")
    t.expect:field("@hud", "UiRect.size.y"):gt(hud[2])
    -- High contrast, then fullscreen.
    t.input:hold("ui_move", { 0, -1 }, 1)
    t:step(2)
    t.input:hold("ui_confirm", true, 1)
    t:step(2)
    t.expect:field("@settings", "UiSettings.high_contrast"):eq(true)
    t.expect:field("@settings.contrast", "UiText.shown"):eq("High contrast: on")
    t.input:hold("ui_move", { 0, -1 }, 1)
    t:step(2)
    t.input:hold("ui_confirm", true, 1)
    t:step(2)
    t.expect:field("@settings", "UiSettings.fullscreen"):eq(true)
    t.expect:field("@settings.fullscreen", "UiText.shown"):eq("Fullscreen: on")
    -- Around again: 150%, then 100%.
    for _ = 1, 2 do
      t.input:hold("ui_move", { 0, 1 }, 1)
      t:step(2)
    end
    t.expect:field("@settings.text", "UiButton.focused"):eq(true)
    t.input:hold("ui_confirm", true, 1)
    t:step(2)
    t.input:hold("ui_confirm", true, 1)
    t:step(2)
    t.expect:field("@settings", "UiSettings.text_scale"):near(1.0, 0.001)
    -- Cancel: the pause menu again.
    t.input:hold("ui_cancel", true, 1)
    t:step(2)
    t.expect:field("@settings", "UiRect.shown"):eq(false)
    t.expect:field("@pause.panel", "UiRect.shown"):eq(true)
  end,
}
