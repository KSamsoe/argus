-- The menu (phase 3 item 1). Escape or Start opens it; while it is open the
-- rest of every turn is frozen but for the UI, so moving through the menu
-- takes no turn: the hero does not step and no monster moves.
return system "input.menu" {
  stage = "input",
  after = { "input.actions" },
  unfrozen = true,
  reads = { "Actions", "UiNode" },
  writes = { "UiNode" },
  doc = "Opens and closes the menu on the pause action, and freezes the rest of each turn while it is open (the UI still runs).",
  run = function(q)
    local menu = q:entity("@menu"):get("UiNode")
    if q:entity("@player"):get("Actions").pause_pressed then
      menu.visible = not menu.visible
      q:emit(menu.visible and "menu.opened" or "menu.closed", {})
    end
    if menu.visible then
      q:freeze()
    end
  end,
}
