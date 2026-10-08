return scenario "balls land on the bridge and it holds" {
  scene = "toybox", seed = 1,
  run = function(t)
    t:run_until { frames = 600 }
    t.expect:events("ball.dropped"):count():gte(6)
    t.expect:events("contact.begin"):count():gte(6)
    t.expect:count({ has = {"Ball"} }):gte(1)
    -- Pinned at both ends: the middle sags, and holds.
    t.expect:field("@plank.2", "Transform.pos.y"):gt(2.5)
    t.expect:field("@plank.2", "Transform.pos.y"):lt(4.1)
  end,
}
