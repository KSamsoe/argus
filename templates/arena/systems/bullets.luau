-- Bullets fly straight, and are gone when they reach the arena's wall or
-- their ttl runs out (a shorter ttl is a shorter-range gun). sim.hits
-- spends the ones that hit.
return system "sim.bullets" {
  stage = "sim",
  after = { "sim.fire" },
  reads = { "Bullet", "Transform", "Sprite" },
  writes = { "Bullet", "Transform" },
  doc = "Moves every bullet by its velocity; despawns it at the arena's wall or when its ttl runs out.",
  run = function(q)
    local dt = q.dt
    local arena = q:entity("@arena")
    local lo = arena:get("Transform").pos - arena:get("Sprite").size * 0.5
    local hi = arena:get("Transform").pos + arena:get("Sprite").size * 0.5
    -- Despawning inside the loop over the same components is not allowed:
    -- collect the ids, despawn after.
    local gone = {}
    for e, b, t in q:each("Bullet", "Transform") do
      local p = t.pos + b.vel * dt
      if b.ttl == 0 or p.x < lo.x or p.x > hi.x or p.y < lo.y or p.y > hi.y then
        table.insert(gone, e:id())
      else
        b.ttl -= 1
        t.pos = p
      end
    end
    for _, id in ipairs(gone) do
      q:entity(id):despawn()
    end
  end,
}
