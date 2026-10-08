-- The floor is a runtime map (phase 5 item 4): sim.floor paints its cells
-- on @floor's map from the seed. The semantic frame reads them as the
-- map's rows (# a wall, . open), with the hero, the monsters, and the
-- stairs still listed as entities over them (section 8: for Delve, L4 is
-- nearly the whole game). And they are the run's: a seek back across a
-- descent shows the floor before it again, a save and a load keep the
-- floor, and a replay saved after the descent comes back to the same
-- cells, whether its keyframe is before the descent (the replay paints
-- them again) or after it (the keyframe holds them).

local MAP = "maps/floor.tilemap.toml"

-- The whole map, top row first, from the collision layers (the walls).
local function rows(t)
  local m = t:call("scene.tiles.list", {}).rows[1]
  local r = t:call("scene.tiles.collision", { file = MAP, rect = { x = 0, y = 0, w = m.width, h = m.height } })
  return r.rows
end

local function same(a, b)
  return table.concat(a, "/") == table.concat(b, "/")
end

local function check_rows(t, want, what)
  local now = rows(t)
  if not same(now, want) then
    error(what .. ": the map's rows are not the floor's:\n" .. table.concat(now, "\n") .. "\nwanted\n" .. table.concat(want, "\n"), 0)
  end
end

local function at(t, id)
  local p = t:call("world.get", { entity = id }).components.Transform.pos
  return p[1], p[2]
end

local function stairs(t)
  local p = t:call("world.query", { select = { "Transform.pos" }, where = { has = { "Stairs" } } }).rows[1][1]
  return p[1], p[2]
end

-- The hero walks the shortest way to the stairs (hitting what stands in
-- it); true on the stairs, false dead or out of turns.
local function walk_to_stairs(t, turns)
  for _ = 1, turns do
    local me = t:call("world.get", { entity = "@player" }).components
    if me.Player.dead then return false end
    local sx, sy = stairs(t)
    local x, y = me.Transform.pos[1], me.Transform.pos[2]
    if x == sx and y == sy then return true end
    local p = t:call("nav.path", { from = "@player", to = { sx, sy } })
    if not p.found then error("no way to the stairs: " .. tostring(p.why), 0) end
    local nx, ny = p.points[1][1], p.points[1][2]
    t.input:tap("move", { nx - x, ny - y })
    t:advance { by = "turn", n = 1 }
  end
  return false
end

