return system "sim.move" {
  stage = "sim",
  reads = { "Actions", "Player" },
  writes = { "Transform" },
  doc = "Walks the player by the move action.",
  run = function(q)
    for _, a, p, t in q:each("Actions", "Player", "Transform") do
      local m = a.move
      if m.x ~= 0 or m.y ~= 0 then
        t.pos = t.pos + m * (p.speed * q.dt)
      end
    end
  end,
}
