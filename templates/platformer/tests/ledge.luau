-- The ledge (scenes/level/ledge.toml) is a wall to walk into and a floor to
-- jump up to: running at it stops the hero at its side, and a jump from
-- there lands the hero on its top, 2.5 up. t:call("sim.set", ...) puts the
-- hero somewhere for the test without changing any file.
return scenario "the ledge is too high to walk up, and a jump lands on it" {
  scene = "level", seed = 1,
  run = function(t)
    t:call("sim.set", { entity = "@player", path = "Transform.pos", value = { 32, 0.6 } })
    t:step(5)
    t.input:hold("move", { 1, 0 })
    t:step(60)
    -- Stopped by its side (x 36, less half the hero's width), on the ground.
    t.expect:field("@player", "Transform.pos.x"):near(35.6, 0.01)
    t.expect:field("@player", "Transform.pos.y"):near(0.6, 0.01)
    t.expect:field("@player", "KinematicBody.wall"):eq(1)

    t.input:hold("jump", true, 20)
    t:step(2)
    t:run_until { event = "body.landed", max_frames = 120 }
    t.expect:events("body.jumped"):count():eq(1)
    t.expect:field("@player", "Transform.pos.x"):gt(36)
    t.expect:field("@player", "Transform.pos.y"):near(3.1, 0.01)
    t.expect:field("@player", "KinematicBody.ground"):eq("@ledge")
  end,
}
