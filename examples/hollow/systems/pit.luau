-- A hero who falls into the hall's pit starts again at the cave mouth. The
-- deep (phase 3 item 6) lies below the cave, so falling is the hall's
-- (x 44 to 72), or out of the bottom of the world.
return system "sim.pit" {
  stage = "sim",
  reads = { "Transform" },
  writes = { "Transform", "KinematicBody" },
  doc = "Below the hall's floor, or below the deepest room, the hero is put back at the start.",
  run = function(q)
    local hero = q:entity("@player")
    local t = hero:get("Transform")
    if (t.pos.x > 44 and t.pos.y < -3) or t.pos.y < -140 then
      t.pos = vector.create(3, 1.8, 0)
      hero:get("KinematicBody").velocity = vector.create(0, 0, 0)
      q:emit("player.fell", { entity = hero })
      q:play("sfx/fell")
    end
  end,
}
