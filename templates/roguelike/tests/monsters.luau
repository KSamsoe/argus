-- The monsters act after the hero: a monster that sees the hero steps one
-- cell toward it each turn, and bites on the very turn the hero comes next
-- to it (it saw where the hero stepped to). The scenario moves one monster
-- into the hero's room with sim.set, so the walk is the same on every seed.
local function pos(t, id)
  local p = t:call("world.get", { entity = id }).components.Transform.pos
  return p[1], p[2]
end

local function expect_at(t, id, x, y, what)
  local ax, ay = pos(t, id)
  if ax ~= x or ay ~= y then
    error(string.format("%s: %s should be at (%d, %d), not (%s, %s)", what, id, x, y, ax, ay), 2)
  end
end

return scenario "a monster steps toward the hero after the hero's turn, then bites" {
  scene = "dungeon", seeds = range(1, 3),
  run = function(t)
    -- The nearest monster, put five cells right of the hero (3, 6).
    local m = t:call("world.query", {
      select = { "id", "Monster.atk" }, where = { has = { "Monster" } },
      order = "dist(@player)", limit = 1,
    }).rows[1]
    local id, atk = m[1], m[2]
    t:call("sim.set", { entity = id, path = "Transform.pos", value = { 8, 6 } })

    -- The hero waits: the monster steps one cell closer.
    t.input:tap("wait", true)
    t:advance { by = "turn", n = 1 }
    expect_at(t, id, 7, 6, "after a wait")

    -- The hero steps right, then the monster steps left: two cells apart.
    t.input:tap("move", { 1, 0 })
    t:advance { by = "turn", n = 1 }
    expect_at(t, "@player", 4, 6, "the hero's step")
    expect_at(t, id, 6, 6, "the monster's step")
    t.expect:events("damage", { target = "@player" }):count():eq(0)

    -- The hero steps next to it: the monster bites this same turn and
    -- stays where it is.
    t.input:tap("move", { 1, 0 })
    t:advance { by = "turn", n = 1 }
    expect_at(t, "@player", 5, 6, "the hero's second step")
    expect_at(t, id, 6, 6, "the biting monster")
    t.expect:events("damage", { target = "@player" }):count():eq(1)
    t.expect:field("@player", "Health.cur"):eq(20 - atk)
  end,
}
