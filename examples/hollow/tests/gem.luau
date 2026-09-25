-- The gem at the top of the shaft (the phase 2 playtest's mark 6, kept as
-- proposal 1's C): up over the drop past the end of the ledge, where
-- walking off the end misses it and a running jump from the ledge takes
-- it. The HUD counts it, and restart puts it back. Off, it is neither drawn
-- nor taken.
local function from_the_ledge(t, note)
  local tx = t:call("edit.begin", { author = "test:hollow", note = note }).tx
  t:call("edit.set", { tx = tx, entity = "@player", path = "Transform.pos", value = { 36, 19.8 } })
  t:call("edit.commit", { tx = tx })
  t:step(5)
  t.input:hold("move", { 1, 0 }, 70)
end

return scenario "a gem is taken by a jump, counted, and back on restart" {
  scene = "hollow", seed = 1,
  run = function(t)
    t.expect:field("@gem", "Sprite.visible"):eq(true)
    t.expect:field("@gem", "Gem.active"):eq(true)
    -- Walking off the end of the ledge goes under it.
    from_the_ledge(t, "walk off the end")
    t:step(90)
    t.expect:events("gem.taken"):count():eq(0)
    -- Off, the jump takes nothing, and it is not drawn.
    local tx = t:call("edit.begin", { author = "test:hollow", note = "the gem off" }).tx
    t:call("edit.set", { tx = tx, entity = "@gem", path = "Gem.active", value = false })
    t:call("edit.commit", { tx = tx })
    from_the_ledge(t, "jump for nothing")
    t:step(22)
    t.input:hold("jump", true, 25)
    t:step(50)
    t.expect:events("gem.taken"):count():eq(0)
    t.expect:field("@gem", "Sprite.visible"):eq(false)
    -- On: the same jump takes it.
    tx = t:call("edit.begin", { author = "test:hollow", note = "the gem on" }).tx
    t:call("edit.set", { tx = tx, entity = "@gem", path = "Gem.active", value = true })
    t:call("edit.commit", { tx = tx })
    from_the_ledge(t, "jump for it")
    t.expect:field("@gem", "Sprite.visible"):eq(true)
    t:step(22)
    t.input:hold("jump", true, 25)
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
