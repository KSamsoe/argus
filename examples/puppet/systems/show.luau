return system "late.show" {
  stage = "late",
  after = { "skeletal.pose" },
  reads = { "Show", "Cue", "Animator", "Skeleton", "Transform" },
  writes = { "Show", "Cue", "Skeleton", "Transform" },
  doc = "Counts the puppets' Spine events this tick, cues the hero to wave every `every` ticks, and paces it left and right while its animator walks, turning it round at the ends.",
  run = function(q)
    local s = q:entity("@world"):get("Show")
    s.steps += #q:events("step")
    s.waves += #q:events("waved")
    local hero = q:entity("@hero")
    local cue = hero:get("Cue")
    cue.wave = false
    s.timer += 1
    if s.timer >= s.every then
      s.timer = 0
      s.cues += 1
      cue.wave = true
    end
    if hero:get("Animator").state ~= "walk" then return end
    local sk = hero:get("Skeleton")
    local t = hero:get("Transform")
    local dir = if sk.flip_x then -1 else 1
    t.pos = vector.create(t.pos.x + dir * s.pace * q.dt, t.pos.y, 0)
    if t.pos.x > 1.5 then
      sk.flip_x = true
    elseif t.pos.x < -3 then
      sk.flip_x = false
    end
  end,
}
