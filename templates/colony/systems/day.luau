-- The day clock. The colony keeps its own seconds (Colony.clock), which
-- stand still while the game is paused; the day, the hour on the HUD, and
-- the dark at night all follow them. A day is argus.toml's [time] day
-- (q.day_length), and a day starts at dawn, 06:00.
--
-- When a day turns it says so (day.turned, with what is in store): the
-- place to hook a daily event, a new colonist arriving, or an autosave
-- (q:save("auto")).

-- The night's colour over the meadow, and how dark it gets (0 to 255).
local NIGHT, DARKEST = "#0B1630", 150
-- How much of a day dusk and dawn take to fade.
local FADE = 0.05

return system "sim.colony.day" {
  stage = "sim",
  reads = { "Stockpile" },
  writes = { "Colony", "Sprite" },
  doc = "Moves the colony's clock (Colony.clock) and turns the day (Colony.day, day.turned {day, food, wood}); darkens @night's Sprite from Colony.dusk until the day turns.",
  run = function(q)
    local colony = q:entity("@colony")
    local rules = colony:get("Colony")
    rules.clock += q.dt
    local day = math.floor(rules.clock / q.day_length + 1e-9) + 1
    if day > rules.day then
      rules.day = day
      local store = colony:get("Stockpile")
      q:emit("day.turned", { day = day, food = store.food, wood = store.wood })
    end
    -- Night: dark from dusk, fading in, and fading out before dawn.
    local t = (rules.clock / q.day_length) % 1
    local dark = math.clamp((t - rules.dusk) / FADE, 0, 1) * math.clamp((1 - t) / FADE, 0, 1)
    q:entity("@night"):get("Sprite").color = string.format("%s%02X", NIGHT, math.floor(dark * DARKEST))
  end,
}
