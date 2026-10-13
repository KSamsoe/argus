-- Hunger and rest. A colonist grows hungry at the colony's rate and, below
-- eat_below, gets a job to go and eat at the stockpile with the most food
-- (only theirs, and it outranks anything else). They tire through the day
-- and work slower for it; at dusk each gets a night's sleep at their bed,
-- which ends when the day turns. One whose food runs out dies.

return system "sim.colony.needs" {
  stage = "sim",
  after = { "sim.colony.work" },
  reads = { "Colony", "Stockpile", "Colonist", "Job", "Transform" },
  writes = { "Needs", "Worker", "Job" },
  doc = "Hunger (Colony.hunger_per_day) and rest (Colony.rest_per_day, back by morning asleep); Worker.rate is 40 to 100 percent by rest. Posts an eat job below Colony.eat_below and a sleep job at dusk (Colony.dusk), ended when the day turns. One at zero fed dies: colonist.died {entity, who, cause: starvation, at}.",
  run = function(q)
    local rules = q:entity("@colony"):get("Colony")
    local dl = q.day_length
    local t = (rules.clock / dl) % 1
    local night = t >= rules.dusk
    local hunger = rules.hunger_per_day * q.dt / dl
    local tire = rules.rest_per_day * q.dt / (dl * rules.dusk)
    local recover = q.dt / ((1 - rules.dusk) * dl)
    local pile, most = nil, -1
    for e, s in q:each("Stockpile") do
      if s.food > most then pile, most = e:id(), s.food end
    end
    local site = pile and q:entity(pile)
    local dead = {}
    for e, needs, who, w in q:each("Needs", "Colonist", "Worker") do
      needs.fed = math.max(0, needs.fed - hunger)
      local sleep = needs.sleep
      local asleep = false
      if sleep ~= nil and sleep:alive() then
        local j = sleep:get("Job")
        asleep = j.stage == "working" and j.worker == e
      end
      if asleep then
        needs.rest = math.min(1, needs.rest + recover)
      else
        needs.rest = math.max(0, needs.rest - tire)
      end
      w.rate = 0.4 + 0.6 * needs.rest
      local meal = needs.meal
      if needs.fed < rules.eat_below and most > 0 and (meal == nil or not meal:alive()) then
        local j = q:spawn({ "Job" })
        local job = j:get("Job")
        job.kind = "eat"
        job.site = site
        job.only = e
        needs.meal = j
      end
      if night then
        if sleep == nil or not sleep:alive() then
          local j = q:spawn({ "Job" })
          local job = j:get("Job")
          job.kind = "sleep"
          job.site = who.bed or e
          job.only = e
          needs.sleep = j
        end
      elseif sleep ~= nil then
        -- Morning.
        if sleep:alive() then sleep:despawn() end
        needs.sleep = nil
      end
      if needs.fed <= 0 then
        table.insert(dead, e:id())
        q:emit("colonist.died", { entity = e, who = who.name, cause = "starvation", at = e:get("Transform").pos })
      end
    end
    for _, id in ipairs(dead) do q:entity(id):despawn() end
  end,
}
