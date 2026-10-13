-- Hunger. Every colonist gets hungrier at Colony.hunger_per_day; below
-- Colony.eat_below they get a job to go and eat at the stockpile. It is
-- theirs only (Job.only) and its priority (scenes/main/jobs.toml) is above
-- every other job's, so it takes them off whatever they were doing. With
-- nothing in store they cannot eat, and at zero they starve.
--
-- A second need (rest, warmth, fun) goes here the same way: a field on
-- Colonist that falls, and a job posted when it is low.

return system "sim.colony.needs" {
  stage = "sim",
  after = { "sim.colony.work" },
  reads = { "Colony", "Stockpile", "Transform" },
  writes = { "Colonist", "Job" },
  doc = "Lowers Colonist.fed by Colony.hunger_per_day; below Colony.eat_below, while the stockpile has food, posts an eat job at @colony for that colonist only. One at zero dies: colonist.died {colonist, who, cause: starvation, at}.",
  run = function(q)
    local colony = q:entity("@colony")
    local rules = colony:get("Colony")
    local store = colony:get("Stockpile")
    local hunger = rules.hunger_per_day * q.dt / q.day_length
    local dead = {}
    for e, c in q:each("Colonist") do
      c.fed = math.max(0, c.fed - hunger)
      local meal = c.meal
      if c.fed < rules.eat_below and store.food > 0 and (meal == nil or not meal:alive()) then
        local j = q:spawn({ "Job" })
        local job = j:get("Job")
        job.kind = "eat"
        job.site = colony
        job.only = e
        c.meal = j
      end
      if c.fed <= 0 then
        table.insert(dead, e:id())
        q:emit("colonist.died", { colonist = e, who = c.name, cause = "starvation", at = e:get("Transform").pos })
      end
    end
    -- Despawned after the loop, never inside it, with their meal.
    for _, id in ipairs(dead) do
      local e = q:entity(id)
      local meal = e:get("Colonist").meal
      if meal ~= nil and meal:alive() then meal:despawn() end
      e:despawn()
    end
  end,
}
