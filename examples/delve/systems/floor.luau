return system "sim.floor" {
  stage = "sim",
  on_load = true, -- the first floor is there before the first key
  reads = { "Floor", "Player", "Monster", "Stairs", "Transform", "Tile" },
  writes = { "Floor", "Player", "Monster", "Stairs", "Transform", "Health", "Sprite", "Tile", "Describe" },
  rng = "floor",
  doc = "Generates a floor when there is none or the hero asked to descend: rooms, L-shaped corridors, the hero in the first room, stairs in the last, monsters never within four steps of the hero.",
  run = function(q)
    local f = q:entity("@floor"):get("Floor")
    local pe = q:entity("@player")
    local pl = pe:get("Player")
    if f.n > 0 and not pl.descend then return end

    -- Clear the old floor's runtime entities.
    local doomed = {}
    for e in q:each("Monster") do table.insert(doomed, e:id()) end
    for e in q:each("Stairs") do table.insert(doomed, e:id()) end
    for e in q:each("Tile") do table.insert(doomed, e:id()) end
    for _, id in ipairs(doomed) do q:entity(id):despawn() end

    local w, h = f.w, f.h
    local grid = table.create(w * h, "#")
    local rng = q:rng()
    local rooms = {}
    local tries = 0
    while #rooms < 6 and tries < 80 do
      tries += 1
      local rw, rh = rng:range(4, 8), rng:range(3, 5)
      local rx, ry = rng:range(1, w - rw - 2), rng:range(1, h - rh - 2)
      local ok = true
      for _, r in ipairs(rooms) do
        if rx < r.x + r.w + 1 and rx + rw + 1 > r.x and ry < r.y + r.h + 1 and ry + rh + 1 > r.y then
          ok = false
          break
        end
      end
      if ok then
        table.insert(rooms, { x = rx, y = ry, w = rw, h = rh, cx = rx + rw // 2, cy = ry + rh // 2 })
        for y = ry, ry + rh - 1 do
          for x = rx, rx + rw - 1 do
            grid[y * w + x + 1] = "."
          end
        end
      end
    end
    for i = 2, #rooms do
      local a, b = rooms[i - 1], rooms[i]
      local x, y = a.cx, a.cy
      while x ~= b.cx do
        grid[y * w + x + 1] = "."
        x += (b.cx > x) and 1 or -1
      end
      while y ~= b.cy do
        grid[y * w + x + 1] = "."
        y += (b.cy > y) and 1 or -1
      end
      grid[y * w + x + 1] = "."
    end
    f.map = table.concat(grid)
    f.n += 1
    f.rooms = #rooms

    local first, last = rooms[1], rooms[#rooms]
    pe:get("Transform").pos = { x = first.cx, y = first.cy }
    local st = q:spawn({ "Stairs", "Transform", "Sprite", "Describe" })
    st:get("Describe").name = "stairs"
    st:get("Transform").pos = vector.create(last.cx, last.cy, 0)
    local ss = st:get("Sprite")
    ss.kind = "rect"
    ss.size = vector.create(0.8, 0.8, 0)
    ss.color = "#7FD1FF"
    ss.z = 2
    -- One drawn cell per floor tile, so the window shows the map.
    local cells = {}
    for i = 1, w * h do
      if grid[i] == "." then table.insert(cells, i - 1) end
    end
    q:spawn_many(#cells, { "Tile", "Transform", "Sprite" }, function(e, i)
      local c = cells[i]
      e:get("Transform").pos = vector.create(c % w, c // w, 0)
      local sp = e:get("Sprite")
      sp.kind = "rect"
      sp.size = vector.create(0.95, 0.95, 0)
      sp.color = "#2E2A3A"
      sp.z = 0
    end)

    local count = 4 + 2 * f.n
    local placed, guard = 0, 0
    local kinds = { "rat", "bat", "goblin" }
    while placed < count and guard < 400 do
      guard += 1
      local r = rooms[rng:range(2, #rooms)]
      local x = rng:range(r.x, r.x + r.w - 1)
      local y = rng:range(r.y, r.y + r.h - 1)
      if math.abs(x - first.cx) + math.abs(y - first.cy) > 4 then
        local k = kinds[rng:range(1, 3)]
        local m = q:spawn({ "Monster", "Health", "Transform", "Sprite", "Describe" })
        -- What the semantic frame (and a screen reader) says about it.
        local d = m:get("Describe")
        d.name = k
        d.text = "HP {Health.cur}/{Health.max}"
        local mo = m:get("Monster")
        mo.kind = k
        mo.atk = (k == "goblin") and 3 or 1
        mo.initiative = rng:range(1, 9)
        local mh = m:get("Health")
        mh.cur = (k == "goblin") and 6 or 3
        mh.max = 6
        m:get("Transform").pos = vector.create(x, y, 0)
        local sp = m:get("Sprite")
        sp.kind = "circle"
        sp.size = vector.create(0.8, 0.8, 0)
        sp.color = (k == "goblin") and "#7FBF5A" or ((k == "bat") and "#A98BD6" or "#C2743F")
        sp.z = 4
        placed += 1
      end
    end
    pl.floor = f.n
    pl.descend = false
    q:emit("floor.generated", { floor = f.n, rooms = #rooms, monsters = placed, tries = tries })
  end,
}
