local function walkable(f, x, y)
  if x < 0 or y < 0 or x >= f.w or y >= f.h then return false end
  local i = y * f.w + x + 1
  return string.sub(f.map, i, i) == "."
end

local DIRS = { { 1, 0 }, { -1, 0 }, { 0, 1 }, { 0, -1 } }

-- Steps from every reachable cell to the hero, by breadth-first search
-- over the floor: eight hundred cells, once per turn.
local function distance_field(f, px, py, limit)
  local w = f.w
  local dist = {}
  local start = py * w + px
  dist[start] = 0
  local queue = { start }
  local head = 1
  while head <= #queue do
    local i = queue[head]
    head += 1
    local d0 = dist[i]
    if d0 < limit then
      local x, y = i % w, i // w
      for _, c in ipairs(DIRS) do
        local nx, ny = x + c[1], y + c[2]
        if walkable(f, nx, ny) then
          local j = ny * w + nx
          if dist[j] == nil then
            dist[j] = d0 + 1
            table.insert(queue, j)
          end
        end
      end
    end
  end
  return dist
end

return system "sim.monsters" {
  stage = "sim",
  after = { "sim.player_turn" },
  reads = { "Monster", "Transform", "Health", "Player", "Floor", "Turn" },
  writes = { "Transform", "Health", "Player" },
  doc = "Every monster acts in initiative order (ties by id): strike the hero when adjacent, else step along the shortest path toward it when within sight.",
  run = function(q)
    local pe = q:entity("@player")
    local pl = pe:get("Player")
    local turn = q:entity("@turn"):get("Turn").n
    if pl.dead then
      q:emit("turn.end", { turn = turn, hero_dead = true })
      return
    end
    local ppos = pe:get("Transform").pos
    local px, py = ppos.x, ppos.y
    local f = q:entity("@floor"):get("Floor")
    local w = f.w
    local order = {}
    local max_sight = 0
    for e, mo, tr in q:each("Monster", "Transform") do
      table.insert(order, {
        id = e:id(), init = mo.initiative, x = tr.pos.x, y = tr.pos.y,
        atk = mo.atk, sight = mo.sight, kind = mo.kind,
      })
      if mo.sight > max_sight then max_sight = mo.sight end
    end
    table.sort(order, function(a, b)
      if a.init ~= b.init then return a.init > b.init end
      return a.id < b.id
    end)
    local dist = distance_field(f, px, py, max_sight)
    local occupied = {}
    for _, m in ipairs(order) do occupied[m.y * w + m.x] = true end
    local php = pe:get("Health")
    for _, m in ipairs(order) do
      local here = dist[m.y * w + m.x]
      if here == 1 then
        local left = php.cur - m.atk
        if left < 0 then left = 0 end
        php.cur = left
        q:emit("damage", { target = "@player", by = m.id, kind = m.kind, amount = m.atk, turn = turn })
        q:play("sfx/hit", { entity = q:entity(m.id) })
        if left == 0 then
          pl.dead = true
          q:emit("player.died", { floor = pl.floor, turn = turn, killer = m.id, kind = m.kind, kills = pl.kills })
          q:play("sfx/death")
          break
        end
      elseif here ~= nil and here <= m.sight then
        local best, bx, by = here, nil, nil
        for _, c in ipairs(DIRS) do
          local nx, ny = m.x + c[1], m.y + c[2]
          local d = dist[ny * w + nx]
          if d ~= nil and d < best and d > 0 and not occupied[ny * w + nx] then
            best, bx, by = d, nx, ny
          end
        end
        if bx ~= nil then
          occupied[m.y * w + m.x] = nil
          occupied[by * w + bx] = true
          q:entity(m.id):get("Transform").pos = { x = bx, y = by }
          m.x, m.y = bx, by
        end
      end
    end
    q:emit("turn.end", { turn = turn })
  end,
}
