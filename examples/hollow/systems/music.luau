-- The hollow's music (phase 3 item 7): each part has its own track, chosen
-- by the room the hero is in (InRoom, written as the rooms stream), and the
-- mixer changes track on the next bar of the one playing. Climbing the
-- shaft brings in the cave's high line; the deeper the hero goes into the
-- deep, the more of its line plays.
return system "late.music" {
  stage = "late",
  reads = { "InRoom", "Transform" },
  writes = { "Music" },
  doc = "Sets Music.track for the hero's room (the cave and the shaft: cave; the hall: hall; the deep: deep) and Music.intensity (how high up the shaft; how many rooms down the deep).",
  run = function(q)
    local hero = q:entity("@player")
    if not hero:has("InRoom") then return end
    local room = hero:get("InRoom").room
    local m = q:entity("@music"):get("Music")
    if room == "c" then
      m.track = "hall"
      m.intensity = 0
    elseif string.sub(room, 1, 5) == "deep-" then
      local row = tonumber(string.match(room, "deep%-%d+%-(%d+)")) or 0
      m.track = "deep"
      m.intensity = math.min(1, row / 9)
    else
      m.track = "cave"
      -- The shaft rises 24 units: the higher, the more of the high line.
      m.intensity = if room == "b" then math.clamp(hero:get("Transform").pos.y / 20, 0, 1) else 0
    end
  end,
}
