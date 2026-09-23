-- A hero who falls into the hall's pit starts again at the cave mouth.
return system "sim.pit" {
  stage = "sim",
  reads = { "Transform" },
  writes = { "Transform", "KinematicBody" },
  doc = "Below the floor, the hero is put back at the start.",
  run = function(q)
    local hero = q:entity("@player")
    local t = hero:get("Transform")
    if t.pos.y < -3 then
      t.pos = vector.create(3, 1.8, 0)
      hero:get("KinematicBody").velocity = vector.create(0, 0, 0)
      q:emit("player.fell", { entity = hero })
      q:play("sfx/fell")
    end
  end,
}
