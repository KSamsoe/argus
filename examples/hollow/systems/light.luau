-- The hollow's light (phase 5 item 3). The hollow is dark: the camera's
-- Lighting gives every room a little cold light, the cave mouth the most
-- and the deep the least (the deeper, the darker), and the hero's lantern
-- (persistent/lantern.toml) does the rest. When the hero crosses into
-- another room the level glides to that room's, under the room change's
-- fade; it never snaps. High contrast (the settings page) lifts the dark,
-- so whoever needs to see more of the hollow does. The lantern goes out
-- with the hero at the way out, and a gem's glint with the gem; once out,
-- daylight through the open way lights the hall the outro looks back on.

-- Each part's share of the ambient (Lighting.level).
local MOUTH, SHAFT, HALL = 0.6, 0.38, 0.34
local DEEP_TOP, DEEP_BOTTOM = 0.22, 0.1
-- The least light high contrast allows.
local CONTRAST = 0.8
-- The hall's light once the hero is out (the outro, until R).
local OUT = 0.6
-- Seconds to close most (63 percent) of the way to a room's level.
local GLIDE = 0.25

local function level_of(room)
  if room == "a" then return MOUTH end
  if room == "b" then return SHAFT end
  if string.sub(room, 1, 5) == "deep-" then
    local row = tonumber(string.match(room, "deep%-%d+%-(%d+)")) or 0
    return DEEP_TOP + (DEEP_BOTTOM - DEEP_TOP) * math.min(1, row / 9)
  end
  -- The hall, and any room past it (a mod's).
  return HALL
end

return system "late.light" {
  stage = "late",
  -- High contrast is chosen in the pause menu, which stands the game still:
  -- the light answers at once, behind the menu.
  unfrozen = true,
  reads = { "InRoom", "Sprite", "Gem", "UiSettings", "Run" },
  writes = { "Lighting", "Light", "Sprite" },
  doc = "Glides every camera's Lighting.level to the hero's room's (the cave mouth 0.6, the shaft 0.38, the hall and past it 0.34, the deep 0.22 at its top to 0.1 at its bottom; at least 0.8 with high contrast, 0.6 once the run is done), most of the way in 0.25 s; turns the lantern off and hides it while the hero is hidden (the way out), and a gem's light off while it is not drawn.",
  run = function(q)
    local hero = q:entity("@player")
    -- The lantern goes out with the hero, and comes back with them.
    local shown = hero:get("Sprite").visible
    local lantern = q:entity("@lantern")
    local lamp = lantern:get("Light")
    if lamp.on ~= shown then
      lamp.on = shown
      lantern:get("Sprite").visible = shown
    end
    -- A gem glints while it is there to take (sim.run draws it or not).
    for e in q:each("Gem") do
      if e:has("Light") then
        local seen = e:get("Sprite").visible
        local l = e:get("Light")
        if l.on ~= seen then l.on = seen end
      end
    end
    if not hero:has("InRoom") then return end
    local target = level_of(hero:get("InRoom").room)
    if q:entity("@settings"):get("UiSettings").high_contrast then
      target = math.max(target, CONTRAST)
    end
    if q:entity("@hud"):get("Run").done then
      target = math.max(target, OUT)
    end
    local k = 1 - math.exp(-q.dt / GLIDE)
    for _, lit in q:each("Lighting") do
      local d = target - lit.level
      if math.abs(d) < 0.002 then
        -- Stored as an f32: equal enough, it is left alone.
        if math.abs(d) > 1e-6 then lit.level = target end
      else
        lit.level += d * k
      end
    end
  end,
}
