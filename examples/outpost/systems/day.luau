-- The colony's day: when one turns, the harvest, and the save.

return system "sim.colony.day" {
  stage = "sim",
  after = { "sim.colony.hunger" },
  reads = { "Colony" },
  writes = { "Colony", "Stockpile" },
  rng = "harvest",
  doc = "When a day turns: the fields' harvest goes into the stockpile (farm_per_day, give or take the weather), day.turned is emitted, and the game saves itself as \"auto\".",
  run = function(q)
    local colony = q:entity("@colony")
    local rules = colony:get("Colony")
    if q.day <= rules.day then return end
    rules.day = q.day
    local harvest = rules.farm_per_day * (1 - rules.weather + 2 * rules.weather * q:rng():next())
    colony:get("Stockpile").food += harvest
    q:emit("day.turned", { day = q.day, harvest = harvest })
    q:save("auto")
  end,
}
