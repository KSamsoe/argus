-- The colony's clock: the hour of its day on a 24-hour clock, as the
-- light shows it. The HUD and the inspector both show it.
local M = {}

-- The colony's hour at t, a share of its day, with night falling at dusk.
-- The day turns at dawn, 05:00; its daylight (to Colony.dusk) runs to
-- 19:00, when night falls; noon and midnight are halfway through each, as
-- systems/light.luau has them. (It was the share of the day times 24, so
-- dawn read 00:00 and night fell at 15:36.)
function M.hour(t, dusk)
  local h = if t < dusk then 5 + 14 * t / dusk else 19 + 10 * (t - dusk) / (1 - dusk)
  return math.floor(h) % 24
end

return M
