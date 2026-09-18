return system "sim.chase" {
  stage = "sim",
  after = { "sim.waves" },
  reads = { "Enemy", "Transform", "Player" },
  writes = { "Transform" },
  doc = "Every enemy walks straight at the player.",
  run = function(q)
    local pe = q:entity("@player")
    if pe:get("Player").dead then return end
    local p = pe:get("Transform").pos
    local px, py, dt = p.x, p.y, q.dt
    for e, en, t in q:each("Enemy", "Transform") do
      local pos = t.pos
      local dx, dy = px - pos.x, py - pos.y
      local len = math.sqrt(dx * dx + dy * dy)
      if len > 0.05 then
        local s = en.speed * dt / len
        t.pos = { x = pos.x + dx * s, y = pos.y + dy * s }
      end
    end
  end,
}
