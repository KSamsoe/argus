-- Doors (the lanterns mod): one in the game's hall, one back from the
-- vigil. Near one its prompt shows, and the talk action sets the hero down
-- on its far side; the rooms stream to wherever the hero is, and the
-- music follows (the game's system, then this mod's).
return system "lanterns.doors" {
  stage = "input",
  after = { "input.hermit" },
  unfrozen = true,
  reads = { "Actions", "Transform", "Talk", "UiNode", "lanterns_Door" },
  writes = { "Transform", "KinematicBody", "UiNode" },
  doc = "Shows a lanterns_Door's prompt while the hero is within 1.2 units of it; the talk action there sets the hero down at its `to` (lanterns.through {door}).",
  run = function(q)
    local hero = q:entity("@player")
    local talking = q:count("Talk") > 0
    local paused = q:entity("@pause"):get("UiNode").visible
    local at = hero:get("Transform").pos
    for e, door in q:each("lanterns_Door") do
      local d = e:get("Transform").pos - at
      local near = d.x * d.x + d.y * d.y < 1.2 * 1.2
      e:get("UiNode").visible = near and not talking
      if near and not talking and not paused and hero:get("Actions").talk_pressed then
        hero:get("Transform").pos = door.to
        hero:get("KinematicBody").velocity = vector.create(0, 0, 0)
        q:emit("lanterns.through", { door = e:id() })
        return
      end
    end
  end,
}
