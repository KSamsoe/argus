-- The kindling (the lanterns mod): once the keeper has lit the lanterns
-- (his conversation sets lanterns_Vigil.kindled), every lantern loaded
-- burns, and the first time the timeline lanterns.kindle plays (a chime,
-- the keeper's light swelling; defs/timeline.toml).
return system "lanterns.kindle" {
  stage = "sim",
  reads = { "lanterns_Vigil", "lanterns_Keeper" },
  writes = { "lanterns_Vigil", "lanterns_Lantern", "Timeline" },
  doc = "Lights every lantern once lanterns_Vigil.kindled, and plays timeline:lanterns.kindle the first time, the keeper its actor.",
  run = function(q)
    local v = q:entity("@lanterns.state"):get("lanterns_Vigil")
    if not v.kindled then return end
    for _, l in q:each("lanterns_Lantern") do
      l.lit = true
    end
    if not v.shown then
      v.shown = true
      local tl = q:spawn({ "Timeline" }):get("Timeline")
      tl.name = "timeline:lanterns.kindle"
      for e in q:each("lanterns_Keeper") do
        tl.actor = e
      end
    end
  end,
}
