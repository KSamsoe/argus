return system "sim.fire" {
  stage = "sim",
  after = { "sim.chase" },
  reads = { "Player", "Transform" },
  writes = { "Player", "Bullet", "Velocity", "Transform", "Sprite" },
  doc = "The player fires a fan of bullets every fire_every ticks; the fan rotates by the golden angle each burst so no direction is favoured.",
  run = function(q)
    local pe = q:entity("@player")
    local pl = pe:get("Player")
    if pl.dead then return end
    if pl.cooldown > 0 then
      pl.cooldown -= 1
      return
    end
    pl.cooldown = pl.fire_every
    local p = pe:get("Transform").pos
    local n = pl.bullets_per_burst
    local base = pl.aim
    q:spawn_many(n, { "Transform", "Velocity", "Bullet", "Sprite" }, function(b, i)
      local a = base + (i - 1) * (2 * math.pi / n)
      b:get("Transform").pos = p
      b:get("Velocity").v = vector.create(math.cos(a) * 18, math.sin(a) * 18, 0)
      local sp = b:get("Sprite")
      sp.kind = "rect"
      sp.size = vector.create(0.3, 0.3, 0)
      sp.color = "#FFD866"
      sp.z = 3
    end)
    pl.aim = (base + 2.399963) % (2 * math.pi)
  end,
}
