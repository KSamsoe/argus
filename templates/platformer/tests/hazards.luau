-- The pit and the spikes send the hero back to the start (systems/hazards.luau),
-- each counted as a fall; spikes switched off (Hazard.active false) are
-- harmless.
local function put(t, x, y)
  t:call("sim.set", { entity = "@player", path = "Transform.pos", value = { x, y } })
end

return scenario "the pit and the spikes send the hero back to the start" {
  scene = "level", seed = 1,
  run = function(t)
    -- Over the pit: it falls in, and is put back at the start.
    put(t, 17.5, 2)
    t:run_until { event = "hero.hurt", max_frames = 120 }
    t.expect:events("hero.hurt", { cause = "pit" }):count():eq(1)
    t.expect:field("@player", "Transform.pos.x"):near(3, 0.01)
    t.expect:field("@player", "Hero.falls"):eq(1)

    -- Before the spikes, running into them.
    put(t, 23, 0.6)
    t.input:hold("move", { 1, 0 }, 60)
    t:run_until { event = "hero.hurt", max_frames = 120 }
    t.expect:events("hero.hurt", { cause = "hazard" }):count():eq(1)
    t.expect:field("@player", "Transform.pos.x"):near(3, 0.01)
    t.expect:field("@player", "Hero.falls"):eq(2)
    t.expect:field("@hud", "UiText.text"):eq("Coins 0/3    Falls 2")

    -- Switched off, the same run goes over them.
    t:call("sim.set", { entity = "@spikes", path = "Hazard.active", value = false })
    put(t, 23, 0.6)
    t.input:hold("move", { 1, 0 }, 60)
    t:step(60)
    t.expect:events("hero.hurt"):count():eq(2)
    t.expect:field("@player", "Transform.pos.x"):gt(27)
  end,
}
