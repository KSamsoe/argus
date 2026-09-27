-- The valley's ambience (phase 3 item 7): the day track by day and the
-- night track after dusk (Colony.dusk); the mixer makes the change at dusk
-- and at dawn on the bar. The birds are loudest at dawn and fade through
-- the afternoon (intensity runs from 1 at dawn to 0 at dusk). Twice a
-- second, like the HUD line.
return system "late.colony.ambience" {
  stage = "late",
  reads = { "Colony" },
  writes = { "Music" },
  doc = "Puts a Music on the colony (an older save's too) and sets its track by the time of day (day, or night after Colony.dusk) and its intensity by how much of the day is left before dusk (the birds).",
  run = function(q)
    if q.tick % 30 ~= 0 then return end
    local colony = q:entity("@colony")
    if not colony:has("Music") then colony:insert("Music") end
    local rules = colony:get("Colony")
    local m = colony:get("Music")
    local t = (rules.clock / q.day_length) % 1
    if t >= rules.dusk then
      m.track = "night"
      m.intensity = 0
    else
      m.track = "day"
      m.intensity = 1 - t / rules.dusk
    end
  end,
}
