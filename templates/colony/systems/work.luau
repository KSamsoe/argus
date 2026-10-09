-- What a finished job does. The engine's job system (jobs.update) walks
-- a worker to a job, counts the work, carries what the job carries, and
-- then says job.done; this system makes the job's result happen:
--
--   gather, chop (any job at a Source): its goods drop at the worker's
--     feet, and the Source starts growing back
--   haul: the goods carried go into the stockpile
--   eat: the eater eats their fill from the stockpile
--
-- A new kind of job is a new branch here, the system that posts it, and
-- its JobKind in scenes/main/jobs.toml.
--
-- It runs right after the job system, and before the systems that post
-- jobs, so a job is forgotten (Source.job) before anything looks for one.

return system "sim.colony.work" {
  stage = "sim",
  after = { "jobs.update", "sim.colony.day" },
  rng = "drops",
  reads = { "Transform" },
  writes = { "Source", "Sprite", "Item", "Transform", "Stockpile", "Colonist" },
  doc = "On job.done: a job at a Source drops Source.yield (varied by Source.vary, from the seed) of its goods as an Item at the worker's feet and starts the Source growing back (colony.gathered); haul adds the Item to the stockpile's field it names and removes it (colony.stored); eat fills the eater from Stockpile.food (colonist.ate).",
  run = function(q)
    local done = q:events("job.done")
    if #done == 0 then return end
    for _, ev in ipairs(done) do
      local site, worker = ev.site, ev.worker
      if ev.kind == "haul" then
        local carry, to = ev.carry, ev.to
        if carry ~= nil and carry:alive() and to ~= nil and to:alive() then
          local item = carry:get("Item")
          local store = to:get("Stockpile")
          store[item.gives] += item.count
          q:emit("colony.stored", { gives = item.gives, count = item.count })
          carry:despawn()
        end
      elseif ev.kind == "eat" then
        if worker ~= nil and worker:alive() and site ~= nil and site:alive() then
          local c = worker:get("Colonist")
          if c.meal == ev.job then c.meal = nil end
          local store = site:get("Stockpile")
          local ate = math.min(store.food, 1 - c.fed)
          store.food -= ate
          c.fed += ate
          q:emit("colonist.ate", { colonist = worker, food = ate })
        end
      elseif site ~= nil and site:alive() and site:has("Source") then
        local src = site:get("Source")
        if src.job == ev.job then src.job = nil end
        src.growth = 0
        site:get("Sprite").color = src.bare_color
        local count = src.yield * (1 - src.vary + 2 * src.vary * q:rng():next())
        local at = site:get("Transform").pos
        if worker ~= nil and worker:alive() then at = worker:get("Transform").pos end
        local item = q:spawn({ "Item", "Transform" })
        local it = item:get("Item")
        it.gives = src.gives
        it.count = count
        item:get("Transform").pos = at
        item:insert("Sprite", { kind = "rect", size = vector.create(0.4, 0.4, 0), color = src.drop_color, z = 3 })
        q:emit("colony.gathered", { gives = src.gives, count = count, worker = worker })
      end
    end
  end,
}
