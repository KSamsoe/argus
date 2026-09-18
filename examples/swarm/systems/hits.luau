return system "sim.hits" {
  stage = "sim",
  after = { "sim.bullets" },
  reads = { "Bullet", "Enemy", "Transform", "Player", "Health", "Director" },
  writes = { "Health", "Player", "Bullet" },
  doc = "Bullets hit the first enemy within reach and die; enemies touching the player hurt it. A cell hash of the bullets keeps it one pass over the enemies.",
  run = function(q)
    local pe = q:entity("@player")
    local pl = pe:get("Player")
    if pl.dead then return end
    local ppos = pe:get("Transform").pos
    local px, py = ppos.x, ppos.y
    local d = q:entity("@director"):get("Director")

    -- Bullets into cells (few hundred at most).
    local cell = 2.0
    local buckets = {}
    local bullets = {}
    for e, b, t in q:each("Bullet", "Transform") do
      local pos = t.pos
      local entry = { x = pos.x, y = pos.y, damage = b.damage, id = e:id(), spent = false }
      table.insert(bullets, entry)
      local key = math.floor(pos.x / cell) * 65536 + math.floor(pos.y / cell)
      local list = buckets[key]
      if list == nil then
        buckets[key] = { entry }
      else
        table.insert(list, entry)
      end
    end

    -- One pass over the enemies: the bullets in their neighbourhood, and the player.
    local hit_r2 = 0.8 * 0.8
    local touch_r2 = 0.7 * 0.7
    local php = pe:get("Health")
    local dead_enemies = {}
    local touched = 0
    local touched_by = nil
    for e, en, t, hp in q:each("Enemy", "Transform", "Health") do
      local pos = t.pos
      local x, y = pos.x, pos.y
      if next(buckets) ~= nil then
        local cx, cy = math.floor(x / cell), math.floor(y / cell)
        for ox = -1, 1 do
          for oy = -1, 1 do
            local list = buckets[(cx + ox) * 65536 + (cy + oy)]
            if list ~= nil then
              for _, b in ipairs(list) do
                if not b.spent then
                  local dx, dy = b.x - x, b.y - y
                  if dx * dx + dy * dy <= hit_r2 then
                    b.spent = true
                    local left = hp.cur - b.damage
                    if left < 0 then left = 0 end
                    hp.cur = left
                    if left == 0 then
                      table.insert(dead_enemies, e:id())
                      pl.kills += 1
                      if d.log_kills then
                        q:emit("death", { entity = e, cause = "bullet", wave = en.wave, tick = q.tick })
                      end
                    end
                    break
                  end
                end
              end
            end
            if hp.cur == 0 then break end
          end
          if hp.cur == 0 then break end
        end
      end
      if hp.cur > 0 then
        local dx, dy = px - x, py - y
        if dx * dx + dy * dy <= touch_r2 then
          touched += en.damage
          touched_by = touched_by or e:id()
        end
      end
    end

    for _, b in ipairs(bullets) do
      if b.spent then q:entity(b.id):despawn() end
    end
    for _, id in ipairs(dead_enemies) do
      q:entity(id):despawn()
    end

    -- The player: at most one hit every 10 ticks, so a crowd is a stream of readable events.
    if pl.iframes > 0 then
      pl.iframes -= 1
    elseif touched > 0 then
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
