-- A scenario: the game run headless with scripted input, and what must be
-- true after. `argus test` runs every file in tests/.
return scenario "the player walks right" {
  scene = "main", seed = 1,
  run = function(t)
    t.input:hold("move", { 1, 0 }, 60)
    t:run_until { frames = 60 }
    -- Six units a second for one second.
    t.expect:field("@player", "Transform.pos.x"):gt(5.5)
    t.expect:field("@player", "Transform.pos.y"):eq(0.0)
  end,
}
