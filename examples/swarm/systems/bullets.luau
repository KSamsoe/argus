return system "sim.bullets" {
  stage = "sim",
  after = { "sim.fire" },
  reads = { "Bullet", "Velocity", "Transform" },
  writes = { "Bullet", "Transform" },
  doc = "Bullets fly straight and expire after ttl ticks.",
  run = function(q)
    local dt = q.dt
    local doomed = {}
    for e, b, v, t in q:each("Bullet", "Velocity", "Transform") do
      if b.ttl == 0 then
        table.insert(doomed, e:id())
      else
        b.ttl -= 1
        local pos, vel = t.pos, v.v
        t.pos = { x = pos.x + vel.x * dt, y = pos.y + vel.y * dt }
      end
    end
    for _, id in ipairs(doomed) do
      q:entity(id):despawn()
    end
  end,
}
