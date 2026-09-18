return system "sim.hits" {
  stage = "sim",
  after = { "sim.bullets" },
  reads = { "Bullet", "Enemy", "Transform", "Player", "Health", "Director" },
  writes = { "Health", "Player", "Bullet" },
  doc = "Each bullet hits the nearest enemy within reach (the engine's broadphase, not a hash of our own) and dies; enemies touching the player hurt it.",
  run = function(q)
    local pe = q:entity("@player")
    local pl = pe:get("Player")
    if pl.dead then return end
    local ppos = pe:get("Transform").pos
    local d = q:entity("@director"):get("Director")

    -- Bullets against enemies: a broadphase lookup per bullet.
    local spent = {}
    local dead_enemies = {}
    for b, bullet, bt in q:each("Bullet", "Transform") do
      local target, dist = q:nearest(bt.pos, 0.8, "Enemy", "Health")
      if target ~= nil then
        local hp = target:get("Health")
        if hp.cur > 0 then
          local left = hp.cur - bullet.damage
          if left < 0 then left = 0 end
          hp.cur = left
          table.insert(spent, b:id())
          if left == 0 then
            table.insert(dead_enemies, target:id())
            pl.kills += 1
            if d.log_kills then
              q:emit("death", { entity = target, cause = "bullet", wave = target:get("Enemy").wave, tick = q.tick })
            end
          end
        end
      end
    end
    for _, id in ipairs(spent) do q:entity(id):despawn() end
    for _, id in ipairs(dead_enemies) do q:entity(id):despawn() end

    -- Enemies against the player: everything within touching distance.
    local touched, touched_by = 0, nil
    for e, en, dist in q:within(ppos, 0.7, "Enemy") do
      touched += en.damage
      touched_by = touched_by or e:id()
    end
    if pl.iframes > 0 then
      pl.iframes -= 1
    elseif touched > 0 then
      local php = pe:get("Health")
      local amount = math.min(touched, 3)
      local left = php.cur - amount
      if left < 0 then left = 0 end
      php.cur = left
      pl.iframes = 10
      q:emit("damage", { target = "@player", by = touched_by, amount = amount, crowd = touched, wave = d.wave, tick = q.tick })
      if left == 0 then
        pl.dead = true
        q:emit("player.died", { wave = d.wave, tick = q.tick, kills = pl.kills, enemies_alive = d.alive, last_hit_by = touched_by })
      end
    end
  end,
}
