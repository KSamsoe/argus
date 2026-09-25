-- The hall's pit puts the hero back at the cave mouth, and says so.
return scenario "the pit puts the hero back at the cave mouth" {
  scene = "hollow", seed = 1,
  run = function(t)
    local tx = t:call("edit.begin", { author = "test:hollow", note = "over the pit" }).tx
    t:call("edit.set", { tx = tx, entity = "@player", path = "Transform.pos", value = { 58, 3 } })
    t:call("edit.commit", { tx = tx })
    t:run_until { event = "player.fell", max_frames = 120 }
    t.expect:events("player.fell"):count():eq(1)
    t.expect:events("sound.played", { sound = "sfx/fell" }):count():eq(1)
    t.expect:field("@player", "Transform.pos.x"):near(3, 0.01)
    t.expect:field("@hud", "Run.falls"):eq(1)
  end,
}
