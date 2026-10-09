-- The hero's death ends the run: nothing more happens on any key until the
-- restart action, which makes a new run (the hero whole on the @, four
-- monsters, the turn count back to 0).
return scenario "the hero's death ends the run, and restart begins a new one" {
  scene = "dungeon", seed = 1,
  run = function(t)
    -- One hit point left, and a monster next to the hero.
    local id = t:call("world.query", {
      select = { "id" }, where = { has = { "Monster" } }, order = "dist(@player)", limit = 1,
    }).rows[1][1]
    t:call("sim.set", { entity = id, path = "Transform.pos", value = { 4, 6 } })
    t:call("sim.set", { entity = "@player", path = "Health.cur", value = 1 })

    t.input:tap("wait", true)
    t:advance { by = "turn", n = 1 }
    t.expect:events("player.died"):count():eq(1)
    t.expect:field("@player", "Health.cur"):eq(0)
    t.expect:field("@player", "Player.dead"):eq(true)
    local said = t.expect:field("@messages", "UiText.shown"):get()
    if not string.find(said, "kills you", 1, true) then error("the last message: " .. said, 0) end

    -- Dead, the hero does nothing and no monster acts.
    for _ = 1, 3 do
      t.input:tap("move", { 1, 0 })
      t:advance { by = "turn", n = 1 }
    end
    t.expect:field("@player", "Transform.pos"):eq({ 3, 6 })
    t.expect:field("@turn", "Turn.n"):eq(1)
    t.expect:events("damage"):count():eq(1)

    -- Restart: a new run.
    t.input:tap("restart", true)
    t:advance { by = "turn", n = 1 }
    t.expect:events("run.new"):count():eq(2) -- the scene's first run and this one
    t.expect:field("@player", "Player.dead"):eq(false)
    t.expect:field("@player", "Health.cur"):eq(20)
    t.expect:field("@player", "Transform.pos"):eq({ 3, 6 })
    t.expect:field("@turn", "Turn.n"):eq(0)
    t.expect:count({ has = { "Monster" } }):eq(4)
    t.expect:count({ has = { "Monster" }, within = { of = "@player", r = 8 } }):eq(0)
    t.expect:field("@messages", "UiText.shown"):eq("A new run begins.")
  end,
}
