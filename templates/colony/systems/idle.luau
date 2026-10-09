-- What a colonist does with nothing to do: they wait about the stockpile,
-- strolling now and then to an open cell near it. The job system leaves a
-- worker without a job alone (Worker.job is nil, and Worker.idle says
-- why), so the game decides what idling looks like; here a stroll, drawn
-- from the seed, so a run replays the same.
--
-- To change first: how far they stroll (REACH) and how often (EVERY).

local REACH = 3   -- cells from the stockpile, each way
local EVERY = 4   -- seconds between strolls, on average

return system "sim.colony.idle" {
  stage = "sim",
  after = { "jobs.update" },
  rng = "strolls",
  reads = { "Colonist", "Worker", "Transform", "NavGrid" },
  writes = { "NavAgent" },
  doc = "A colonist with no job, standing still, strolls now and then (about every 4 seconds, drawn from the seed) to an open cell within 3 cells of the stockpile (q:walkable); EVERY and REACH in systems/idle.luau.",
  run = function(q)
    local home = q:entity("@colony"):get("Transform").pos
    local rng = q:rng()
    for _, _, w, agent in q:each("Colonist", "Worker", "NavAgent") do
      if w.job == nil and agent.state ~= "moving" and rng:chance(q.dt / EVERY) then
        local to = home + vector.create(rng:range(-REACH, REACH), rng:range(-REACH, REACH), 0)
        if q:walkable(to) then agent.goal = to end
      end
    end
  end,
}
