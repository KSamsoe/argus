-- Goods lying on the ground are hauled to the stockpile: a haul job whose
-- site is the item (where the worker picks it up), whose carry is the
-- item (the job system keeps it at the worker's side), and whose to is
-- @colony (where they take it). sim.colony.work stores it on job.done.
--
-- With more than one stockpile, send each item to the nearest
-- (q:nearest(pos, r, "Stockpile")).

return system "sim.colony.haul" {
  stage = "sim",
  after = { "sim.colony.sources" },
  reads = {},
  writes = { "Item", "Job" },
  doc = "Posts a haul job for every Item without one: picked up where it lies and carried to @colony.",
  run = function(q)
    local colony = q:entity("@colony")
    for e, item in q:each("Item") do
      if item.job == nil or not item.job:alive() then
        local j = q:spawn({ "Job" })
        local job = j:get("Job")
        job.kind = "haul"
        job.site = e
        job.carry = e
        job.to = colony
        item.job = j
      end
    end
  end,
}
