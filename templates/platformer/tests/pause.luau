-- The pause (systems/pause.luau): the pause action shows the panel and the
-- hero stands still whatever the keys do; pressed again, the panel goes
-- and the game goes on.
local function hero_x(t)
  return t:call("world.get", { entity = "@player" }).components.Transform.pos[1]
end

return scenario "pause stops the game until it is pressed again" {
  scene = "level", seed = 1,
  run = function(t)
    t:step(10)
    t.expect:field("@pause", "UiRect.shown"):eq(false)
    t.input:hold("pause", true, 1)
    t:step(2)
    t.expect:events("pause.opened"):count():eq(1)
    t.expect:field("@pause", "UiRect.shown"):eq(true)

    -- Paused: running does nothing.
    local x = hero_x(t)
    t.input:hold("move", { 1, 0 }, 30)
    t:step(30)
    t.expect:field("@player", "Transform.pos.x"):near(x, 0.001)

    t.input:hold("pause", true, 1)
    t:step(2)
    t.expect:events("pause.closed"):count():eq(1)
    t.expect:field("@pause", "UiRect.shown"):eq(false)
    t.input:hold("move", { 1, 0 }, 30)
    t:step(30)
    t.expect:field("@player", "Transform.pos.x"):gt(x + 1)
  end,
}
