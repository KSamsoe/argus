-- The menu's own key, both ways: the press that opens the menu and the one
-- that closes it take no turn, like every press while it is open. (The
-- closing press used to pass as a wait, so the monsters moved; a template
-- agent found it, distribution item 2.)
return scenario "the menu key opens and closes the menu without a turn" {
  scene = "delve", seed = 3,
  run = function(t)
    t:advance { by = "turn", n = 1 }
    local turn = t:call("world.get", { entity = "@turn" }).components.Turn.n
    t.input:hold("pause", true, 1)
    t:advance { by = "turn", n = 1 }
    t.expect:field("@menu", "UiRect.shown"):eq(true)
    t.input:hold("pause", true, 1)
    t:advance { by = "turn", n = 1 }
    t.expect:field("@menu", "UiRect.shown"):eq(false)
    t.expect:events("menu.closed"):count():eq(1)
    t.expect:field("@turn", "Turn.n"):eq(turn)
    -- The next press is a turn again.
    t:advance { by = "turn", n = 1 }
    t.expect:field("@turn", "Turn.n"):eq(turn + 1)
  end,
}
