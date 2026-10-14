-- The menu (phase 3 item 1): no turn passes while it is open, whatever
-- is pressed; Resume goes back to the floor; New run starts a new one.
return scenario "the menu takes no turn and its buttons work" {
  scene = "delve", seed = 3,
  run = function(t)
    t:advance { by = "turn", n = 1 }
    local turn = t:call("world.get", { entity = "@turn" }).components.Turn.n
    local at = t:call("world.get", { entity = "@player" }).components.Transform.pos
    t.input:hold("pause", true, 1)
    t:advance { by = "turn", n = 1 }
    t.expect:field("@menu", "UiRect.shown"):eq(true)
    t.expect:field("@menu.resume", "UiButton.focused"):eq(true)
    -- The semantic frame reads the menu out: its buttons in order, Resume
    -- focused (a screen reader's feed, section 12).
    local open = t:call("obs.frame", { format = "semantic" })
    local buttons, focused = {}, nil
    for _, u in ipairs(open.ui) do
      if u.button then
        table.insert(buttons, u.text)
        if u.focused then focused = u.text end
      end
    end
    if table.concat(buttons, " | ") ~= "Resume | New run | Controls | Settings | Saves" then
      error("the semantic frame's buttons: " .. table.concat(buttons, " | "), 0)
    end
    if focused ~= "Resume" then error("the semantic frame's focus: " .. tostring(focused), 0) end
    -- Moving through the menu: the hero does not step, no turn passes.
    t.input:hold("move", { 1, 0 }, 1)
    t.input:hold("ui_move", { 0, -1 }, 1)
    t:advance { by = "turn", n = 1 }
    t.expect:field("@menu.new", "UiButton.focused"):eq(true)
    t.expect:field("@turn", "Turn.n"):eq(turn)
    t.expect:field("@player", "Transform.pos.x"):near(at[1], 0.001)
    -- Back up to Resume and press it: the game goes on.
    t.input:hold("ui_move", { 0, 1 }, 1)
    t:advance { by = "turn", n = 1 }
    t.input:hold("ui_confirm", true, 1)
    t:advance { by = "turn", n = 1 }
    t.expect:events("menu.resume"):count():eq(1)
    t.expect:field("@menu", "UiRect.shown"):eq(false)
    t:advance { by = "turn", n = 2 }
    t.expect:field("@turn", "Turn.n"):gt(turn)
    -- New run: a new first floor, now.
    t.input:hold("pause", true, 1)
    t:advance { by = "turn", n = 1 }
    t.input:hold("ui_move", { 0, -1 }, 1)
    t:advance { by = "turn", n = 1 }
    t.input:hold("ui_confirm", true, 1)
    t:advance { by = "turn", n = 1 }
    t.expect:events("run.new"):count():eq(1)
    t.expect:field("@floor", "Floor.n"):eq(1)
    t.expect:field("@player", "Player.floor"):eq(1)
    t.expect:field("@menu", "UiRect.shown"):eq(false)
    -- The HUD says so, and the semantic frame lists it.
    local f = t:call("obs.frame", { format = "semantic" })
    local hud = f.ui[1]
    assert(hud ~= nil and string.find(hud.text, "Floor 1, turn 0"), "the HUD line: " .. tostring(hud and hud.text))
  end,
}