return scenario "the floor is a runtime map the semantic frame reads and the run keeps" {
  -- Seeds whose hero lives to walk to the stairs (seed 1's first replay
  -- starts at the run's first frame, seed 13's at a later keyframe).
  scene = "delve", seeds = { 1, 13 },
  run = function(t)
    local first = rows(t)
    if #first ~= 20 or #first[1] ~= 40 then error("a 40 x 20 map: " .. #first .. " rows of " .. #first[1], 0) end
    local walls, open = 0, 0
    for _, r in ipairs(first) do
      if not string.match(r, "^[#.]+$") then error("a row of walls and open cells: " .. r, 0) end
      local _, n = string.gsub(r, "%.", "")
      open += n
      walls += #r - n
    end
    if open < 40 or walls < 400 then error(string.format("rooms in rock: %d open, %d walls", open, walls), 0) end

    -- The semantic frame lists the map in view, a character a cell, as
    -- the screen shows it; and the hero, the stairs, and the monsters in
    -- view as entities, each on an open cell. The floor is no group.
    local f = t:call("obs.frame", { format = "semantic" })
    if f.tiles == nil or #f.tiles.maps ~= 1 then error("the frame's tiles: " .. tostring(f.tiles), 0) end
    local m = f.tiles.maps[1]
    if m.entity ~= "@floor" or m.map ~= MAP or m.step ~= 1 then
      error(string.format("the frame's map: %s %s step %s", tostring(m.entity), tostring(m.map), tostring(m.step)), 0)
    end
    local c0, r0 = m.cell[1], m.cell[2]
    for i, r in ipairs(m.rows) do
      if r ~= string.sub(first[r0 + i], c0 + 1, c0 + #r) then
        error("the frame's row " .. (r0 + i - 1) .. ", " .. r .. ", is not the map's " .. first[r0 + i], 0)
      end
    end
    for _, g in ipairs(f.groups) do
      if g.what == "Tile" then error("the floor's cells as a group again", 0) end
    end
    local h = #first
    local listed = {}
    for _, e in ipairs(f.entities) do
      local id, name = e[1], e[2]
      local x, y = at(t, id)
      local row = m.rows[(h - 1 - y) - r0 + 1]
      local c = row and string.sub(row, x - c0 + 1, x - c0 + 1)
      if c ~= "." then error(string.format("%s (%s) at %d, %d stands on %s", name, id, x, y, tostring(c)), 0) end
      listed[id] = name
    end
    if listed["@player"] ~= "hero" then error("the hero is listed", 0) end
    -- Every monster and the stairs the camera shows, by name.
    local v = f.camera.rect
    local things = t:call("world.query", { select = { "id", "Transform.pos", "Describe.name" }, where = { has = { "Describe" } }, limit = 100 })
    for _, r in ipairs(things.rows) do
      local p = r[2]
      if p[1] > v[1] and p[1] < v[3] and p[2] > v[2] and p[2] < v[4] and listed[r[1]] ~= r[3] then
        error(string.format("%s (%s) is in view and not listed", r[3], r[1]), 0)
      end
    end

    -- Down the stairs, with the hit points to live through the walk on
    -- any seed.
    t:call("sim.set", { entity = "@player", path = "Health.cur", value = 1000 })
    if not walk_to_stairs(t, 200) then error("the hero did not reach the stairs", 0) end
    local on_floor1 = t:status().tick
    t.input:tap("descend")
    t:advance { by = "turn", n = 2 }
    t.expect:field("@floor", "Floor.n"):eq(2)
    t.expect:field("@player", "Player.floor"):eq(2)
    local second = rows(t)
    if same(first, second) then error("floor 2 is floor 1", 0) end
    local on_floor2 = t:status().tick
    t:call("replay.save", { dir = ".argus/replays/delve-map/descent", clip = false, note = "just after the descent" })
    -- Past the next keyframe and a replay's ten turns of lead-in, so the
    -- later replay starts on floor 2.
    t:advance { by = "turn", n = 22 }
    check_rows(t, second, "22 turns on")
    local later = t:status().tick

    -- A seek back across the descent: floor 1's cells again; forward: 2's.
    local s = t:call("sim.seek", { frame = on_floor1 })
    if s.diverged ~= nil then error("the replay to floor 1 diverged", 0) end
    check_rows(t, first, "back on floor 1")
    t.expect:field("@floor", "Floor.n"):eq(1)
    s = t:call("sim.seek", { frame = later })
    if s.diverged ~= nil then error("the replay to floor 2 diverged", 0) end
    check_rows(t, second, "forward to floor 2")
    t:log(string.format("descended at frame %d; sought back to %d and forward to %d", on_floor2, on_floor1, later))

    -- A save keeps the floor; a load puts it back over floor 1's.
    t:call("replay.save", { dir = ".argus/replays/delve-map/later", clip = false, note = "22 turns after the descent" })
    t:call("sim.save", { name = "delve-map" })
    t:call("sim.seek", { frame = on_floor1 })
    check_rows(t, first, "back on floor 1 before the load")
    t:call("sim.load", { name = "delve-map" })
    check_rows(t, second, "loaded")
    t.expect:field("@floor", "Floor.n"):eq(2)

    -- The replays: the same world, and the same cells. The first starts
    -- before the floor was made (a keyframe before frame on_floor2 is
    -- before the tick that painted it), the second after.
    for _, r in ipairs({ { "descent", on_floor2, true }, { "later", later, false } }) do
      local o = t:call("replay.open", { path = ".argus/replays/delve-map/" .. r[1] })
      if not o.digest.same then error("the replay " .. r[1] .. " is another world", 0) end
      if o.frame ~= r[2] then error("the replay " .. r[1] .. " stops at " .. o.frame .. ", not " .. r[2], 0) end
      if (o.from < on_floor2) ~= r[3] then
        error(string.format("the replay %s starts at keyframe %d; the floor was painted in tick %d", r[1], o.from, on_floor2 - 1), 0)
      end
      check_rows(t, second, "the replay " .. r[1])
      t:log(string.format("replay %s: from keyframe %d, %s the descent, to frame %d", r[1], o.from, r[3] and "before" or "after", o.frame))
    end
  end,
}
