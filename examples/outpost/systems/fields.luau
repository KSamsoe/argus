-- Fields grow by day, at the day's weather, and a ripe one asks for a
-- farmer: a harvest job (JobKind harvest: skill farm).

return system "sim.colony.fields" {
  stage = "sim",
  after = { "sim.colony.needs" },
  reads = { "Colony" },
  writes = { "Field", "Job" },
  doc = "Each field grows 1 / Colony.grow_days a day of daylight, times the day's growth; a ripe one without a harvest job gets one.",
  run = function(q)
    local rules = q:entity("@colony"):get("Colony")
    local dl = q.day_length
    if (rules.clock / dl) % 1 >= rules.dusk then return end
    local grow = rules.growth * q.dt / (rules.grow_days * dl * rules.dusk)
    for e, f in q:each("Field") do
      if f.growth < 1 then
        f.growth = math.min(1, f.growth + grow)
      elseif f.job == nil or not f.job:alive() then
        local j = q:spawn({ "Job" })
        local job = j:get("Job")
        job.kind = "harvest"
        job.site = e
        f.job = j
      end
    end
  end,
}
