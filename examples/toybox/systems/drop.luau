return system "sim.drop" {
  stage = "sim",
  reads = { "Dropper", "Ball", "Transform" },
  writes = { "Dropper", "Ball", "Transform", "Collider", "RigidBody", "Sprite" },
  rng = "drop",
  doc = "Drops a bouncy ball over the bridge every `every` ticks while fewer than `max` are in play, and takes back the balls that fell off the world.",
  run = function(q)
    local gone = {}
    local balls = 0
    for e, b, t in q:each("Ball", "Transform") do
      if t.pos.y < -20 then
        table.insert(gone, e:id())
      else
        balls += 1
      end
    end
    for _, id in ipairs(gone) do
      q:entity(id):despawn()
    end
    local d = q:entity("@world"):get("Dropper")
    if d.timer > 0 then
      d.timer -= 1
      return
    end
    d.timer = d.every
    if balls >= d.max then return end
    local rng = q:rng()
    local x = -3 + rng:next() * 6
    local e = q:spawn({ "Ball", "Transform", "Collider", "RigidBody", "Sprite" })
    e:get("Ball").born = q.tick
    e:get("Transform").pos = vector.create(x, 16, 0)
    e:get("Collider").radius = 0.4
    local r = e:get("RigidBody")
    r.restitution = 0.7
    r.density = 0.5
    local s = e:get("Sprite")
    s.kind = "circle"
    s.size = vector.create(0.8, 0.8, 0)
    s.color = "#F2C14E"
    s.z = 3
    d.dropped += 1
    q:emit("ball.dropped", { x = x, tick = q.tick })
  end,
}
