-- What sends the hero back to the start: falling below Hero.fall_y (the
-- pit between the two grounds), or touching anything with a Hazard (the
-- spikes). The hero goes back to Hero.start, stopped, and the camera cuts
-- to it there instead of gliding across the level.
--
-- Touches come from the engine: every Collider that starts overlapping
-- another gives an overlap.begin event {entity, other} in physics.overlap,
-- so this system runs after it. To add a hazard, give an entity a Collider
-- and a Hazard; to add a pit, leave a gap in the ground.
return system "sim.hazards" {
  stage = "sim",
  after = { "physics.overlap" },
  reads = { "Hero", "Hazard", "Transform" },
  writes = { "Hero", "Transform", "KinematicBody", "Camera" },
  doc = "Sends the hero back to Hero.start when it falls below Hero.fall_y (a pit) or touches an active Hazard (spikes): counts it in Hero.falls, cuts the camera to it, and emits hero.hurt {cause, by}.",
  run = function(q)
    local hero = q:entity("@player")
    local h = hero:get("Hero")
    local t = hero:get("Transform")

    local cause, by = nil, nil
    if t.pos.y < h.fall_y then
      cause = "pit"
    end
    for _, ev in ipairs(q:events("overlap.begin")) do
      -- The pair comes either way round: find the one that is not the hero.
      local other = nil
      if ev.entity == hero then
        other = ev.other
      elseif ev.other == hero then
        other = ev.entity
      end
      if other ~= nil and other:has("Hazard") and other:get("Hazard").active then
        cause, by = "hazard", other
      end
    end
    if cause == nil then
      return
    end

    t.pos = h.start
    hero:get("KinematicBody").velocity = vector.create(0, 0, 0)
    h.falls += 1
    -- false makes the camera snap to its target on the next tick.
    q:entity("@camera"):get("Camera").tracking = false
    q:emit("hero.hurt", { cause = cause, by = by })
  end,
}
