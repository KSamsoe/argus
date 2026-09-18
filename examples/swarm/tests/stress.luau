return scenario "fifty thousand enemies tick" {
  scene = "stress", seed = 1,
  run = function(t)
    t:run_until { event = "wave.spawned", max_frames = 5 }
    t.expect:count({ has = {"Enemy"} }):eq(50000)
    t:run_until { frames = 30 }
    t.expect:count({ has = {"Enemy"} }):gte(49000)
    t.expect:field("@player", "Health.cur"):gte(1)
    -- The Luau ceiling (design section 19): the number itself is the finding.
    t.expect:perf("sim"):p95_ms():lt(300.0)
  end,
}
