-- The player's clock: the pause action stops the colony and starts it
-- again, and the speed action steps through 1x, 3x, and 10x.
--
-- Pausing is q:freeze(): the rest of the tick stands still except systems
-- marked `unfrozen` (this one, the HUD, the engine's camera and UI), so
-- the colonists, their jobs, and the colony's clock all wait.

-- The speeds the speed action steps through. Add 30 for a faster forward.
local SPEEDS = { 1, 3, 10 }

return system "input.colony.controls" {
  stage = "input",
  after = { "input.actions" },
  unfrozen = true,
  reads = { "Actions" },
  writes = { "Colony" },
  doc = "The pause action toggles Colony.paused, and a paused colony freezes each tick; the speed action asks for the next of 1x, 3x, and 10x (q:speed).",
  run = function(q)
    local colony = q:entity("@colony")
    local rules = colony:get("Colony")
    local a = colony:get("Actions")
    if a.pause_pressed then
      rules.paused = not rules.paused
    end
    if a.speed_pressed then
      local now, nxt = q:speed(), SPEEDS[1]
      for i, x in ipairs(SPEEDS) do
        if x == now then nxt = SPEEDS[i % #SPEEDS + 1] end
      end
      q:speed(nxt)
    end
    if rules.paused then q:freeze() end
  end,
}
