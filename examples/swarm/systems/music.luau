return system "late.music" {
  stage = "late",
  reads = { "Enemy", "Transform", "Player" },
  writes = { "Music" },
  doc = "Music intensity is how crowded it is: enemies within 15 units of the player, thirty is full.",
  run = function(q)
    local p = q:entity("@player"):get("Transform").pos
    local n = 0
    for e in q:within(p, 15, "Enemy") do n += 1 end
    local m = q:entity("@director"):get("Music")
    m.intensity = math.min(1, n / 30)
  end,
}
