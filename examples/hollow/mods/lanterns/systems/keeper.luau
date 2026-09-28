-- The lantern keeper (the lanterns mod), as the hermit is the game's:
-- near him his prompt shows, and the talk action starts his conversation
-- (defs/dialogue.toml). The game's own hermit system stands the game still
-- while any conversation is open, this one too.
return system "lanterns.keeper" {
  stage = "input",
  after = { "input.hermit" },
  unfrozen = true,
  reads = { "Actions", "Transform", "Talk", "UiNode", "lanterns_Keeper" },
  writes = { "UiNode", "Talk" },
  doc = "Shows the keeper's prompt while the hero is within 1.8 units of him; the talk action there starts a Talk from dialogue:lanterns.keeper.start.",
  run = function(q)
    -- He is in the vigil, which is loaded only near it.
    local keeper = nil
    for e in q:each("lanterns_Keeper") do
      -- q:each hands out one handle it reuses: keep the id's own.
      keeper = q:entity(e:id())
    end
    if keeper == nil then return end
    local hero = q:entity("@player")
    local talking = q:count("Talk") > 0
    local d = keeper:get("Transform").pos - hero:get("Transform").pos
    local near = d.x * d.x + d.y * d.y < 1.8 * 1.8
    keeper:get("UiNode").visible = near and not talking
    if talking then return end
    local paused = q:entity("@pause"):get("UiNode").visible
    if near and not paused and hero:get("Actions").talk_pressed then
      local t = q:spawn({ "Talk" }):get("Talk")
      t.start = "dialogue:lanterns.keeper.start"
      t.speaker = keeper
      t.listener = hero
      t.slot = 1
      q:freeze()
    end
  end,
}
