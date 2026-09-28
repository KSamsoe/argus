-- The vigil's music (the lanterns mod): after the game's own music system
-- (late.music, which knows nothing of the vigil and would play the cave),
-- the vigil's track while the hero is in it; its bell comes in once the
-- lanterns are lit. The change waits for the bar, as the game's do.
return system "lanterns.music" {
  stage = "late",
  after = { "late.music" },
  reads = { "InRoom", "lanterns_Vigil" },
  writes = { "Music" },
  doc = "Sets Music.track to lanterns/vigil while the hero is in the vigil, and Music.intensity to 1 once the lanterns are lit.",
  run = function(q)
    local hero = q:entity("@player")
    if not hero:has("InRoom") or hero:get("InRoom").room ~= "lanterns-vigil" then return end
    local m = q:entity("@music"):get("Music")
    m.track = "lanterns/vigil"
    m.intensity = if q:entity("@lanterns.state"):get("lanterns_Vigil").kindled then 1 else 0
  end,
}
