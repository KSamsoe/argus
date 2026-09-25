-- Goods lying about are hauled to the nearest stockpile. Posting is capped
-- (Colony.haul_open jobs at a time) and looked at once a second, so twenty
-- thousand stones cost nothing a tick.

return system "sim.colony.haul" {
  stage = "sim",
  after = { "sim.colony.kitchen" },
  reads = { "Colony", "Stockpile", "Transform" },
  writes = { "Item", "Job" },
  doc = "Once a second, while fewer than Colony.haul_open haul jobs are open, posts haul jobs for goods without one: to the nearest stockpile, carrying the goods.",
  run = function(q)
    if q.tick % 60 ~= 0 then return end
    local rules = q:entity("@colony"):get("Colony")
    local open = 0
    for _, j in q:each("Job") do
      if j.kind == "haul" and j.worker == nil then open += 1 end
    end
    local want = rules.haul_open - open
    if want <= 0 then return end
    local piles = {}
    for e, _, t in q:each("Stockpile", "Transform") do
      table.insert(piles, { id = e:id(), at = t.pos })
    end
    if #piles == 0 then return end
    for e, item, t in q:each("Item", "Transform") do
      if item.job == nil or not item.job:alive() then
        local best, bd = nil, math.huge
        for _, p in ipairs(piles) do
          local d = vector.magnitude(p.at - t.pos)
          if d < bd then best, bd = p.id, d end
        end
        local j = q:spawn({ "Job" })
        local job = j:get("Job")
        job.kind = "haul"
        job.site = e
        job.carry = e
        job.to = q:entity(best)
        item.job = j
        want -= 1
        if want <= 0 then return end
      end
    end
  end,
}
