-- The 2D staples in Toybox: the pill stands on its rounded end, the wedge
-- holds still where it was put, and the tripwire's ray ends at a body.
return scenario "a pill stands, the wedge holds, and the tripwire sees" {
  scene = "toybox", seed = 1,
  run = function(t)
    t:run_until { frames = 240 }
    -- The pill's centre half its height over the floor, where it fell.
    t.expect:field("@pill", "Transform.pos.y"):near(1.0, 0.03)
    t.expect:field("@pill", "Transform.pos.x"):near(15.0, 0.01)
    t.expect:field("@wedge", "Transform.pos.x"):eq(-6.0)
    -- The beam, 0.4 over the floor, ends on the pill's rounded foot:
    -- 0.49 short of its centre at that height, so 9.51 along.
    t.expect:field("@laser", "Laser.distance"):near(9.51, 0.01)
  end,
}
