return scenario "stairs are always reachable on floor 1" {
  scene = "delve", seeds = range(1, 50),
  run = function(t)
    t:advance { by = "turn", n = 1 }                  -- generate floor
    t.expect:events("floor.generated", { floor = 1 }):count():eq(1)
    t.expect:count({ has = {"Stairs"} }):eq(1)
    t.expect:path("@player", { has = {"Stairs"} }):exists()
    t.expect:count({ has = {"Monster"}, within = { of = "@player", r = 3 } }):eq(0)
    t.expect:count({ has = {"Monster"} }):gte(4)
    t.expect:field("@player", "Player.floor"):eq(1)
  end,
}
