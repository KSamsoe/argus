return system "sim.waves" {
  stage = "sim",
  reads = { "Director", "Enemy", "Transform" },
  writes = { "Director", "Enemy", "Transform", "Health", "Sprite", "Dasher" },
  rng = "waves",
  doc = "Counts the living, announces a cleared wave, and after the gap spawns the next one on a ring around the player, in one batch; from Director.dash_from on, dash_share of it are dashers.",
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
    -- From dash_from on, a share of the wave are dashers (sim.dash, in
    -- plugins/swarm-hot).
    local dashers = 0
    if n >= d.dash_from then
      dashers = math.floor(count * d.dash_share)
    end
    local p = q:entity("@player"):get("Transform").pos
    local rng = q:rng()
    local radius = d.spawn_radius
    -- One enemy on the ring: the same draws, in the same order, for both kinds.
    local function place(e)
      local a = rng:next() * 2 * math.pi
      local r = radius + rng:next() * 10
      e:get("Transform").pos = p + vector.create(math.cos(a) * r, math.sin(a) * r, 0)
      local en = e:get("Enemy")
      en.speed = 2.5 + rng:next() * 1.5
      en.wave = n
      local hp = e:get("Health")
      hp.cur = 2
      hp.max = 2
      local sp = e:get("Sprite")
      sp.kind = "circle"
      sp.z = 1
      return sp
    end
    if count > dashers then
      q:spawn_many(count - dashers, { "Transform", "Enemy", "Health", "Sprite" }, function(e, i)
        local sp = place(e)
        sp.size = vector.create(0.9, 0.9, 0)
        sp.color = "#E8703F"
      end)
    end
    if dashers > 0 then
      q:spawn_many(dashers, { "Transform", "Enemy", "Health", "Sprite", "Dasher" }, function(e, i)
        local sp = place(e)
        -- Three bullets, not one: a dasher shot down in its first steps
        -- would never be met.
        local hp = e:get("Health")
        hp.cur = 6
        hp.max = 6
        -- The walking look (systems/dash_look.luau).
        sp.size = vector.create(1.0, 1.0, 0)
        sp.color = "#B45CFF"
        -- Out of step with each other: the first dash may come any time
        -- in its first every ticks (once the player is in range).
        local da = e:get("Dasher")
        da.timer = math.floor(rng:next() * da.every)
      end)
    end
    d.wave = n
    d.cleared = false
    d.alive = count
    q:emit("wave.spawned", { wave = n, count = count, dashers = dashers, tick = q.tick })
  end,
}
