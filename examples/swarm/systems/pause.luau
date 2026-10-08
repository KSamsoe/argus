-- The pause (phase 5 item 2). Escape or Start opens the pause menu and the
-- game stands still: the rest of every tick is frozen while it is open,
-- but for the UI, which moves focus and presses buttons as ever. The menu
-- opens on its first page. Closed, this is one action and one flag a tick,
-- whatever the swarm's size.
return system "input.pause" {
  stage = "input",
  after = { "input.actions" },
  unfrozen = true,
  reads = { "Actions", "UiNode" },
  writes = { "UiNode" },
  doc = "Opens and closes the pause menu on the pause action (on its first page), and freezes the rest of each tick while it is open (the UI still runs).",
  run = function(q)
    local menu = q:entity("@pause"):get("UiNode")
    if q:entity("@player"):get("Actions").pause_pressed then
      menu.visible = not menu.visible
      if menu.visible then
        q:entity("@pause/panel"):get("UiNode").visible = true
        q:entity("@pause/controls"):get("UiNode").visible = false
        q:entity("@pause/settings"):get("UiNode").visible = false
      end
      q:emit(menu.visible and "pause.opened" or "pause.closed", {})
    end
    if menu.visible then
      q:freeze()
    end
  end,
}
