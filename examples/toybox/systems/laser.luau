-- The tripwire (2D staples items 0 and 1): a ray from the laser to the
-- right, meeting the bodies' colliders. A ball that comes into the beam
-- trips it; the beam and where it ends are drawn for debugging, which a
-- release build leaves out.
return system "sim.laser" {
  stage = "sim",
  reads = { "Collider", "Transform", "Ball" },
  writes = { "Laser" },
  doc = "Casts the tripwire's ray to the right, keeps what it meets and how far, and counts the times a ball comes into it; draws the beam for debugging.",
  run = function(q)
    local e = q:entity("@laser")
    local l = e:get("Laser")
    local from = e:get("Transform").pos
    local to = from + vector.create(l.length, 0, 0)
    local h = q:raycast(from, to, { tiles = false })
    local was = l.hit
    if h == nil then
      l.hit, l.distance = "", l.length
    else
      l.hit = h.entity:name() or (h.entity:has("Ball") and "ball") or "body"
      l.distance = h.distance
    end
    if l.hit == "ball" and was ~= "ball" then
      l.tripped += 1
      q:emit("laser.tripped", { distance = l.distance })
    end
    local stop = h and h.point or to
    q:draw_line(from, stop, { color = l.hit == "ball" and "#FF4040" or "#40FF80" })
    if h then
      q:draw_circle(stop, 0.15, { color = "#FFFFFF", fill = true })
    end
  end,
}
