-- Scenarios: the game run headless with scripted input, and what must be
-- true after. `argus test` runs every file in tests/; each seed is a fresh
-- load of the scene. These read the numbers (arena size, wave size, speeds)
-- from the world, so tuning them keeps the tests green while the rules
-- hold. When you change a rule, change its test; add one for each new
-- thing the game must keep doing.

-- An entity's components now, as a table: get(t, "@arena").Waves.first.
local function get(t, id)
  return t:call("world.get", { entity = id }).components
end

-- Every enemy's id and position now, as rows: { "e:12", { x, y } }.
local function enemies(t)
  return t:call("world.query", { select = { "id", "Transform.pos" }, where = { has = { "Enemy" } }, limit = 1000 }).rows
end

local function dist(a, b)
  return math.sqrt((a[1] - b[1]) ^ 2 + (a[2] - b[2]) ^ 2)
end

return scenario "wave 1 comes in from the edges and closes in on a hero who holds fire" {
  scene = "main", seeds = range(1, 3),
  run = function(t)
    -- A range of 0: the hero never fires, so every enemy lives to arrive.
    -- sim.set changes the running world only, never the files.
    t:call("sim.set", { entity = "@player", path = "Player.range", value = 0 })
    local arena = get(t, "@arena")
    local centre = arena.Transform.pos
    local half = { arena.Sprite.size[1] / 2, arena.Sprite.size[2] / 2 }
    local w = arena.Waves

    t:run_until { event = "wave.spawned", max_frames = 600 }
    t.expect:events("wave.spawned", { wave = 1, count = w.first }):count():eq(1)
    t.expect:count({ has = { "Enemy" } }):eq(w.first)

    -- Each one is on the arena's edge (it has walked one tick since).
    local hero = get(t, "@player").Transform.pos
    local start = {}
    for _, row in ipairs(enemies(t)) do
      local x, y = row[2][1] - centre[1], row[2][2] - centre[2]
      local on_edge = math.abs(math.abs(x) - half[1]) < 0.1 or math.abs(math.abs(y) - half[2]) < 0.1
      if not on_edge then
        error(string.format("%s came in at (%.2f, %.2f), not on the edge", row[1], x, y), 0)
      end
      start[row[1]] = dist(row[2], hero)
    end

    -- A second later, every one is nearer the hero, by at least what the
    -- slowest enemy walks in a second.
    t:step(60)
    for _, row in ipairs(enemies(t)) do
      local now = dist(row[2], hero)
      if now > start[row[1]] - (w.slowest - 0.01) then
        error(string.format("%s went from %.2f to %.2f from the hero", row[1], start[row[1]], now), 0)
      end
    end
  end,
}
