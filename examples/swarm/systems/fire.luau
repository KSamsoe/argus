return system "sim.fire" {
  stage = "sim",
  after = { "sim.chase" },
  reads = { "Player", "Transform" },
  writes = { "Player", "Bullet", "Velocity", "Transform" },
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
    for i = 0, n - 1 do
      local a = base + i * (2 * math.pi / n)
      local b = q:spawn()
      b:insert("Transform", { pos = { x = p.x, y = p.y } })
      b:insert("Velocity", { v = { x = math.cos(a) * 18, y = math.sin(a) * 18 } })
      b:insert("Bullet", {})
    end
    pl.aim = (base + 2.399963) % (2 * math.pi)
  end,
}
