-- The gem is back after a restart made deep in the cave (found at the
-- phase 5 gate by the agent who added the feather): the restart puts back
-- the gems whose rooms are in, but the shaft streams out in the deep and
-- remembered its gem as taken, so the new run found it gone.
local function set(t, value)
  local tx = t:call("edit.begin", { author = "test:hollow", note = "move the hero" }).tx
  t:call("edit.set", { tx = tx, entity = "@player", path = "Transform.pos", value = value })
  t:call("edit.commit", { tx = tx })
end

local function loaded(t, name)
  for _, x in ipairs(t:call("scene.rooms", {}).rooms) do
    if x.name == name then return x.loaded end
  end
  return nil
end

return scenario "the gem is back after a restart made while its room is out" {
  scene = "hollow", seed = 1,
  run = function(t)
    -- Taken, as tests/gem.luau takes it: a running jump from the ledge.
    set(t, { 36, 19.8 })
    t:step(5)
    t.input:hold("move", { 1, 0 }, 70)
    t:step(22)
    t.input:hold("jump", true, 25)
    t:step(50)
    t.expect:events("gem.taken"):count():eq(1)
    -- Deep in the cave, the shaft is out.
    set(t, { -6 * 24 + 5, -14 * 3 + 1.8 })
    t:step(30)
    if loaded(t, "b") then error("the shaft should be out in the deep", 0) end
    -- A new run from there, and back up to the gem.
    t.input:hold("restart", true, 2)
    t:step(10)
    set(t, { 36, 19.8 })
    t:step(10)
    if not loaded(t, "b") then error("the shaft should be in again", 0) end
    t.expect:field("@gem", "Gem.taken"):eq(false)
    t.expect:field("@gem", "Sprite.visible"):eq(true)
    t.expect:field("@hud", "Run.gems"):eq(0)
  end,
}
