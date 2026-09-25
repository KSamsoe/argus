-- The colony's calendar: its clock, the day turning, the weather, and the
-- autosave. The clock stands still while the game is paused (the controls
-- freeze the tick), so night falls on the colony's time, not the sim's.

return system "sim.colony.day" {
  stage = "sim",
  after = { "sim.colony.found" },
  reads = { "Stockpile" },
  writes = { "Colony" },
  rng = "weather",
  doc = "Moves the colony's clock; when a day turns, draws the day's growth from the weather, emits day.turned with the food stored, and the game saves itself as \"auto\".",
  run = function(q)
    local rules = q:entity("@colony"):get("Colony")
    rules.clock += q.dt
    local day = math.floor(rules.clock / q.day_length + 1e-9)
    if day <= rules.day then return end
    rules.day = day
    rules.growth = 1 - rules.weather + 2 * rules.weather * q:rng():next()
    local food = 0
    for _, s in q:each("Stockpile") do food += s.food end
    q:emit("day.turned", { day = day, food = food, growth = rules.growth })
    q:save("auto")
  end,
}
