return scenario "first wave clears within 20s" {
  scene = "arena", seed = 7,
  run = function(t)
    t.input:hold("move", {1, 0}, 60)          -- walk right for a second
    t:run_until { event = "wave.cleared", max_frames = 1200 }
    t.expect:count({ has = {"Enemy"} }):eq(0)
    t.expect:field("@player", "Health.cur"):gte(1)
    t.expect:events("death", { cause = "bullet" }):count():eq(200)
    t.expect:events("wave.spawned"):count():eq(1)
    t.expect:field("@player", "Player.kills"):eq(200)
    t.expect:snapshot("wave1_end", { select = {"Transform.pos", "Health.cur", "Player.kills"}, where = { has = {"Player"} } })
    t.expect:perf("sim"):p95_ms():lt(4.0)
  end,
}
