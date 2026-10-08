-- The settings (phase 5 item 2): from the menu, a page that cycles the
-- text size (the whole UI grows) and turns high contrast and fullscreen on
-- and off, in the UiSettings the UI and the window read. No turn passes
-- while it is used.
return scenario "the settings page scales the text and turns contrast and fullscreen on" {
  scene = "delve", seed = 3,
  run = function(t)
    t:advance { by = "turn", n = 1 }
    local turn = t:call("world.get", { entity = "@turn" }).components.Turn.n
    local hud = t:call("world.get", { entity = "@turn" }).components.UiRect.size
    t.input:hold("pause", true, 1)
    t:advance { by = "turn", n = 1 }
    -- Down to Settings, the fourth button: a turn with the key down and
    -- one with it up, as a key press is.
    for _ = 1, 3 do
      t.input:hold("ui_move", { 0, -1 }, 1)
      t:advance { by = "turn", n = 2 }
    end
    t.expect:field("@menu.settings", "UiButton.focused"):eq(true)
    t.input:hold("ui_confirm", true, 1)
    t:advance { by = "turn", n = 1 }
    t.expect:field("@settings", "UiRect.shown"):eq(true)
    t.expect:field("@menu.panel", "UiRect.shown"):eq(false)
    t.expect:field("@settings.text", "UiButton.focused"):eq(true)
    t.expect:field("@settings.text", "UiText.shown"):eq("Text size: 100%")
    -- Text size: 125%, and the HUD line grows with it.
    t.input:hold("ui_confirm", true, 1)
    t:advance { by = "turn", n = 1 }
    t.expect:field("@settings", "UiSettings.text_scale"):near(1.25, 0.001)
    t.expect:field("@settings.text", "UiText.shown"):eq("Text size: 125%")
    t.expect:field("@turn", "UiRect.size.y"):gt(hud[2])
    -- High contrast, then fullscreen.
    t.input:hold("ui_move", { 0, -1 }, 1)
    t:advance { by = "turn", n = 2 }
    t.input:hold("ui_confirm", true, 1)
    t:advance { by = "turn", n = 1 }
    t.expect:field("@settings", "UiSettings.high_contrast"):eq(true)
    t.expect:field("@settings.contrast", "UiText.shown"):eq("High contrast: on")
    t.input:hold("ui_move", { 0, -1 }, 1)
    t:advance { by = "turn", n = 2 }
    t.input:hold("ui_confirm", true, 1)
    t:advance { by = "turn", n = 1 }
    t.expect:field("@settings", "UiSettings.fullscreen"):eq(true)
    t.expect:field("@settings.fullscreen", "UiText.shown"):eq("Fullscreen: on")
    -- The semantic frame reads the page out: its buttons, the focused one
    -- among them, and not the menu's first page behind it.
    local f = t:call("obs.frame", { format = "semantic" })
    local buttons, focused = {}, nil
    for _, u in ipairs(f.ui) do
      if u.button then
        table.insert(buttons, u.text)
        if u.focused then focused = u.text end
      end
    end
    local want = { "Text size: 125%", "High contrast: on", "Fullscreen: on", "Back" }
    if table.concat(buttons, " | ") ~= table.concat(want, " | ") then
      error("the semantic frame's buttons: " .. table.concat(buttons, " | "), 0)
    end
    if focused ~= "Fullscreen: on" then error("the semantic frame's focus: " .. tostring(focused), 0) end
    -- Around again: 150%, then 100%.
    for _ = 1, 2 do
      t.input:hold("ui_move", { 0, 1 }, 1)
      t:advance { by = "turn", n = 2 }
    end
    t.expect:field("@settings.text", "UiButton.focused"):eq(true)
    t.input:hold("ui_confirm", true, 1)
    t:advance { by = "turn", n = 1 }
    t.expect:field("@settings", "UiSettings.text_scale"):near(1.5, 0.001)
    t.input:hold("ui_confirm", true, 1)
    t:advance { by = "turn", n = 1 }
    t.expect:field("@settings", "UiSettings.text_scale"):near(1.0, 0.001)
    -- Cancel: the menu's first page again; and no turn passed.
    t.input:hold("ui_cancel", true, 1)
    t:advance { by = "turn", n = 1 }
    t.expect:field("@settings", "UiRect.shown"):eq(false)
    t.expect:field("@menu.panel", "UiRect.shown"):eq(true)
    t.expect:field("@turn", "Turn.n"):eq(turn)
  end,
}
