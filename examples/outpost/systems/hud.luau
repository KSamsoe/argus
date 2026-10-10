-- The line at the top of the screen (the day, the time, the food, the
-- wood, the colonists, and the speed) and the notice under it (the colony's news: a
-- day begun, a colonist lost), in the string tables' words (phase 3 item 4).
--
-- The hour is the colony's day on a 24-hour clock as the light shows it:
-- the day turns at dawn, 05:00; its daylight (to Colony.dusk) runs to
-- 19:00, when night falls; noon and midnight are halfway through each, as
-- systems/light.luau has them. (It was the share of the day times 24, so
-- dawn read 00:00 and night fell at 15:36.)

-- The colony's hour at t, a share of its day, with night falling at dusk.
local function hour(t, dusk)
  local h = if t < dusk then 5 + 14 * t / dusk else 19 + 10 * (t - dusk) / (1 - dusk)
  return math.floor(h) % 24
end

return system "late.colony.hud" {
  stage = "late",
  unfrozen = true,
  reads = { "Colony", "Stockpile", "Colonist", "Worker", "Hud" },
  writes = { "UiText" },
  doc = "Writes the HUD line twice a second (day and hour, food stored, wood stored, colonists and how many are working, the speed) and the notice line when a day turns or a colonist dies, from the string tables (hud.*, notice.*).",
  run = function(q)
    local notice
    for _, ev in ipairs(q:events("day.turned")) do
      notice = q:text("notice.day", { day = ev.day, food = string.format("%.1f", ev.food) })
    end
    for _, ev in ipairs(q:events("colonist.died")) do
      notice = q:text("notice.starved", { name = ev.name })
    end
    local line
    if q.tick % 30 == 0 then
      local rules = q:entity("@colony"):get("Colony")
      local food, meal, crop, wood = 0, 0, 0, 0
      for _, s in q:each("Stockpile") do
        food += s.food
        meal += s.meal
        crop += s.crop
        wood += s.wood
      end
      local n, busy = 0, 0
      for _, _, w in q:each("Colonist", "Worker") do
        n += 1
        if w.job ~= nil then busy += 1 end
      end
      local t = (rules.clock / q.day_length) % 1
      line = q:text("hud.line", {
        day = rules.day,
        hour = string.format("%02d", hour(t, rules.dusk)),
        night = if t >= rules.dusk then q:text("hud.night") else "",
        food = string.format("%.1f", food),
        meals = math.floor(meal),
        crops = math.floor(crop),
        wood = math.floor(wood),
        colonists = n,
        working = busy,
        -- The clock's speed, however it was set: the keys, the editor, the agent.
        speed = if rules.paused then q:text("hud.paused") else q:text("hud.speed", { x = q:speed() }),
      })
    end
    if line == nil and notice == nil then return end
    for _, h, ui in q:each("Hud", "UiText") do
      if h.kind == "line" and line ~= nil then ui.text = line end
      if h.kind == "notice" and notice ~= nil then ui.text = notice end
    end
  end,
}
