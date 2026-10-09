-- The pause: the pause action (Escape, P, or Start) shows @paused and
-- freezes the game; pressed again, it hides it and the game goes on.
--
-- q:freeze() stops the rest of the tick for every system not marked
-- `unfrozen`, so nothing else needs to know about pausing. This system is
-- unfrozen, or it could never unpause. A pause menu (resume, restart,
-- settings) is UI buttons on @paused: see the swarm example's
-- prefabs/pause_menu.toml.
return system "input.pause" {
  stage = "input",
  after = { "input.actions" },
  unfrozen = true,
  reads = { "Actions", "UiNode" },
  writes = { "UiNode" },
  doc = "Shows or hides @paused on the pause action, and freezes the rest of each tick while it shows.",
  run = function(q)
    local shown = q:entity("@paused"):get("UiNode")
    if q:entity("@player"):get("Actions").pause_pressed then
      shown.visible = not shown.visible
      q:emit(shown.visible and "game.paused" or "game.resumed", {})
    end
    if shown.visible then
      q:freeze()
    end
  end,
}
