-- Getting hurt: touching an enemy costs health, then the hero is safe for
-- Player.grace ticks; at zero it is game over, everything stands still,
-- and the restart action plays again from wave 1.
local function get(t, id)
  return t:call("world.get", { entity = id }).components
end

return scenario "touching an enemy costs health, at zero it is game over, and restart plays again" {
  scene = "main", seed = 1,
  run = function(t)
    local hero = get(t, "@player")
    local full, grace, range = hero.Health.max, hero.Player.grace, hero.Player.range
    -- The hero never fires (range 0), so the wave reaches it.
    t:call("sim.set", { entity = "@player", path = "Player.range", value = 0 })
    -- run_until stops at the event, or after max_frames without it (which
    -- does not fail the run by itself: the lines after it say what is wrong).
    local reached = t:run_until { event = "player.hurt", max_frames = 1800 }
    if not reached.hit then error("no enemy reached the hero in 30 seconds", 0) end
    -- first() is the event: its frame, and the fields sim.hits gave it.
    local cost = t.expect:events("player.hurt"):first().fields.amount
    if cost < 1 then error("a touch cost " .. cost, 0) end
    t.expect:field("@player", "Health.cur"):eq(full - cost)

    -- The grace: the enemy is still touching, but no second hit until it ends.
    t:step(grace)
    t.expect:events("player.hurt"):count():eq(1)
    t:step(1)
    t.expect:events("player.hurt"):count():eq(2)

    -- On to zero: game over shows, and nothing moves any more.
    t:run_until { event = "player.died", max_frames = 6000 }
    t.expect:field("@player", "Health.cur"):eq(0)
    t.expect:field("@player", "Player.dead"):eq(true)
    t:step(1)
    t.expect:field("@game_over", "UiRect.shown"):eq(true)
    local one = t:call("world.query", { select = { "id", "Transform.pos" }, where = { has = { "Enemy" } }, limit = 1 }).rows[1]
    local x = get(t, "@player").Transform.pos[1]
    t.input:hold("move", { 1, 0 }, 30)
    t:step(30)
    t.expect:field(one[1], "Transform.pos.x"):near(one[2][1], 1e-6)
    t.expect:field("@player", "Transform.pos.x"):near(x, 1e-6)

    -- Restart: the hero whole, no enemies, score 0, and wave 1 again; and
    -- this time it fires (its range back as it was).
    t:call("sim.set", { entity = "@player", path = "Player.range", value = range })
    t.input:hold("restart", true, 1)
    t:step(2)
    t.expect:events("game.restarted"):count():eq(1)
    t.expect:field("@player", "Health.cur"):eq(full)
    t.expect:field("@player", "Player.score"):eq(0)
    t.expect:count({ has = { "Enemy" } }):eq(0)
    t.expect:field("@game_over", "UiRect.shown"):eq(false)
    t:run_until { event = "wave.spawned", max_frames = 600 }
    t.expect:events("wave.spawned", { wave = 1 }):count():eq(2)
    t:run_until { event = "enemy.killed", max_frames = 1200 }
    t.expect:field("@player", "Player.score"):gte(1)
  end,
}
