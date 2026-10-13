-- A colonist lost (phase 3 item 5): a grave is dug where they fell, and a
-- vignette, the "lost" timeline in defs/timeline.toml, takes the view there
-- for a few seconds and back. The colony goes on beneath it; one vignette
-- plays at a time.
return system "late.colony.vignette" {
  stage = "late",
  reads = { "Colony", "Timeline", "Vignette" },
  writes = { "Grave", "Transform", "Sprite", "Describe", "Camera", "Vignette", "Timeline" },
  doc = "On colonist.died: digs a grave where they fell (Grave, and a Describe for the agent) and, unless a vignette is playing, plays the lost timeline with the grave as its actor and the vignette camera (made the first time, following the grave) as its other.",
  run = function(q)
    local deaths = q:events("colonist.died")
    if #deaths == 0 then return end
    local rules = q:entity("@colony"):get("Colony")
    local playing = q:count("Timeline") > 0
    for _, ev in ipairs(deaths) do
      local g = q:spawn({ "Grave", "Transform", "Describe" })
      g:get("Transform").pos = ev.at
      local grave = g:get("Grave")
      grave.name = ev.who
      grave.day = rules.day
      g:insert("Sprite", { kind = "rect", size = vector.create(0.5, 0.8, 0), color = "#9A968C", z = 3 })
      local d = g:get("Describe")
      d.name = "grave"
      d.text = ev.who .. " lies here, starved on day " .. rules.day .. "."
      if not playing then
        playing = true
        local cam = nil
        for e in q:each("Vignette") do cam = e end
        if cam == nil then
          cam = q:spawn({ "Camera", "Transform", "Vignette" })
          local c = cam:get("Camera")
          c.active = false
          c.priority = 10
          c.bounded = false
          c.blend = 0.6
          c.height = 18
        end
        cam:get("Camera").target = g
        cam:get("Transform").pos = ev.at
        local tl = q:spawn({ "Timeline" }):get("Timeline")
        tl.name = "timeline:lost"
        tl.actor = g
        tl.other = cam
      end
    end
  end,
}
