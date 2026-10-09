-- The dungeon, drawn by hand: one character a cell, the top row first.
--   #  rock (it blocks)    .  open floor    @  where the hero starts
-- Change the picture to change the dungeon. Keep every row the same
-- length, and when the size changes, change maps/dungeon.tilemap.toml's
-- width and height and the Transform in scenes/dungeon/dungeon.toml.
local MAP = {
  "##############################",
  "#........#####################",
  "#........#####################",
  "#........#########..........##",
  "#..@...........###..........##",
  "#........#####.###...##.....##",
  "#........#####.###...##.....##",
  "#........#####..............##",
  "#........#########..........##",
  "##################..........##",
  "##############################",
}

-- The kinds of monster. Each monster placed is one of these, rolled from
-- the seed. Add a line to add a kind; the messages use its name.
local KINDS = {
  { kind = "rat", hp = 3, atk = 1, color = "#C2743F" },
  { kind = "goblin", hp = 6, atk = 2, color = "#7FBF5A" },
}
local MONSTERS = 4 -- how many each run
local SAFE = 12 -- monsters start more than this many cells from the hero (across plus up or down)

return system "sim.dungeon" {
  stage = "sim",
  on_load = true, -- the first run's dungeon is there before the first key
  reads = { "Dungeon", "Actions", "Monster", "Tilemap" },
  writes = { "Dungeon", "Player", "Health", "Transform", "Monster", "Sprite", "Describe", "Tilemap", "Turn" },
  -- Its own random stream, seeded from the scene's seed: the same seed
  -- places the same monsters, and nothing else's rolls change it.
  rng = "dungeon",
  doc = "Makes a run when there is none or on the restart action: paints the map picture on @dungeon's tilemap, puts the hero on the @ at full health, and places MONSTERS monsters of random KINDS on random open cells more than SAFE cells from the hero. A restart takes no turn.",
  run = function(q)
    local de = q:entity("@dungeon")
    local d = de:get("Dungeon")
    local pe = q:entity("@player")
    local restart = d.runs > 0 and pe:get("Actions").restart_pressed
    if d.runs > 0 and not restart then return end

    -- The last run's monsters go (collect first, then despawn).
    local old = {}
    for e in q:each("Monster") do table.insert(old, e:id()) end
    for _, id in ipairs(old) do q:entity(id):despawn() end

    -- The picture onto the map: rock on the walls layer, floor tiles on
    -- the floor layer. The map's cells count from 0, rows down.
    local map = q:tiles(de)
    map:paint("walls", 0, 0, MAP, { ["#"] = 1, ["."] = 0, ["@"] = 0 })
    map:paint("floor", 0, 0, MAP, { ["#"] = 0, ["."] = 1, ["@"] = 1 })
    local sx, sy, open = 0, 0, {}
    for row, line in ipairs(MAP) do
      for col = 1, #line do
        local c = string.sub(line, col, col)
        if c == "@" then sx, sy = col - 1, row - 1 end
        if c == "." then table.insert(open, { col - 1, row - 1 }) end
      end
    end

    -- The hero on the @, at full health. map:center turns a cell into a
    -- place in the world.
    pe:get("Transform").pos = map:center(sx, sy)
    local hp = pe:get("Health")
    hp.cur = hp.max
    local pl = pe:get("Player")
    pl.dead = false
    pl.kills = 0
    q:entity("@turn"):get("Turn").n = 0

    -- The monsters, each on an open cell far enough from the hero.
    local far = {}
    for _, c in ipairs(open) do
      if math.abs(c[1] - sx) + math.abs(c[2] - sy) > SAFE then table.insert(far, c) end
    end
    local rng = q:rng()
    local placed = 0
    while placed < MONSTERS and #far > 0 do
      local cell = table.remove(far, rng:range(1, #far))
      local k = KINDS[rng:range(1, #KINDS)]
      local m = q:spawn({ "Monster", "Health", "Transform", "Sprite", "Describe" })
      local mo = m:get("Monster")
      mo.kind = k.kind
      mo.atk = k.atk
      local mh = m:get("Health")
      mh.cur = k.hp
      mh.max = k.hp
      m:get("Transform").pos = map:center(cell[1], cell[2])
      local sp = m:get("Sprite")
      sp.kind = "circle"
      sp.size = vector.create(0.7, 0.7, 0)
      sp.color = k.color
      sp.z = 4
      local ds = m:get("Describe")
      ds.name = k.kind
      ds.text = "HP {Health.cur}/{Health.max}"
      placed += 1
    end

    d.runs += 1
    q:emit("run.new", { run = d.runs, monsters = placed })
    -- A restart is not a turn: the hero and the monsters do not act on it.
    if restart then q:freeze() end
  end,
}
