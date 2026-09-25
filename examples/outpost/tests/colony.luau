-- Outpost's core at work (phase 3 item 3): jobs claimed and done, the
-- economy as aggregates, why a colonist is idle, day and night, and the
-- speed and pause controls.

scenario "colonists harvest, haul, cook, eat, and sleep" {
  scene = "valley", seed = 3,
  run = function(t)
    t:run_until { sim_time = days(2) + 1 }
    for _, kind in ipairs({ "harvest", "haul", "cook", "eat", "sleep" }) do
      t.expect:events("job.claimed", { kind = kind }):exists()
    end
    for _, kind in ipairs({ "harvest", "haul", "cook", "eat" }) do
      t.expect:events("job.done", { kind = kind }):exists()
    end
    t.expect:events("colony.stored", { kind = "item:crop" }):exists()
    t.expect:events("colony.cooked"):exists()
    -- Finn has no skills: he hauls and eats, and never cooks or harvests.
    local finn = t:call("world.query", { where = { ["Colonist.name"] = "Finn" } }).rows[1][1]
    t.expect:events("job.claimed", { kind = "cook", worker = finn }):count():eq(0)
    t.expect:events("job.claimed", { kind = "harvest", worker = finn }):count():eq(0)
    t.expect:events("job.claimed", { kind = "haul", worker = finn }):exists()
    -- The economy as aggregates (section 8: "is the economy drifting").
    local q = t:call("world.query", { select = { "sum(Stockpile.food)", "sum(Stockpile.meal)", "sum(Stockpile.crop)" } })
    t.expect:sum("Stockpile.food"):gte(1)
    t:log(string.format("day 2: food %.1f, meals %.1f, crops %.1f", q.rows[1][1], q.rows[1][2], q.rows[1][3]))
    local jobs = t:call("world.query", { group_by = "Job.kind", select = { "count()" } })
    t:log("jobs by kind: " .. #jobs.rows .. " kinds")
    local needs = t:call("world.query", { select = { "mean(Needs.fed)", "min(Needs.fed)", "mean(Needs.rest)" } })
    t:log(string.format("fed on average %.2f, least %.2f; rest %.2f", needs.rows[1][1], needs.rows[1][2], needs.rows[1][3]))
    -- Night: by its end everyone is asleep at their bed (some eat first).
    t:run_until { sim_time = days(3) - 1 }
    t.expect:count({ has = { "Job" }, ["Job.kind"] = "sleep", ["Job.stage"] = "working" }):eq(6)
    -- Morning: the sleep jobs are over.
    t:run_until { sim_time = days(3) + 1 }
    t.expect:count({ has = { "Job" }, ["Job.kind"] = "sleep" }):eq(0)
  end,
}

scenario "why a colonist is idle" {
  scene = "valley", seed = 1,
  run = function(t)
    -- Finn hauls and nothing else: early on, with nothing lying about, the
    -- cook and harvest jobs are not his to do.
    t:run_until { event = "job.idle", max_ticks = 3600 }
    local why = t:call("obs.why", { name = "job.idle" })
    t.expect:field(why.event.fields.worker, "Worker.idle"):eq(why.event.fields.why)
    local j = why.jobs
    if j == nil then error("obs.why on job.idle carries the worker's jobs", 0) end
    t:log(why.event.fields.why)
    local rows = t:call("jobs.why", { worker = why.event.fields.worker })
    if rows.columns[6] ~= "verdict" then error("jobs.why has a verdict column", 0) end
    t:log(#rows.rows .. " jobs judged")
    -- The job system emits it: obs.why names it.
    local found = false
    for _, r in ipairs(why.rows) do
      if r[1] == "jobs.update" and r[4] == "emitted it" then found = true end
    end
    if not found then error("obs.why names jobs.update as the emitter", 0) end
  end,
}

return scenario "speed and pause" {
  scene = "valley", seed = 1,
  run = function(t)
    t:step(10)
    t.input:tap("speed_10")
    t:step(2)
    t.expect:field("@colony", "Colony.speed"):eq(10)
    if t:status().speed ~= 10 then error("speed_10 set the clock's speed: " .. tostring(t:status().speed), 0) end
    t.input:tap("speed_1")
    t:step(2)
    if t:status().speed ~= 1 then error("speed_1 set it back", 0) end
    -- Paused, the colony's clock stands still though ticks go on.
    t.input:tap("pause")
    t:step(2)
    t.expect:field("@colony", "Colony.paused"):eq(true)
    local clock = t.expect:field("@colony", "Colony.clock"):get()
    t:step(120)
    t.expect:field("@colony", "Colony.clock"):eq(clock)
    t.input:tap("pause")
    t:step(60)
    t.expect:field("@colony", "Colony.clock"):gt(clock)
  end,
}
