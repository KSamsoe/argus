-- Rich text as it looks (reaching players item 3): the log with a
-- monster's harm in its colour and a potion's healing in its own, and the
-- menu's hint with the player's own keys. Drawn with the software
-- rasteriser and compared with its golden; `argus test name=look
-- update=true` records a new look after a change meant to show.
return scenario "the log and the menu look as they did" {
  scene = "delve", seed = 1,
  run = function(t)
    -- Waiting until a monster has hit the hero.
    local hurt = false
    for _ = 1, 120 do
      t:advance { by = "turn", n = 1 }
      local said = t.expect:field("@messages", "UiText.shown"):get()
      if string.find(said, "hits you for", 1, true) then
        hurt = true
        break
      end
    end
    if not hurt then error("no monster hit the hero in 120 turns", 0) end
    -- Then a potion, drunk with the player's own key (Q).
    t:call("sim.set", { entity = "@player", path = "Player.potions", value = 1 })
    t:call("input.device", { events = { { key = "KeyQ", pressed = true } } })
    t:advance { by = "turn", n = 1 }
    t:call("input.device", { events = { { key = "KeyQ", pressed = false } } })
    local drunk = t.expect:field("@messages", "UiText.shown"):get()
    if not string.find(drunk, "heal %[color=heal%]%d+%[/color%]") then
      error("the drink's line: " .. tostring(drunk), 0)
    end
    t.expect:image("log")
    -- The menu, its hint in the keyboard's names.
    t.input:hold("pause", true, 1)
    t:advance { by = "turn", n = 1 }
    t.expect:field("@menu.hint", "UiText.shown")
      :eq("[key]Arrow keys[/key] to choose, [key]Enter[/key] to press, [key]Esc[/key] to go back")
    t.expect:image("menu")
  end,
}
