-- The controls (phase 5 item 2): from the menu, a page of buttons that
-- show each action's key and rebind it; Back returns to the menu. No turn
-- passes while it is used.
return scenario "the controls page shows the keys and rebinds them" {
  scene = "delve", seed = 3,
  run = function(t)
    t:advance { by = "turn", n = 1 }
    local turn = t:call("world.get", { entity = "@turn" }).components.Turn.n
    t.input:hold("pause", true, 1)
    t:advance { by = "turn", n = 1 }
    -- Down to Controls, the third button.
    for _ = 1, 2 do
      t.input:hold("ui_move", { 0, -1 }, 1)
      t:advance { by = "turn", n = 2 }
    end
    t.expect:field("@menu.controls", "UiButton.focused"):eq(true)
    t.input:hold("ui_confirm", true, 1)
    t:advance { by = "turn", n = 1 }
    t.expect:field("@controls", "UiRect.shown"):eq(true)
    t.expect:field("@menu.panel", "UiRect.shown"):eq(false)
    t.expect:field("@controls.wait", "UiButton.focused"):eq(true)
    t.expect:field("@controls.wait", "UiText.shown"):eq("Wait: period")
    t.expect:field("@controls.descend", "UiText.shown"):eq("Descend: enter")
    t.expect:field("@controls.pause", "UiText.shown"):eq("Menu: escape")
    -- The semantic frame reads the page out, the focused button among them.
    local f = t:call("obs.frame", { format = "semantic" })
    local buttons, focused = {}, nil
    for _, u in ipairs(f.ui) do
      if u.button then
        table.insert(buttons, u.text)
        if u.focused then focused = u.text end
      end
    end
    local want = { "Wait: period", "Descend: enter", "Menu: escape", "Back" }
    if table.concat(buttons, " | ") ~= table.concat(want, " | ") then
      error("the semantic frame's buttons: " .. table.concat(buttons, " | "), 0)
    end
    if focused ~= "Wait: period" then error("the semantic frame's focus: " .. tostring(focused), 0) end
    -- Pressed, it waits for a key.
    t.input:hold("ui_confirm", true, 1)
    t:advance { by = "turn", n = 1 }
    t.expect:field("@controls.wait", "InputBinding.listening"):eq(true)
    t.expect:field("@controls.wait", "UiText.shown"):eq("Wait: press a key")
    -- The key, as the window hands one over. (In play the caught key takes
    -- the tick itself; a scenario moves time on its own.)
    t:call("input.device", { events = { { key = "KeyW", pressed = true }, { key = "KeyW", pressed = false } } })
    t:advance { by = "turn", n = 1 }
    t.expect:field("@controls.wait", "UiText.shown"):eq("Wait: w")
    t.expect:field("@controls.wait", "InputBinding.listening"):eq(false)
    -- Back: the menu again, and no turn passed.
    t.input:hold("ui_cancel", true, 1)
    t:advance { by = "turn", n = 1 }
    t.expect:field("@controls", "UiRect.shown"):eq(false)
    t.expect:field("@menu.panel", "UiRect.shown"):eq(true)
    t.expect:field("@turn", "Turn.n"):eq(turn)
    -- Put the key back for the other tests' sake (bindings live in the
    -- daemon's map, not the world).
    t:call("input.bind", { action = "wait", bindings = { "period" } })
  end,
}
