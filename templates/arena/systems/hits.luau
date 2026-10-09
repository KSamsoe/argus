-- Hits: bullets against enemies, and enemies against the hero.
--
-- Both use the engine's spatial index (q:nearest, q:within) over
-- Transform, so there is no collision setup: a hit is "close enough". The
-- reach numbers below are the sprites' half sizes added up; change them if
-- you change the sizes. The index is rebuilt once a tick, after these
-- systems, so it holds where things were at the end of the last tick: an
-- entity spawned this tick is found from the next one, and a despawned one
-- is never found. Every hit is an event (enemy.hit, enemy.killed,
-- player.hurt, player.died): the tests count them, and `argus events`
-- shows them.
local BULLET_REACH = 0.6 -- bullet half size 0.15 + enemy half size 0.45
local TOUCH_REACH = 0.9 -- hero half size 0.5 + enemy half size 0.45, a little less

return system "sim.hits" {
  stage = "sim",
  after = { "sim.bullets" },
  reads = { "Bullet", "Enemy", "Health", "Player", "Transform", "Waves" },
  writes = { "Health", "Player", "Sprite", "UiNode" },
  doc = "A bullet within reach of an enemy takes Bullet.damage from its Health and is spent; an enemy at zero dies and scores. An enemy touching the hero takes Enemy.damage from the hero's Health, then the hero is safe for Player.grace ticks; at zero the hero dies and @game_over shows.",
  run = function(q)
    local pe = q:entity("@player")
    local pl = pe:get("Player")
    if pl.dead then return end

    -- Bullets against enemies.
    local spent, killed = {}, {}
    for b, bullet, bt in q:each("Bullet", "Transform") do
      local target = q:nearest(bt.pos, BULLET_REACH, "Enemy", "Health")
      if target ~= nil then
        local hp = target:get("Health")
        if hp.cur > 0 then
          hp.cur = math.max(0, hp.cur - bullet.damage)
          table.insert(spent, b:id())
          if hp.cur == 0 then
            table.insert(killed, target:id())
            pl.score += 1
            q:emit("enemy.killed", { entity = target, by = "bullet" })
          else
            -- Hurt but standing: a paler colour shows the hit landed.
            target:get("Sprite").color = "#F5B08F"
            q:emit("enemy.hit", { entity = target, left = hp.cur })
          end
        end
      end
    end
    -- Despawn after the loop, never inside it.
    for _, id in ipairs(spent) do q:entity(id):despawn() end
    for _, id in ipairs(killed) do q:entity(id):despawn() end

    -- Enemies against the hero: one hit, then a short grace.
    local sp = pe:get("Sprite")
    if pl.safe > 0 then
      pl.safe -= 1
      sp.visible = pl.safe == 0 or (pl.safe // 4) % 2 == 0 -- blink while safe
      return
    end
    local hero = pe:get("Transform").pos
    local by = q:nearest(hero, TOUCH_REACH, "Enemy")
    if by == nil then return end
    local hp = pe:get("Health")
    local amount = by:get("Enemy").damage
    hp.cur = math.max(0, hp.cur - amount)
    pl.safe = pl.grace
    q:emit("player.hurt", { by = by, amount = amount, left = hp.cur })
    if hp.cur == 0 then
      pl.dead = true
      sp.visible = true
      q:entity("@game_over"):get("UiNode").visible = true
      q:emit("player.died", { score = pl.score, wave = q:entity("@arena"):get("Waves").wave })
    end
  end,
}
