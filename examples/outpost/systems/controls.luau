-- The player's clock: 1, 2, and 3 set the speed to 1x, 3x, and 10x (section
-- 6: Outpost's speed controls are sim.speed), and space pauses. A pause
-- freezes the tick after the input stage, so the colony's clock and its
-- jobs stand still while the UI and the camera still answer. While the
-- menu is open (systems/menu.luau) the keys are the menu's, and these do
-- nothing.

local SPEEDS = { { "speed_1", 1 }, { "speed_3", 3 }, { "speed_10", 10 } }

return system "input.colony.controls" {
  stage = "input",
  after = { "input.actions" },
  unfrozen = true,
  reads = { "Actions", "Menu", "UiNode" },
  writes = { "Colony" },
  doc = "Speed keys ask for 1x, 3x, or 10x (q:speed); pause toggles Colony.paused, and a paused colony freezes each tick. Not while the menu is open.",
  run = function(q)
    local rules = q:entity("@colony"):get("Colony")
    local menu = false
    for _, _, n in q:each("Menu", "UiNode") do
      if n.visible then menu = true end
    end
    if not menu then
      for _, a in q:each("Actions") do
        for _, s in ipairs(SPEEDS) do
          if a[s[1] .. "_pressed"] and q:speed() ~= s[2] then
            q:speed(s[2])
          end
        end
        if a.pause_pressed then rules.paused = not rules.paused end
      end
    end
    if rules.paused then q:freeze() end
  end,
}
