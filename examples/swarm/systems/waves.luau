return system "sim.waves" {
  stage = "sim",
  reads = { "Director", "Enemy", "Transform" },
  writes = { "Director", "Enemy", "Transform", "Health" },
  rng = "waves",
  doc = "Counts the living, announces a cleared wave, and after the gap spawns the next one on a ring around the player.",
  run = function(q)
    local d = q:entity("@director"):get("Director")
    local alive = q:count("Enemy")
    d.alive = alive
    if d.wave > 0 and alive == 0 and not d.cleared then
      d.cleared = true
      d.timer = d.gap
      q:emit("wave.cleared", { wave = d.wave, tick = q.tick })
    end
    if d.wave ~= 0 and not d.cleared then return end
    if d.timer > 0 then
      d.timer -= 1
      return
    end
    local n = d.wave + 1
    local count = math.floor(d.first * d.growth ^ (n - 1))
    if count > d.cap then count = d.cap end
    local p = q:entity("@player"):get("Transform").pos
    local rng = q:rng()
    for i = 1, count do
      local a = rng:next() * 2 * math.pi
      local r = d.spawn_radius + rng:next() * 10
      local e = q:spawn()
      e:insert("Transform", { pos = { x = p.x + math.cos(a) * r, y = p.y + math.sin(a) * r } })
      e:insert("Enemy", { speed = 2.5 + rng:next() * 1.5, wave = n })
      e:insert("Health", { cur = 2, max = 2 })
    end
    d.wave = n
    d.cleared = false
    d.alive = count
    q:emit("wave.spawned", { wave = n, count = count, tick = q.tick })
  end,
}
