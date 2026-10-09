-- The pause: Escape (or Start) shows the pause panel (scenes/level/pause.toml)
-- and the game stands still until it is pressed again.
--
-- q:freeze() stops the rest of this tick for every system but the ones
-- marked unfrozen, as this one is, so it still sees the next press. It runs
-- in the input stage, right after input.actions has turned keys into
-- actions.
return system "input.pause" {
  stage = "input",
  after = { "input.actions" },
  unfrozen = true,
  reads = { "Actions", "UiNode" },
  writes = { "UiNode" },
  doc = "Shows or hides the pause panel (@pause) on the pause action (pause.opened, pause.closed), and freezes the rest of every tick while it shows.",
  run = function(q)
    local panel = q:entity("@pause"):get("UiNode")
    if q:entity("@player"):get("Actions").pause_pressed then
      panel.visible = not panel.visible
      q:emit(panel.visible and "pause.opened" or "pause.closed", {})
    end
    if panel.visible then
      q:freeze()
    end
  end,
}
