-- The waves: how enemies come into the arena.
--
-- @arena's Waves component (scenes/main/arena.toml) holds the numbers:
-- wave 1 is `first` enemies, each wave after adds `more`, with `gap` ticks
-- of quiet between. Each enemy starts at a random point on the arena's
-- edge, never right on top of the hero. All randomness comes from this
-- system's own stream (rng = "waves"), seeded by the run's seed, so the same
-- seed always sends the same waves: that is what lets the tests replay.
--
-- To change what an enemy is (its look, health, speed), change `spawn_one`.
return system "sim.waves" {
  stage = "sim",
  after = { "sim.move" },
  reads = { "Waves", "Enemy", "Player", "Transform", "Sprite" },
  writes = { "Waves", "Enemy", "Health", "Transform", "Sprite" },
  rng = "waves",
  doc = "Counts the living enemies; when none are left, waits Waves.gap ticks and spawns the next wave (first, then more each wave) at random points on the arena's edge.",
  run = function(q)
    if q:entity("@player"):get("Player").dead then return end
    local arena = q:entity("@arena")
    local w = arena:get("Waves")
    local before = w.alive
    local alive = q:count("Enemy")
    w.alive = alive
    if alive > 0 then return end

    -- The arena is clear. If there were enemies last tick, the wave was
    -- just cleared: start the quiet before the next one.
    if before > 0 then
      w.timer = w.gap
      q:emit("wave.cleared", { wave = w.wave, score = q:entity("@player"):get("Player").score })
    end
    if w.timer > 0 then
      w.timer -= 1
      return
    end

    local n = w.wave + 1
    local count = w.first + w.more * (n - 1)
    local rng = q:rng()
    local centre = arena:get("Transform").pos
    local half = arena:get("Sprite").size * 0.5
    local hero = q:entity("@player"):get("Transform").pos

    -- A random point on the arena's edge: one of the four sides, then a
    -- place along it.
    local function edge_point()
      local side = rng:below(4)
      local along = rng:next() * 2 - 1 -- from -1 to 1
      local p
      if side == 0 then p = vector.create(-half.x, along * half.y, 0)
      elseif side == 1 then p = vector.create(half.x, along * half.y, 0)
      elseif side == 2 then p = vector.create(along * half.x, -half.y, 0)
      else p = vector.create(along * half.x, half.y, 0) end
      p = centre + p
      -- Too close to the hero: the opposite edge instead.
      if vector.magnitude(p - hero) < 5 then
        p = centre - (p - centre)
      end
      return p
    end

    local function spawn_one(e)
      e:get("Transform").pos = edge_point()
      e:get("Enemy").speed = w.slowest + rng:next() * (w.fastest - w.slowest)
      local hp = e:get("Health")
      hp.cur = w.enemy_health
      hp.max = w.enemy_health
      local sp = e:get("Sprite")
      sp.kind = "circle"
      sp.size = vector.create(0.9, 0.9, 0)
      sp.color = "#E8703F"
      sp.z = 1
    end

    -- One call for the whole wave. The callback gets one reused handle:
    -- keep e:id() if you need an enemy later, never e itself.
    q:spawn_many(count, { "Transform", "Enemy", "Health", "Sprite" }, spawn_one)
    w.wave = n
    w.alive = count
    q:emit("wave.spawned", { wave = n, count = count })
  end,
}
