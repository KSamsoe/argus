-- The hero walks by the move action and stays inside the arena.
--
-- The arena is the floor you see: @arena's Sprite (scenes/main/arena.toml).
-- Its size is the walls, so making the arena bigger is one number there.
-- On keys the move action is already eight-way and the same speed on a
-- diagonal; on a stick it is any direction.
return system "sim.move" {
  stage = "sim",
  after = { "sim.restart" },
  reads = { "Actions", "Player", "Sprite" },
  writes = { "Transform" },
  doc = "Walks the hero by the move action at Player.speed, kept inside the arena's rect (@arena's Sprite).",
  run = function(q)
    local pe = q:entity("@player")
    local pl = pe:get("Player")
    if pl.dead then return end
    local t = pe:get("Transform")
    local m = pe:get("Actions").move
    if m.x == 0 and m.y == 0 then return end
    local pos = t.pos + m * (pl.speed * q.dt)

    -- The walls: the arena's half size, less the hero's own half size.
    local arena = q:entity("@arena")
    local centre = arena:get("Transform").pos
    local half = arena:get("Sprite").size * 0.5 - pe:get("Sprite").size * 0.5
    local x = math.clamp(pos.x, centre.x - half.x, centre.x + half.x)
    local y = math.clamp(pos.y, centre.y - half.y, centre.y + half.y)
    t.pos = vector.create(x, y, 0)
  end,
}
