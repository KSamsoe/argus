-- What a finished job does (the job system says job.done; section 12):
-- a harvest leaves crops on the ground, a haul stores what it carried, a
-- cook's meal goes into the stockpile, and a meal feeds whoever ate it.
--
-- It runs right after the job system and before anything that posts jobs
-- (needs, fields, kitchen, haul), and forgets a job only where it is the
-- one that finished: posting in between made a second cook job for the
-- stove, which this then orphaned (found in the first week's run).

return system "sim.colony.work" {
  stage = "sim",
  after = { "jobs.update", "sim.colony.day" },
  reads = { "Colony" },
  writes = { "Field", "Item", "Stockpile", "Needs", "Station", "Transform", "Sprite" },
  doc = "On job.done: harvest drops Colony.crop_yield crops at the field and resets it; haul adds the goods to the stockpile it went to and removes them; cook adds its recipe's output to the stockpile with the most food; eat fills the eater from the stockpile, meals first. Emits colony.stored, colony.cooked, and colonist.ate.",
  run = function(q)
    local done = q:events("job.done")
    if #done == 0 then return end
    local rules = q:entity("@colony"):get("Colony")
    for _, ev in ipairs(done) do
      local kind = ev.kind
      if kind == "harvest" and ev.site ~= nil and ev.site:alive() then
        local f = ev.site:get("Field")
        f.growth = 0
        if f.job == ev.job then f.job = nil end
        local item = q:spawn({ "Item", "Transform" })
        local it = item:get("Item")
        it.kind = f.crop
        it.count = rules.crop_yield
        item:get("Transform").pos = ev.site:get("Transform").pos
        item:insert("Sprite", { kind = "rect", size = vector.create(0.4, 0.4, 0), color = q:def(f.crop).color, z = 3 })
      elseif kind == "haul" and ev.carry ~= nil and ev.carry:alive() and ev.to ~= nil and ev.to:alive() then
        local it = ev.carry:get("Item")
        local s = ev.to:get("Stockpile")
        local def = q:def(it.kind)
        if it.kind == "item:meal" then
          s.meal += it.count
        elseif it.kind == "item:crop" then
          s.crop += it.count
        else
          s.goods += it.count
        end
        s.food += it.count * def.food
        q:emit("colony.stored", { kind = it.kind, count = it.count, pile = ev.to })
        ev.carry:despawn()
      elseif kind == "cook" and ev.site ~= nil and ev.site:alive() then
        local st = ev.site:get("Station")
        if st.job == ev.job then st.job = nil end
        local r = q:def(st.recipe)
        local food = q:def(r.output).food
        local best, most = nil, -1
        for e, s in q:each("Stockpile") do
          if s.food > most then best, most = e:id(), s.food end
        end
        if best ~= nil then
          local s = q:entity(best):get("Stockpile")
          s.meal += r.count
          s.food += r.count * food
          q:emit("colony.cooked", { output = r.output, count = r.count })
        end
      elseif kind == "eat" and ev.worker ~= nil and ev.worker:alive() and ev.site ~= nil and ev.site:alive() then
        local needs = ev.worker:get("Needs")
        if needs.meal == ev.job then needs.meal = nil end
        local s = ev.site:get("Stockpile")
        local want = 1 - needs.fed
        local ate = 0
        for _, kind in ipairs({ "item:meal", "item:crop" }) do
          local food = q:def(kind).food
          local have = kind == "item:meal" and s.meal or s.crop
          local n = math.min(have, want / food)
          if n > 0 then
            if kind == "item:meal" then s.meal -= n else s.crop -= n end
            s.food = math.max(0, s.food - n * food)
            want -= n * food
            ate += n * food
          end
        end
        needs.fed = math.min(1, needs.fed + ate)
        q:emit("colonist.ate", { entity = ev.worker, food = ate })
      end
    end
  end,
}
