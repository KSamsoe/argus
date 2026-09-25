-- The line at the top of the screen: the day, the time, the food, the
-- colonists, and the speed.

return system "late.colony.hud" {
  stage = "late",
  unfrozen = true,
  reads = { "Colony", "Stockpile", "Colonist", "Worker" },
  writes = { "UiText" },
  doc = "Writes the HUD line twice a second: day and hour, food stored (meals and crops), colonists and how many are working, and the speed.",
  run = function(q)
    if q.tick % 30 ~= 0 then return end
    local rules = q:entity("@colony"):get("Colony")
    local food, meal, crop = 0, 0, 0
    for _, s in q:each("Stockpile") do
      food += s.food
      meal += s.meal
      crop += s.crop
    end
    local n, busy = 0, 0
    for _, _, w in q:each("Colonist", "Worker") do
      n += 1
      if w.job ~= nil then busy += 1 end
    end
    local t = (rules.clock / q.day_length) % 1
    local hour = math.floor(t * 24)
    local speed = if rules.paused then "paused" else string.format("%dx", rules.speed)
    local text = string.format("Day %d, %02d:00%s   food %.1f (%d meals, %d crops)   %d colonists, %d working   %s",
      rules.day, hour, if t >= rules.dusk then " (night)" else "", food, math.floor(meal), math.floor(crop), n, busy, speed)
    for _, ui in q:each("UiText") do
      ui.text = text
      return
    end
  end,
}
