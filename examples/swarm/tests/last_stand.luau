return scenario "a standing player dies and the log says why" {
  scene = "last_stand", seeds = range(1, 3),
  run = function(t)
    t:run_until { event = "player.died", max_frames = 3000 }
    t.expect:events("player.died"):count():eq(1)
    t.expect:field("@player", "Health.cur"):eq(0)
    t.expect:field("@player", "Player.dead"):eq(true)
    -- Why: every hit on the player is an event naming the enemy, the amount, the wave, the tick.
    t.expect:events("damage", { target = "@player" }):count():gte(4)
    t.expect:events("damage", { target = "@player", amount = 3 }):count():gte(1)
  end,
}
