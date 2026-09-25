-- A sign says what it says while the hero stands near it. Found by a mark
-- on the phase 2 playtest: "what is this thing?" The signs' words reached
-- the agent through the semantic frame and never the player.
return system "late.signs" {
  stage = "late",
  reads = { "Transform", "Describe" },
  writes = { "Describe" },
  doc = "Shows a sign's words over it while the hero is within 2.5 units.",
  run = function(q)
    local hero = q:entity("@player"):get("Transform").pos
    for _, d, t in q:each("Describe", "Transform") do
      if d.name == "sign" or d.name == "exit" then
        local dx, dy = t.pos.x - hero.x, t.pos.y - hero.y
        d.shown = dx * dx + dy * dy < 2.5 * 2.5
      end
    end
  end,
}
