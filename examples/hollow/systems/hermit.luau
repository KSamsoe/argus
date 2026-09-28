-- The hermit (phase 3 item 4). Near him his prompt shows, and the talk
-- action starts a conversation from defs/dialogue.toml, which
-- dialogue.update plays through the UI in the chosen language. While it is
-- open the game stands still, as the pause menu makes it, and the run's
-- clock does not count it.
return system "input.hermit" {
  stage = "input",
  after = { "input.actions", "input.pause" },
  unfrozen = true,
  reads = { "Actions", "Transform", "Talk", "UiNode", "Hermit" },
  writes = { "UiNode", "Talk", "Run" },
  doc = "Shows the hermit's prompt while the hero is within 1.8 units; the talk action there starts a Talk from dialogue:hermit.start. While a Talk is open the rest of each tick is frozen (the UI and the dialogue still run) and the run's clock waits.",
  run = function(q)
    local talking = q:count("Talk") > 0
    -- Any conversation stands the game still, his or another's (a mod's
    -- keeper, phase 3 item 8).
    if talking then
      q:entity("@hud"):get("Run").started += q.dt
      q:freeze()
    end
    -- His room streams out when the hero is two rooms from it (phase 3
    -- item 6): then there is no one here to talk to. Found by his
    -- component, not his id, which would fail and stop this system.
    local hermit = nil
    for e in q:each("Hermit") do
      hermit = q:entity(e:id())
    end
    if hermit == nil then return end
    local hero = q:entity("@player")
    local d = hermit:get("Transform").pos - hero:get("Transform").pos
    local near = d.x * d.x + d.y * d.y < 1.8 * 1.8
    hermit:get("UiNode").visible = near and not talking
    if talking then return end
    local paused = q:entity("@pause"):get("UiNode").visible
    if near and not paused and hero:get("Actions").talk_pressed then
      local t = q:spawn({ "Talk" }):get("Talk")
      t.start = "dialogue:hermit.start"
      t.speaker = hermit
      t.listener = hero
      t.slot = 1
      q:freeze()
    end
  end,
}
