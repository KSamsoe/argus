return system "sim.move" {
  stage = "sim",
  reads = { "PlayerSlot", "Actions", "Player" },
  writes = { "Transform" },
  doc = "Walks the player by the move action.",
  run = function(q)
    local dt = q.dt
    for e, a, pl, t in q:each("Actions", "Player", "Transform") do
      if not pl.dead then
        local m = a.move
        if m.x ~= 0 or m.y ~= 0 then
          local pos = t.pos
          t.pos = { x = pos.x + m.x * pl.speed * dt, y = pos.y + m.y * pl.speed * dt }
        end
      end
    end
  end,
}
