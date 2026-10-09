-- The player's clock: pause stands the colony still (its clock, and the
-- colonists mid-walk), and the speed action steps through 1x, 3x, and 10x.
local function hud(t)
  return t:call("world.get", { entity = "@hud" }).components.UiText.text
end

return scenario "pause stands the colony still, and speed steps through 1x, 3x, 10x" {
  scene = "main", seed = 1,
  run = function(t)
    t:step(30)
    t.input:tap("pause")
    t:step(2)
    t.expect:field("@colony", "Colony.paused"):eq(true)
    local clock = t.expect:field("@colony", "Colony.clock"):get()
    local x = t.expect:field("@colonist.ada", "Transform.pos.x"):get()
    t:step(120)
    t.expect:field("@colony", "Colony.clock"):eq(clock)
    t.expect:field("@colonist.ada", "Transform.pos.x"):eq(x)
    if not string.find(hud(t), "paused", 1, true) then error("the HUD says paused: " .. hud(t), 0) end
    t.input:tap("pause")
    t:step(2)
    t.expect:field("@colony", "Colony.clock"):gt(clock)
    for _, want in ipairs({ 3, 10, 1 }) do
      t.input:tap("speed")
      t:step(2)
      if t:status().speed ~= want then
        error("speed steps to " .. want .. "x, not " .. tostring(t:status().speed), 0)
      end
    end
  end,
}
