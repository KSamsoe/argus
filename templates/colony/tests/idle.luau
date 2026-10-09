-- A colonist with nothing to do idles: no job, a reason why (Worker.idle,
-- the job.idle event, `argus events why=job.idle`), and they wait about
-- the stockpile (systems/idle.luau). Once a job is posted, one of them
-- takes it and sees it through.
return scenario "with nothing to do colonists idle, and take a job once one is posted" {
  scene = "main", seed = 1,
  run = function(t)
    -- Nothing ripe: every bush and tree as if just worked.
    local sources = t:call("world.query", { where = { has = { "Source" } }, limit = 100 })
    for _, row in ipairs(sources.rows) do
      t:call("sim.set", { entity = row[1], path = "Source.growth", value = 0 })
    end
    t:run_until { sim_time = seconds(10) }
    t.expect:count({ has = { "Job" } }):eq(0)
    t.expect:count({ has = { "Worker" }, ["Worker.idle"] = "no jobs are posted" }):eq(4)
    t.expect:events("job.idle", { why = "no jobs are posted" }):count():eq(4)
    t.expect:count({ has = { "Colonist" }, within = { of = "@colony", r = 5 } }):eq(4)
    -- One bush ripens: it posts a gather job, one colonist takes it, and
    -- the berries reach the stockpile.
    t:call("sim.set", { entity = "@bush.4", path = "Source.growth", value = 1 })
    t:step(5)
    t.expect:events("job.claimed", { kind = "gather" }):count():eq(1)
    local hit = t:run_until { event = "colony.stored", max_frames = 1800 }
    if not hit.hit then error("the berries were stored within 30 seconds", 0) end
    t.expect:events("job.done", { kind = "gather" }):count():eq(1)
    t.expect:events("job.done", { kind = "haul" }):count():eq(1)
  end,
}
