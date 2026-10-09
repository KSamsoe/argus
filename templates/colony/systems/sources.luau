-- Bushes and trees: a Source grows back over Source.regrow_days once it
-- has been worked, and a ripe one asks for a worker. Asking is spawning a
-- Job entity (an engine component): its kind names a JobKind in
-- scenes/main/jobs.toml, which says its priority, the skill it needs, and
-- how long the work takes; the job system does the rest.

return system "sim.colony.sources" {
  stage = "sim",
  after = { "sim.colony.needs" },
  reads = {},
  writes = { "Source", "Sprite", "Job" },
  doc = "Grows every worked Source back (1 / Source.regrow_days a day), colouring it Source.ripe_color when it is ripe again; a ripe Source without a job posts one of its Source.job_kind at it.",
  run = function(q)
    for e, src, sprite in q:each("Source", "Sprite") do
      if src.growth < 1 then
        src.growth = math.min(1, src.growth + q.dt / (src.regrow_days * q.day_length))
        if src.growth >= 1 then sprite.color = src.ripe_color end
      elseif src.job == nil or not src.job:alive() then
        local j = q:spawn({ "Job" })
        local job = j:get("Job")
        job.kind = src.job_kind
        job.site = e
        src.job = j
      end
    end
  end,
}
