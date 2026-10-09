-- The line at the top of the screen: the day and the hour, what is in
-- store, how many colonists there are and how many are working, and the
-- speed. It runs in the late stage (after the sim has settled) and while
-- paused (unfrozen), so the line says "paused".

return system "late.colony.hud" {
  stage = "late",
  unfrozen = true,
  reads = { "Colony", "Stockpile", "Colonist", "Worker" },
  writes = { "UiText" },
  doc = "Writes @hud's line: day and hour (night after Colony.dusk), food and wood in store, colonists and how many have a job, and the speed or paused.",
  run = function(q)
    local colony = q:entity("@colony")
    local rules = colony:get("Colony")
    local store = colony:get("Stockpile")
    local people, working = 0, 0
    for _, _, w in q:each("Colonist", "Worker") do
      people += 1
      if w.job ~= nil then working += 1 end
    end
    local t = (rules.clock / q.day_length) % 1
    local hour = math.floor(6 + t * 24) % 24
    q:entity("@hud"):get("UiText").text = string.format(
      "Day %d, %02d:00%s   food %.1f   wood %d   %d colonists, %d working   %s",
      rules.day,
      hour,
      if t >= rules.dusk then " (night)" else "",
      store.food,
      math.floor(store.wood),
      people,
      working,
      if rules.paused then "paused" else q:speed() .. "x"
    )
  end,
}
