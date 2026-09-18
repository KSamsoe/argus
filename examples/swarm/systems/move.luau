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
          t.pos = t.pos + m * (pl.speed * dt)
        end
      end
    end
  end,
}
