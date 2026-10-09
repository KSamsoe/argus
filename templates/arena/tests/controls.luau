-- The controls: the hero walks any of eight ways at one speed, stops at
-- the arena's wall, and the pause action freezes the game until pressed
-- again.
local function get(t, id)
  return t:call("world.get", { entity = id }).components
end

return scenario "the hero walks diagonally at full speed, stops at the wall, and pause freezes the game" {
  scene = "main", seed = 1,
  run = function(t)
    local hero = get(t, "@player")
    local arena = get(t, "@arena")
    local speed = hero.Player.speed
    local start = hero.Transform.pos

    -- Up and right for half a second, as W and D together give it: the
    -- same speed as straight, so 0.7071 of it along each axis.
    t.input:hold("move", { 0.70710678, 0.70710678 }, 30)
    t:step(30)
    local along = speed * 0.5 * 0.70710678
    t.expect:field("@player", "Transform.pos.x"):near(start[1] + along, 0.01)
    t.expect:field("@player", "Transform.pos.y"):near(start[2] + along, 0.01)

    -- Right until well past the wall: it stops half its size inside the
    -- arena's edge.
    local wall = arena.Transform.pos[1] + arena.Sprite.size[1] / 2 - hero.Sprite.size[1] / 2
    local ticks = math.ceil((wall - start[1]) / speed * 60) + 30
    t.input:hold("move", { 1, 0 }, ticks)
    t:step(ticks)
    t.expect:field("@player", "Transform.pos.x"):near(wall, 1e-4)

    -- Pause: walking does nothing, and no enemy moves.
    if t.expect:count({ has = { "Enemy" } }):get() == 0 then
      t:run_until { event = "wave.spawned", max_frames = 600 }
    end
    t.input:hold("pause", true, 1)
    t:step(2)
    t.expect:field("@paused", "UiRect.shown"):eq(true)
    local one = t:call("world.query", { select = { "id", "Transform.pos" }, where = { has = { "Enemy" } }, limit = 1 }).rows[1]
    t.input:hold("move", { -1, 0 }, 30)
    t:step(30)
    t.expect:field("@player", "Transform.pos.x"):near(wall, 1e-4)
    t.expect:field(one[1], "Transform.pos.x"):near(one[2][1], 1e-6)
    t.expect:field(one[1], "Transform.pos.y"):near(one[2][2], 1e-6)

    -- Pause again: the game goes on.
    t.input:hold("pause", true, 1)
    t:step(2)
    t.expect:field("@paused", "UiRect.shown"):eq(false)
    t.input:hold("move", { -1, 0 }, 30)
    t:step(30)
    t.expect:field("@player", "Transform.pos.x"):lt(wall - speed * 0.4)
  end,
}
