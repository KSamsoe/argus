return system "sim.chase" {
  stage = "sim",
  after = { "sim.waves" },
  reads = { "Enemy", "Transform", "Player" },
  writes = { "Transform" },
  doc = "Every enemy walks straight at the player. Positions are native vectors: no allocation per entity.",
  run = function(q)
    local pe = q:entity("@player")
    if pe:get("Player").dead then return end
    local p = pe:get("Transform").pos
    local dt = q.dt
    for e, en, t in q:each("Enemy", "Transform") do
      local d = p - t.pos
      local len = vector.magnitude(d)
      if len > 0.05 then
        t.pos = t.pos + d * (en.speed * dt / len)
      end
    end
  end,
}
