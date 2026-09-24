-- A sign's words show while the hero is near it, and only then.
return scenario "a sign speaks while the hero is near it" {
  scene = "hollow", seed = 1,
  run = function(t)
    t:step(2)
    t.expect:field("@c/sign", "Describe.shown"):eq(false)
    t.expect:field("@hud", "Describe.shown"):eq(true)
    -- Beside the hall's sign, before the pit.
    local tx = t:call("edit.begin", { author = "test:hollow", note = "by the sign" }).tx
    t:call("edit.set", { tx = tx, entity = "@player", path = "Transform.pos", value = { 50.5, 1.8 } })
    t:call("edit.commit", { tx = tx })
    t:step(2)
    t.expect:field("@c/sign", "Describe.shown"):eq(true)
    t.expect:field("@b/sign", "Describe.shown"):eq(false)
  end,
}
