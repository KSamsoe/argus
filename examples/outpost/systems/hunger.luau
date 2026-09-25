-- Hunger and meals: colonists grow hungry, eat from the stockpile, and
-- starve without it.

return system "sim.colony.hunger" {
  stage = "sim",
  after = { "sim.colony.found" },
  reads = { "Colony", "Needs", "Colonist" },
  writes = { "Needs", "Stockpile" },
  doc = "Colonists grow hungry at the colony's rate; one eats from the stockpile once fed drops below eat_below, filling up with what is there (a meal is a day's food); one at zero starves (colonist.died, cause starvation).",
  run = function(q)
    local colony = q:entity("@colony")
    local rules = colony:get("Colony")
    local store = colony:get("Stockpile")
    local per_tick = rules.hunger_per_day * q.dt / q.day_length
    local dead = {}
    for e, needs, who in q:each("Needs", "Colonist") do
      needs.fed = math.max(0, needs.fed - per_tick)
      -- A meal fills them up, from what the stockpile has: a meal is a
      -- day's food, so a colonist eats what they have gone without.
      if needs.fed < rules.eat_below and store.food > 0 then
        local meal = math.min(1 - needs.fed, store.food)
        store.food -= meal
        needs.fed += meal
        q:emit("colonist.ate", { entity = e, meal = meal })
      end
      if needs.fed <= 0 then
        table.insert(dead, e)
        q:emit("colonist.died", { entity = e, name = who.name, cause = "starvation" })
      end
    end
    for _, e in ipairs(dead) do e:despawn() end
  end,
}
