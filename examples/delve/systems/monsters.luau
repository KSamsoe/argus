local DIRS = { { 1, 0 }, { -1, 0 }, { 0, 1 }, { 0, -1 } }

-- One number a cell, for the cells monsters stand in.
local function cell(x, y)
  return y * 65536 + x
end

return system "sim.monsters" {
  stage = "sim",
  after = { "sim.player_turn" },
  reads = { "Monster", "Transform", "Health", "Player", "Turn", "NavField", "Tilemap", "DebugView" },
  writes = { "Transform", "Health", "Player" },
  doc = "Every monster acts in initiative order (ties by id): strike the hero when adjacent, else step along the shortest path toward it when within sight: every turn when no wall is between them (a ray against the floor's walls), every other turn when it only follows the trail (the hero's flow field, section 12). With @turn's DebugView.sight it draws each sight line.",
  run = function(q)
    local pe = q:entity("@player")
    local hero = pe:get("Transform").pos
    local debug = q:entity("@turn"):get("DebugView").sight
    -- Whether a monster at (x, y) sees the hero: a ray from its cell's
    -- centre to the hero's meets no wall (2D staples item 0). With the
    -- debug view on, the ray is drawn (item 1).
    local function sees(x, y)
      local from = vector.create(x, y, 0)
      local wall = q:raycast(from, hero, { colliders = false })
      if debug then
        if wall then
          q:draw_line(from, wall.point, { color = "#FF5050" })
          q:draw_circle(wall.point, 0.12, { color = "#FF5050", fill = true })
        else
          q:draw_line(from, hero, { color = "#60FF90" })
        end
      end
      return wall == nil
    end
    local pl = pe:get("Player")
    local turn = q:entity("@turn"):get("Turn").n
    if pl.dead then
      q:emit("turn.end", { turn = turn, hero_dead = true })
      return
    end
    local order = {}
    for e, mo, tr in q:each("Monster", "Transform") do
      table.insert(order, {
        id = e:id(), init = mo.initiative, x = tr.pos.x, y = tr.pos.y,
        atk = mo.atk, sight = mo.sight, kind = mo.kind,
      })
    end
    table.sort(order, function(a, b)
      if a.init ~= b.init then return a.init > b.init end
      return a.id < b.id
    end)
    -- Steps from a cell to the hero, down the hero's flow field (a Rust
    -- hot path; it was a breadth-first search here, every turn).
    local function steps(x, y)
      return q:distance(pe, vector.create(x, y, 0))
    end
    local occupied = {}
    for _, m in ipairs(order) do occupied[cell(m.x, m.y)] = true end
    local php = pe:get("Health")
    for _, m in ipairs(order) do
      local here = steps(m.x, m.y)
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
      -- One that sees the hero comes every turn; one that only follows its
      -- trail, every other turn.
      elseif here ~= nil and here <= m.sight and (sees(m.x, m.y) or turn % 2 == 0) then
        local best, bx, by = here, nil, nil
        for _, c in ipairs(DIRS) do
          local nx, ny = m.x + c[1], m.y + c[2]
          local d = steps(nx, ny)
          if d ~= nil and d < best and d > 0 and not occupied[cell(nx, ny)] then
            best, bx, by = d, nx, ny
          end
        end
        if bx ~= nil then
          occupied[cell(m.x, m.y)] = nil
          occupied[cell(bx, by)] = true
          q:entity(m.id):get("Transform").pos = { x = bx, y = by }
          m.x, m.y = bx, by
        end
      end
    end
    q:emit("turn.end", { turn = turn })
  end,
}
