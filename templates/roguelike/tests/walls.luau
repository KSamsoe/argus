-- A scenario: the game run headless with scripted input, then what must be
-- true. `argus test` runs every file in tests/. On the discrete clock
-- t.input:tap sets an action for the next turn and t:advance takes it.
--
-- The hero starts on the @ of the map (systems/dungeon.luau), at (3, 6) in
-- the world; three cells up is the room's top row, and above it rock.
local function at(t)
  local p = t:call("world.get", { entity = "@player" }).components.Transform.pos
  return p[1], p[2]
end

return scenario "rock blocks the hero, and bumping it costs no turn" {
  scene = "dungeon", seed = 1,
  run = function(t)
    for _ = 1, 3 do
      t.input:tap("move", { 0, 1 })
      t:advance { by = "turn", n = 1 }
    end
    local x, y = at(t)
    if x ~= 3 or y ~= 9 then error(string.format("three steps up from (3, 6) should be (3, 9), not (%s, %s)", x, y), 0) end
    t.expect:field("@turn", "Turn.n"):eq(3)

    -- A monster in the same row, five cells away: it sees the hero and
    -- would step closer on any turn that passes.
    local id = t:call("world.query", { select = { "id" }, where = { has = { "Monster" } }, limit = 1 }).rows[1][1]
    t:call("sim.set", { entity = id, path = "Transform.pos", value = { 8, 9 } })

    -- One more step up is rock: the hero stays, and no turn passes, so
    -- the monster does not move either.
    t.input:tap("move", { 0, 1 })
    t:advance { by = "turn", n = 1 }
    x, y = at(t)
    if x ~= 3 or y ~= 9 then error(string.format("the hero walked into rock, to (%s, %s)", x, y), 0) end
    t.expect:events("bump"):count():eq(1)
    t.expect:field("@turn", "Turn.n"):eq(3)
    t.expect:field(id, "Transform.pos"):eq({ 8, 9 })
    t.expect:field("@messages", "UiText.shown"):eq("A wall.")

    -- A wait does pass a turn: now the monster steps toward the hero.
    t.input:tap("wait", true)
    t:advance { by = "turn", n = 1 }
    t.expect:field("@turn", "Turn.n"):eq(4)
    t.expect:field(id, "Transform.pos"):eq({ 7, 9 })
  end,
}
