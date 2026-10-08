-- The day's light (phase 5 item 3; section 12: "Outpost's day-night
-- cycle"). The colony's clock turns the day (Colony.clock, which stands
-- still while the game is paused, as the HUD's hour does): morning comes
-- when the day turns, noon is halfway to dusk (Colony.dusk), and midnight
-- halfway from dusk to the next morning. Every tick the cameras' Lighting
-- follows it: warm and low at dawn, white at noon, orange at dusk, a dark
-- blue all night, gliding from one to the next. The colony's lamps (the
-- campfire, the stove: Lamp) come up as the light goes and go out in the
-- morning, so the colony is still seen at night. High contrast (the
-- menu's settings) keeps the night light enough to see by.
--
-- One write a tick to the camera (two while a vignette's camera is made),
-- the lamps' only while the light is changing for them: the stress
-- scene's 200 colonists carry no light.

-- Where in the day (d: a share of the daylight, from morning to dusk; n:
-- a share of the night, from dusk to morning), the ambient's colour, and
-- its level. Between two keys the light eases from one to the other.
local KEYS = {
  { d = 0.0, rgb = { 255, 184, 144 }, level = 0.62 }, -- dawn, as the day turns
  { d = 0.18, rgb = { 255, 235, 210 }, level = 0.92 }, -- morning
  { d = 0.5, rgb = { 255, 255, 255 }, level = 1.0 }, -- noon
  { d = 0.82, rgb = { 255, 230, 196 }, level = 0.92 }, -- afternoon
  { d = 1.0, rgb = { 255, 156, 112 }, level = 0.6 }, -- dusk
  { n = 0.22, rgb = { 124, 132, 208 }, level = 0.42 }, -- evening
  { n = 0.5, rgb = { 112, 132, 232 }, level = 0.34 }, -- midnight
  { n = 0.85, rgb = { 144, 136, 204 }, level = 0.42 }, -- before dawn
}
-- The lamps come up as the level falls below the first and are at full
-- below the second.
local LAMPS_FROM, LAMPS_FULL = 0.85, 0.45
-- The least level high contrast allows.
local CONTRAST = 0.75

local function ease(x)
  return x * x * (3 - 2 * x)
end

-- The ambient (#RRGGBB, each channel to a step of 4, so a day's colours
-- are a few hundred strings, the same every day) and its level at t (0 to
-- 1 of the day) with night falling at dusk.
local function sky(t, dusk)
  local at = {}
  for i, k in ipairs(KEYS) do
    at[i] = if k.d ~= nil then k.d * dusk else dusk + k.n * (1 - dusk)
  end
  local n = #KEYS
  local i = n
  for j = 1, n do
    if t >= at[j] then i = j end
  end
  local a, b = KEYS[i], KEYS[i % n + 1]
  local from, to = at[i], if i == n then 1 + at[1] else at[i + 1]
  local s = if to > from then ease(math.clamp((t - from) / (to - from), 0, 1)) else 1
  local rgb = {}
  for c = 1, 3 do
    local v = a.rgb[c] + (b.rgb[c] - a.rgb[c]) * s
    rgb[c] = math.min(255, math.floor(v / 4 + 0.5) * 4)
  end
  return string.format("#%02X%02X%02X", rgb[1], rgb[2], rgb[3]), a.level + (b.level - a.level) * s
end

return system "late.colony.light" {
  stage = "late",
  -- After a vignette's camera is made, so it is lit from its first frame.
  after = { "late.colony.vignette" },
  -- On load, so the first frame is lit; unfrozen, so high contrast chosen
  -- in the menu (which stands the game still) answers at once.
  on_load = true,
  unfrozen = true,
  reads = { "Colony", "Camera", "Lamp", "UiSettings" },
  writes = { "Lighting", "Light" },
  doc = "Lights the valley by the colony's time of day (Colony.clock, Colony.dusk): every camera's Lighting (one is put on a camera without it) eases between dawn (warm, 0.62), noon (white, 1), dusk (orange, 0.6) and midnight (dark blue, 0.34), at least 0.75 with high contrast; each Lamp's Light comes up as the level falls below 0.85, to Lamp.intensity below 0.45, and is off by day.",
  run = function(q)
    local rules = q:entity("@colony"):get("Colony")
    local t = (rules.clock / q.day_length) % 1
    local ambient, level = sky(t, rules.dusk)
    local night = ease(math.clamp((LAMPS_FROM - level) / (LAMPS_FROM - LAMPS_FULL), 0, 1))
    for _, s in q:each("UiSettings") do
      if s.high_contrast then level = math.max(level, CONTRAST) end
    end
    local unlit = {}
    for e in q:each("Camera") do
      if e:has("Lighting") then
        local lit = e:get("Lighting")
        if lit.ambient ~= ambient then lit.ambient = ambient end
        -- Stored as an f32: equal enough (a paused clock), it is left alone.
        if math.abs(lit.level - level) > 1e-6 then lit.level = level end
      else
        table.insert(unlit, e:id())
      end
    end
    -- The valley has no walls to throw shadows: none are cast.
    for _, id in ipairs(unlit) do
      q:entity(id):insert("Lighting", { ambient = ambient, level = level, shadows = false, tiles = false })
    end
    for e, lamp in q:each("Lamp") do
      local l = e:get("Light")
      local want = lamp.intensity * night
      local on = want > 0.001
      if l.on ~= on then l.on = on end
      if math.abs(l.intensity - want) > 1e-4 then l.intensity = want end
    end
  end,
}
