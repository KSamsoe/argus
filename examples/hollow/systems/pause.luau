-- The pause (phase 3 item 1). Escape or Start opens the pause menu and the
-- game stands still: the rest of every tick is frozen while it is open,
-- but for the UI, which moves focus and presses buttons as ever. The run's
-- clock does not count a pause.
return system "input.pause" {
  stage = "input",
  after = { "input.actions" },
  unfrozen = true,
  reads = { "Actions", "UiNode" },
  writes = { "UiNode", "Run" },
  doc = "Opens and closes the pause menu on the pause action, and freezes the rest of each tick while it is open (the UI still runs); the run's clock does not count a pause.",
  run = function(q)
    local menu = q:entity("@pause"):get("UiNode")
    if q:entity("@player"):get("Actions").pause_pressed then
      menu.visible = not menu.visible
      q:emit(menu.visible and "pause.opened" or "pause.closed", {})
    end
    if menu.visible then
      q:entity("@hud"):get("Run").started += q.dt
      q:freeze()
    end
  end,
}
