-- The player's clock: 1, 2, and 3 set the speed to 1x, 3x, and 10x (section
-- 6: Outpost's speed controls are sim.speed), and space pauses. A pause
-- freezes the tick after the input stage, so the colony's clock and its
-- jobs stand still while the UI and the camera still answer.

local SPEEDS = { { "speed_1", 1 }, { "speed_3", 3 }, { "speed_10", 10 } }

return system "input.colony.controls" {
  stage = "input",
  after = { "input.actions" },
  unfrozen = true,
  reads = { "Actions" },
  writes = { "Colony" },
  doc = "Speed keys ask for 1x, 3x, or 10x (q:speed); pause toggles Colony.paused, and a paused colony freezes each tick.",
  run = function(q)
    local rules = q:entity("@colony"):get("Colony")
    for _, a in q:each("Actions") do
      for _, s in ipairs(SPEEDS) do
        if a[s[1] .. "_pressed"] and q:speed() ~= s[2] then
          q:speed(s[2])
        end
      end
      if a.pause_pressed then rules.paused = not rules.paused end
    end
    if rules.paused then q:freeze() end
  end,
}
