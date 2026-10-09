-- The hero's turn, the first thing in every turn. The move action names a
-- neighbouring cell: a monster there is attacked, rock there is a bump
-- (and no turn passes), open floor is walked onto. No move (the wait
-- action, or a turn with no key) is a wait. Add new commands here: an
-- action in input.toml, a branch below.

-- Whether a place blocks: rock on the map's walls layer, or off the map.
-- map:cell turns a place in the world into the map's column and row.
local function blocked(map, pos)
  local col, row = map:cell(pos)
  local w, h = map:size()
  if col < 0 or row < 0 or col >= w or row >= h then return true end
  return map:get("walls", col, row) ~= 0
end

return system "sim.hero_turn" {
  stage = "sim",
  after = { "sim.dungeon" },
  reads = { "Actions", "Player", "Monster", "Transform", "Health", "Tilemap", "Turn" },
  writes = { "Player", "Transform", "Health", "Turn" },
  doc = "The hero's turn: step one cell in the move direction, attack a monster standing there (Player.atk), or bump rock (no turn passes and the monsters do not act); no move is a wait. Emits damage, death, bump, and wait.",
  run = function(q)
    local pe = q:entity("@player")
    local pl = pe:get("Player")
    if pl.dead then return end -- the run is over; the restart action starts the next
    local turn = q:entity("@turn"):get("Turn")

    -- One cell, never diagonal.
    local m = pe:get("Actions").move
    local dx, dy = 0, 0
    if m.x > 0.5 then dx = 1
    elseif m.x < -0.5 then dx = -1
    elseif m.y > 0.5 then dy = 1
    elseif m.y < -0.5 then dy = -1 end
    if dx == 0 and dy == 0 then
      turn.n += 1
      q:emit("wait", { turn = turn.n })
      return
    end

    local pos = pe:get("Transform").pos
    local to = vector.create(pos.x + dx, pos.y + dy, 0)

    -- A monster in that cell? Then the step is an attack. (Keep the id,
    -- not the entity: q:each hands out one handle it reuses.)
    local target = nil
    for e, _, tr in q:each("Monster", "Transform") do
      if tr.pos.x == to.x and tr.pos.y == to.y then
        target = e:id()
        break
      end
    end
    if target == nil and blocked(q:tiles("@dungeon"), to) then
      -- Walking into rock costs nothing: the rest of this turn stands
      -- still (sim.messages runs anyway, to say "A wall.").
      q:emit("bump", { x = to.x, y = to.y })
      q:freeze()
      return
    end

    turn.n += 1
    if target == nil then
      pe:get("Transform").pos = to
      return
    end
    local me = q:entity(target)
    local mo = me:get("Monster")
    local hp = me:get("Health")
    hp.cur = math.max(0, hp.cur - pl.atk)
    q:emit("damage", { by = "@player", target = me, kind = mo.kind, amount = pl.atk, left = hp.cur, turn = turn.n })
    if hp.cur == 0 then
      pl.kills += 1
      q:emit("death", { entity = me, kind = mo.kind, cause = "hero", turn = turn.n })
      me:despawn()
    end
  end,
}
