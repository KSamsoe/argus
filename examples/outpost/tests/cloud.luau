-- The colony in the cloud (phase 5 item 9): the day's autosave goes to the
-- platform's cloud as well as the disk (the daemon's null platform keeps
-- its cloud in .argus/platform/cloud), and friends see the day.
return scenario "the day's autosave goes to the cloud and friends see the day" {
  scene = "valley", seed = 1,
  run = function(t)
    t:call("platform.reset", { cloud = true })
    t:run_until { sim_time = 61 }
    t.expect:events("game.saved", { name = "auto", cloud = true }):count():eq(1)
    local s = t:call("platform.status", {})
    local found = false
    for _, f in ipairs(s.state.cloud.files) do
      if f.name == "auto.save" and f.size > 0 then
        found = true
      end
    end
    if not found then
      error("the cloud has no auto.save", 0)
    end
    if s.state.presence.status ~= "Day 1 in the valley" then
      error("friends see: " .. tostring(s.state.presence.status), 0)
    end
    -- A load takes the newer copy: the disk's, saved the same second.
    local l = t:call("sim.load", { name = "auto" })
    if l.from ~= "disk" then
      error("loaded from " .. tostring(l.from), 0)
    end
  end,
}
