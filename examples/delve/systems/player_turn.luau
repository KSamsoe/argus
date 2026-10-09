-- Open ground: on the floor's map and no wall there. World (x, y) is the
-- map's cell (x, h - 1 - y): its rows go down (scenes/delve/floor.toml).
local function walkable(map, x, y)
  local w, h = map:size()
  if x < 0 or y < 0 or x >= w or y >= h then return false end
  return map:get("walls", x, h - 1 - y) == 0
end

return system "sim.player_turn" {
  stage = "sim",
  after = { "sim.floor" },
  reads = { "PlayerSlot", "Actions", "Tilemap", "Monster", "Stairs", "Transform", "Health", "Turn" },
  writes = { "Player", "Transform", "Health", "Turn" },
  doc = "The hero's turn: step in the move direction (a wall on the floor's map is a bump), attack a monster in the way, wait, descend on the stairs, or drink a potion (heals Player.potion_heal, never above Health.max; with none it says so and no turn passes).",
  run = function(q)
    local pe = q:entity("@player")
    local pl = pe:get("Player")
    local turn = q:entity("@turn"):get("Turn")
    local a = pe:get("Actions")

    -- Drinking with no potion does nothing but say so: no turn passes, so
    -- the rest of this one is frozen (the monsters do not act), as the
    -- menu does; sim.messages runs unfrozen to say it.
    if not pl.dead and a.drink_pressed and pl.potions == 0 then
      q:emit("potion.none", { turn = turn.n })
      q:freeze()
      return
    end

    turn.n += 1
    if pl.dead then return end
    local pos = pe:get("Transform").pos

    if a.descend_pressed then
      for e, st, tr in q:each("Stairs", "Transform") do
        if tr.pos.x == pos.x and tr.pos.y == pos.y then
          pl.descend = true
          q:emit("floor.descend", { from = pl.floor, turn = turn.n })
          return
        end
      end
      q:emit("bump", { what = "no stairs here", turn = turn.n })
      return
    end

    -- Drink a potion: it heals a set amount, never above the maximum (nor
    -- below where the hero is), and takes the turn, even at full health.
    if a.drink_pressed then
      local hp = pe:get("Health")
      local before = hp.cur
      local after = math.max(before, math.min(hp.max, before + pl.potion_heal))
      hp.cur = after
      pl.potions -= 1
      q:emit("potion.drunk", { healed = after - before, hp = after, max = hp.max, left = pl.potions, turn = turn.n })
      return
    end

    local m = a.move
    local dx, dy = 0, 0
    if m.x > 0.5 then dx = 1
    elseif m.x < -0.5 then dx = -1
    elseif m.y > 0.5 then dy = 1
    elseif m.y < -0.5 then dy = -1 end
    if dx == 0 and dy == 0 then
      q:emit("wait", { turn = turn.n })
      return
    end
    local nx, ny = pos.x + dx, pos.y + dy
    for e, mo, tr, hp in q:each("Monster", "Transform", "Health") do
      if tr.pos.x == nx and tr.pos.y == ny then
        local left = hp.cur - pl.atk
        if left < 0 then left = 0 end
        hp.cur = left
        q:emit("damage", { target = e, kind = mo.kind, by = "@player", amount = pl.atk, turn = turn.n })
        q:play("sfx/hit", { entity = e })
        if left == 0 then
          pl.kills += 1
          -- Where it fell and its drop chance, for sim.potions (the
          -- entity is gone by then).
          q:emit("death", { entity = e, kind = mo.kind, cause = "hero", turn = turn.n, x = nx, y = ny, drop = mo.drop })
          e:despawn()
        end
        return
      end
    end
    if walkable(q:tiles("@floor"), nx, ny) then
      pe:get("Transform").pos = { x = nx, y = ny }
      q:play("sfx/step")
    else
      q:emit("bump", { x = nx, y = ny, turn = turn.n })
      q:play("sfx/bump")
    end
  end,
}
