return system "late.music" {
  stage = "late",
  reads = { "Enemy", "Transform", "Player" },
  writes = { "Music" },
  doc = "Music intensity is how crowded it is: enemies within 15 units of the player, thirty is full.",
  run = function(q)
    local p = q:entity("@player"):get("Transform").pos
    -- Thirty is full, so counting stops there: when the swarm closes in,
    -- tens of thousands are within 15 (found at the phase 5 gate: walking
    -- them all cost 10.7 ms a tick once the swarm reached the hero).
    local n = 0
    for e in q:within(p, 15, "Enemy") do
      n += 1
      if n >= 30 then break end
    end
    local m = q:entity("@director"):get("Music")
    m.intensity = math.min(1, n / 30)
  end,
}
