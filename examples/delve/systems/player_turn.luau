local function walkable(f, x, y)
  if x < 0 or y < 0 or x >= f.w or y >= f.h then return false end
  local i = y * f.w + x + 1
  return string.sub(f.map, i, i) == "."
end

return system "sim.player_turn" {
  stage = "sim",
  after = { "sim.floor" },
  reads = { "PlayerSlot", "Actions", "Floor", "Monster", "Stairs", "Transform", "Health", "Turn" },
  writes = { "Player", "Transform", "Health", "Turn" },
  doc = "The hero's turn: step in the move direction, attack a monster in the way, wait, or descend on the stairs.",
  run = function(q)
    local pe = q:entity("@player")
    local pl = pe:get("Player")
    local turn = q:entity("@turn"):get("Turn")
    turn.n += 1
    if pl.dead then return end
    local a = pe:get("Actions")
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
          q:emit("death", { entity = e, kind = mo.kind, cause = "hero", turn = turn.n })
          e:despawn()
        end
        return
      end
    end
    local f = q:entity("@floor"):get("Floor")
    if walkable(f, nx, ny) then
      pe:get("Transform").pos = { x = nx, y = ny }
      q:play("sfx/step")
    else
      q:emit("bump", { x = nx, y = ny, turn = turn.n })
      q:play("sfx/bump")
    end
  end,
}
