-- The chase: every enemy walks straight at the hero.
--
-- This is the simplest chase there is, and the place to make enemies
-- smarter: keep them apart from each other (q:within finds the near ones),
-- let some circle or charge, or give a kind its own speed. Positions are
-- native vectors, so this loop allocates nothing per enemy and a few
-- hundred enemies cost little.
return system "sim.chase" {
  stage = "sim",
  after = { "sim.waves" },
  reads = { "Enemy", "Player", "Transform" },
  writes = { "Transform" },
  doc = "Every enemy walks straight at the hero at its Enemy.speed (nothing moves once the hero is dead).",
  run = function(q)
    local pe = q:entity("@player")
    if pe:get("Player").dead then return end
    local hero = pe:get("Transform").pos
    local dt = q.dt
    for _, en, t in q:each("Enemy", "Transform") do
      local d = hero - t.pos
      local len = vector.magnitude(d)
      if len > 0.05 then
        t.pos = t.pos + d * (en.speed * dt / len)
      end
    end
  end,
}
