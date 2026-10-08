return scenario "the pendulum swings through" {
  scene = "toybox", seed = 1,
  run = function(t)
    -- Let go level with its hinge at x 8: half a swing later it is out
    -- the other side, and always three units from the hinge.
    t:run_until { frames = 90 }
    t.expect:field("@bob", "Transform.pos.x"):lt(6.0)
    t.expect:field("@bob", "Transform.pos.y"):gt(9.0)
  end,
}
