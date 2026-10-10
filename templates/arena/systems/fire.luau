-- Firing: every fire_every ticks the hero shoots one bullet at the nearest
-- enemy within range. No button, no aiming: walking is the whole control.
--
-- To fire on a button instead, add a button action to input.toml (fire)
-- and shoot only while `pe:get("Actions").fire` is true. To aim with the
-- right stick, add an axis2 action and shoot along it instead of at the
-- nearest enemy. A bullet is prefabs/bullet.toml: change its look or its
-- numbers there.
return system "sim.fire" {
  stage = "sim",
  after = { "sim.chase" },
  reads = { "Player", "Enemy", "Transform" },
  writes = { "Player" },
  spawns = { "bullet" },
  doc = "Every Player.fire_every ticks, fires one bullet at the nearest enemy within Player.range.",
  run = function(q)
    local pe = q:entity("@player")
    local pl = pe:get("Player")
    if pl.dead then return end
    if pl.cooldown > 0 then
      pl.cooldown -= 1
      return
    end
    local from = pe:get("Transform").pos
    -- The engine's spatial index: the closest Enemy within range, or nil.
    local target = q:nearest(from, pl.range, "Enemy")
    if target == nil then return end
    local d = target:get("Transform").pos - from
    local len = vector.magnitude(d)
    if len < 0.001 then return end

    pl.cooldown = pl.fire_every
    q:spawn_prefab("bullet", {
      ["Transform.pos"] = from,
      ["Bullet.vel"] = d * (pl.bullet_speed / len),
    })
  end,
}
