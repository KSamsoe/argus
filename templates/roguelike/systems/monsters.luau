-- The monsters' turn, right after the hero's. Each monster in turn: next
-- to the hero, it bites; else, if the hero is within its sight, it steps
-- to the neighbouring cell nearest the hero. "Nearest" counts steps around
-- the walls, read from the hero's flow field (the NavField on @player),
-- not a straight line, so a monster finds its way round a corner.
-- Give monsters other minds here: flee when hurt, wander when they cannot
-- see the hero, keep their distance and shoot.

local DIRS = { { 1, 0 }, { -1, 0 }, { 0, 1 }, { 0, -1 } }

-- One number for a cell, to remember which cells monsters stand in.
local function key(x, y)
  return y * 65536 + x
end

return system "sim.monsters" {
  stage = "sim",
  after = { "sim.hero_turn" },
  reads = { "Monster", "Transform", "Health", "Player", "Turn", "NavField" },
  writes = { "Transform", "Health", "Player" },
  doc = "Every monster acts, in the order they were made: bite the hero when next to it (Monster.atk), else step one cell down the hero's flow field when the hero is within Monster.sight steps, never onto another monster. Emits damage and player.died.",
  run = function(q)
    local pe = q:entity("@player")
    local pl = pe:get("Player")
    if pl.dead then return end
    local hp = pe:get("Health")
    local turn = q:entity("@turn"):get("Turn").n

    -- Where every monster stands, so two never share a cell.
    local monsters, taken = {}, {}
    for e, mo, tr in q:each("Monster", "Transform") do
      table.insert(monsters, { id = e:id(), kind = mo.kind, atk = mo.atk, sight = mo.sight, x = tr.pos.x, y = tr.pos.y })
      taken[key(tr.pos.x, tr.pos.y)] = true
    end

    -- Steps from a cell to the hero, around the walls; nil for rock.
    local function steps(x, y)
      return q:distance(pe, vector.create(x, y, 0))
    end

    for _, m in ipairs(monsters) do
      local here = steps(m.x, m.y)
      if here == 1 then
        hp.cur = math.max(0, hp.cur - m.atk)
        q:emit("damage", { by = m.id, target = "@player", kind = m.kind, amount = m.atk, left = hp.cur, turn = turn })
        if hp.cur == 0 then
          pl.dead = true
          q:emit("player.died", { killer = m.id, kind = m.kind, kills = pl.kills, turn = turn })
          break
        end
      elseif here ~= nil and here <= m.sight then
        -- The free neighbour with the fewest steps left to the hero.
        local best, bx, by = here, nil, nil
        for _, d in ipairs(DIRS) do
          local nx, ny = m.x + d[1], m.y + d[2]
          local s = steps(nx, ny)
          if s ~= nil and s > 0 and s < best and not taken[key(nx, ny)] then
            best, bx, by = s, nx, ny
          end
        end
        if bx ~= nil then
          taken[key(m.x, m.y)] = nil
          taken[key(bx, by)] = true
          q:entity(m.id):get("Transform").pos = vector.create(bx, by, 0)
        end
      end
    end
  end,
}
