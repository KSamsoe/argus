-- A gem: off, it is neither drawn nor taken; on, the hero takes it by
-- touch, the HUD says so, and restart puts it back.
return scenario "a gem is taken on touch, counted, and back on restart" {
  scene = "hollow", seed = 1,
  run = function(t)
    t.expect:field("@gem", "Sprite.visible"):eq(false)
    local tx = t:call("edit.begin", { author = "test:hollow", note = "beside the gem" }).tx
    t:call("edit.set", { tx = tx, entity = "@player", path = "Transform.pos", value = { 35.5, 19.8 } })
    t:call("edit.commit", { tx = tx })
    t.input:hold("move", { 1, 0 }, 40)
    t:step(40)
    t.expect:events("gem.taken"):count():eq(0)
    -- On, where it is: walk back through it.
    tx = t:call("edit.begin", { author = "test:hollow", note = "the gem on" }).tx
    t:call("edit.set", { tx = tx, entity = "@gem", path = "Gem.active", value = true })
    t:call("edit.set", { tx = tx, entity = "@player", path = "Transform.pos", value = { 35.5, 19.8 } })
    t:call("edit.commit", { tx = tx })
    t:step(1)
    t.expect:field("@gem", "Sprite.visible"):eq(true)
    t.input:hold("move", { 1, 0 }, 40)
    t:run_until { event = "gem.taken", max_frames = 60 }
    t.expect:events("gem.taken"):count():eq(1)
    t.expect:events("sound.played", { sound = "sfx/gem" }):count():eq(1)
    t.expect:field("@hud", "Run.gems"):eq(1)
    t.expect:field("@gem", "Sprite.visible"):eq(false)
    t.input:hold("restart", true, 2)
    t:step(3)
    t.expect:field("@gem", "Gem.taken"):eq(false)
    t.expect:field("@gem", "Sprite.visible"):eq(true)
    t.expect:field("@hud", "Run.gems"):eq(0)
  end,
}
