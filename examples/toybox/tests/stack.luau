return scenario "the stack settles and falls asleep" {
  scene = "toybox", seed = 1,
  run = function(t)
    t:run_until { frames = 240 }
    -- Five crates of one unit, the top one's centre at 4.5.
    t.expect:field("@crate.4", "Transform.pos.y"):gte(4.4)
    t.expect:field("@crate.4", "Transform.pos.y"):lte(4.55)
    t.expect:field("@crate.4", "Transform.pos.x"):gte(-10.05)
    t.expect:field("@crate.4", "Transform.pos.x"):lte(-9.95)
    t.expect:field("@crate.0", "RigidBody.sleeping"):eq(true)
    t.expect:field("@ground", "Transform.pos.y"):eq(-1.0)
    t.expect:perf("sim"):p95_ms():lt(8.0)
  end,
}
