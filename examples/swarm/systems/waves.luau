return system "sim.waves" {
  stage = "sim",
  reads = { "Director", "Enemy", "Transform" },
  writes = { "Director", "Enemy", "Transform", "Health" },
  rng = "waves",
  doc = "Counts the living, announces a cleared wave, and after the gap spawns the next one on a ring around the player, in one batch.",
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
    local radius = d.spawn_radius
    q:spawn_many(count, { "Transform", "Enemy", "Health" }, function(e, i)
      local a = rng:next() * 2 * math.pi
      local r = radius + rng:next() * 10
      e:get("Transform").pos = p + vector.create(math.cos(a) * r, math.sin(a) * r, 0)
      local en = e:get("Enemy")
      en.speed = 2.5 + rng:next() * 1.5
      en.wave = n
      local hp = e:get("Health")
      hp.cur = 2
      hp.max = 2
    end)
    d.wave = n
    d.cleared = false
    d.alive = count
    q:emit("wave.spawned", { wave = n, count = count, tick = q.tick })
  end,
}
