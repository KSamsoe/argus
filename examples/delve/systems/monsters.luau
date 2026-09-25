local DIRS = { { 1, 0 }, { -1, 0 }, { 0, 1 }, { 0, -1 } }

return system "sim.monsters" {
  stage = "sim",
  after = { "sim.player_turn" },
  reads = { "Monster", "Transform", "Health", "Player", "Floor", "Turn", "NavField" },
  writes = { "Transform", "Health", "Player" },
  doc = "Every monster acts in initiative order (ties by id): strike the hero when adjacent, else step along the shortest path toward it when within sight (the hero's flow field, section 12).",
  run = function(q)
    local pe = q:entity("@player")
    local pl = pe:get("Player")
    local turn = q:entity("@turn"):get("Turn").n
    if pl.dead then
      q:emit("turn.end", { turn = turn, hero_dead = true })
      return
    end
    local w = q:entity("@floor"):get("Floor").w
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
    for _, m in ipairs(order) do occupied[m.y * w + m.x] = true end
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
      elseif here ~= nil and here <= m.sight then
        local best, bx, by = here, nil, nil
        for _, c in ipairs(DIRS) do
          local nx, ny = m.x + c[1], m.y + c[2]
          local d = steps(nx, ny)
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
