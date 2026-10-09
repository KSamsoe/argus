-- The pause. Every key press is a turn on the discrete clock, so a pause
-- has to stop the turn: while @pause shows, q:freeze() stands the rest of
-- every turn still (only `unfrozen` systems run), so a key pressed while
-- paused moves nothing. The press that pauses or goes on is frozen too,
-- so neither costs a turn.
return system "input.pause" {
  stage = "input",
  after = { "input.actions" },
  unfrozen = true,
  reads = { "Actions", "UiNode" },
  writes = { "UiNode" },
  doc = "Shows and hides the pause screen (@pause) on the pause action, and freezes the rest of every turn while it shows and on the press itself.",
  run = function(q)
    local pause = q:entity("@pause"):get("UiNode")
    local pressed = q:entity("@player"):get("Actions").pause_pressed
    if pressed then
      pause.visible = not pause.visible
      q:emit(pause.visible and "paused" or "unpaused", {})
    end
    if pressed or pause.visible then
      q:freeze()
    end
  end,
}
