-- A one-way plank: jumped up through from below, stood on, and dropped
-- through by holding down and pressing jump.
return scenario "a plank is jumped through, stood on, and dropped through" {
  scene = "hollow", seed = 1,
  run = function(t)
    -- Under the cave's plank: x 8 to 12, its top at y 4.
    local tx = t:call("edit.begin", { author = "test:hollow", note = "under the plank" }).tx
    t:call("edit.set", { tx = tx, entity = "@player", path = "Transform.pos", value = { 10, 1.8 } })
    t:call("edit.commit", { tx = tx })
    t:step(10)
    t.input:hold("jump", true, 20)
    t:step(60)
    t.expect:field("@player", "KinematicBody.on_one_way"):eq(true)
    t.expect:field("@player", "Transform.pos.y"):near(4.8, 0.01)
    t.input:hold("move", { 0, -1 }, 20)
    t.input:hold("jump", true, 5)
    t:step(60)
    t.expect:field("@player", "Transform.pos.y"):near(1.8, 0.01)
    t.expect:field("@player", "KinematicBody.on_one_way"):eq(false)
  end,
}
