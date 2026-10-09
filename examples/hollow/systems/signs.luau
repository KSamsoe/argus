-- A sign says what it says while the hero stands near it. Found by a mark
-- on the phase 2 playtest: "what is this thing?" The signs' words reached
-- the agent through the semantic frame and never the player. Since phase 3
-- item 1 the words are a UI panel hung over the sign. The feather says what
-- it gives the same way, while it is there to take: from a little farther,
-- since it sits up on the cave's plank, over the hero's head.
return system "late.signs" {
  stage = "late",
  reads = { "Transform", "Describe", "Sprite" },
  writes = { "UiNode" },
  doc = "Shows a sign's words over it (its UI panel) while the hero is within 2.5 units, and the feather's while it is drawn and the hero is within 3.",
  run = function(q)
    local hero = q:entity("@player"):get("Transform").pos
    for e, d, t, n in q:each("Describe", "Transform", "UiNode") do
      if d.name == "sign" or d.name == "exit" then
        local dx, dy = t.pos.x - hero.x, t.pos.y - hero.y
        n.visible = dx * dx + dy * dy < 2.5 * 2.5
      elseif d.name == "feather" then
        local dx, dy = t.pos.x - hero.x, t.pos.y - hero.y
        n.visible = e:get("Sprite").visible and dx * dx + dy * dy < 3 * 3
      end
    end
  end,
}
