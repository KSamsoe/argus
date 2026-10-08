-- The settings (phase 5 item 2): from the pause menu, a page that cycles
-- the text size (the menu's words grow) and turns high contrast and
-- fullscreen on and off, in the UiSettings the UI and the window read.
return scenario "the settings page scales the text and turns contrast and fullscreen on" {
  scene = "arena", seed = 7,
  run = function(t)
    t:step(2)
    t.input:hold("pause", true, 1)
    t:step(2)
    -- Down to Settings, the fourth button.
    for _ = 1, 3 do
      t.input:hold("ui_move", { 0, -1 }, 1)
      t:step(2)
    end
    t.expect:field("@pause/panel/settings", "UiButton.focused"):eq(true)
    t.input:hold("ui_confirm", true, 1)
    t:step(2)
    t.expect:field("@pause/settings", "UiRect.shown"):eq(true)
    t.expect:field("@pause/panel", "UiRect.shown"):eq(false)
    t.expect:field("@pause/settings/text", "UiButton.focused"):eq(true)
    t.expect:field("@pause/settings/text", "UiText.shown"):eq("Text size: 100%")
    local size = t:call("world.get", { entity = "@pause/settings/text" }).components.UiRect.size
    -- Text size: 125%, and the button grows with its words.
    t.input:hold("ui_confirm", true, 1)
    t:step(2)
    t.expect:field("@pause/settings", "UiSettings.text_scale"):near(1.25, 0.001)
    t.expect:field("@pause/settings/text", "UiText.shown"):eq("Text size: 125%")
    t.expect:field("@pause/settings/text", "UiRect.size.y"):gt(size[2])
    -- High contrast, then fullscreen.
    t.input:hold("ui_move", { 0, -1 }, 1)
    t:step(2)
    t.input:hold("ui_confirm", true, 1)
    t:step(2)
    t.expect:field("@pause/settings", "UiSettings.high_contrast"):eq(true)
    t.expect:field("@pause/settings/contrast", "UiText.shown"):eq("High contrast: on")
    t.input:hold("ui_move", { 0, -1 }, 1)
    t:step(2)
    t.input:hold("ui_confirm", true, 1)
    t:step(2)
    t.expect:field("@pause/settings", "UiSettings.fullscreen"):eq(true)
    t.expect:field("@pause/settings/fullscreen", "UiText.shown"):eq("Fullscreen: on")
    -- The semantic frame reads the page out, the focused button among them.
    local f = t:call("obs.frame", { format = "semantic" })
    local list, focused = {}, nil
    for _, u in ipairs(f.ui) do
      if u.button then
        table.insert(list, u.text)
        if u.focused then focused = u.text end
      end
    end
    local want = "Text size: 125% | High contrast: on | Fullscreen: on | Back"
    if table.concat(list, " | ") ~= want then error("the semantic frame's buttons: " .. table.concat(list, " | "), 0) end
    if focused ~= "Fullscreen: on" then error("the semantic frame's focus: " .. tostring(focused), 0) end
    -- Around again: 150%, then 100%.
    for _ = 1, 2 do
      t.input:hold("ui_move", { 0, 1 }, 1)
      t:step(2)
    end
    t.expect:field("@pause/settings/text", "UiButton.focused"):eq(true)
    t.input:hold("ui_confirm", true, 1)
    t:step(2)
    t.expect:field("@pause/settings", "UiSettings.text_scale"):near(1.5, 0.001)
    t.input:hold("ui_confirm", true, 1)
    t:step(2)
    t.expect:field("@pause/settings", "UiSettings.text_scale"):near(1.0, 0.001)
    -- Cancel: the pause menu again.
    t.input:hold("ui_cancel", true, 1)
    t:step(2)
    t.expect:field("@pause/settings", "UiRect.shown"):eq(false)
    t.expect:field("@pause/panel", "UiRect.shown"):eq(true)
  end,
}
