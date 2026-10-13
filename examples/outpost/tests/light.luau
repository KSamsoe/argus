-- The day's light (phase 5 item 3; section 12: "Outpost's day-night
-- cycle"). The valley's light follows the colony's clock, a tick at a
-- time: warm and low at dawn, white at noon, orange at dusk, dark blue at
-- night; the campfire and the stove are lit as the light goes and put out
-- in the morning. The agent reads it in the semantic frame's lighting.

local helpers = require("helpers")
local one = helpers.one
local function set(t, entity, path, value)
  helpers.set(t, entity, path, value, "the light")
end

local function rgb(hex)
  return tonumber(string.sub(hex, 2, 3), 16), tonumber(string.sub(hex, 4, 5), 16), tonumber(string.sub(hex, 6, 7), 16)
end

-- The colony's camera's light: its level and its ambient.
local function light(t, cam)
  local l = t:call("world.get", { entity = cam }).components.Lighting
  if l == nil then error("the colony's camera is lit", 0) end
  return l.level, l.ambient
end

-- The lights the semantic frame lists, by id.
local function lights(t)
  local f = t:call("obs.frame", { format = "semantic" })
  if f.lighting == nil then error("the semantic frame has the valley's lighting", 0) end
  local by = {}
  for _, l in ipairs(f.lighting.lights) do by[l.id] = l end
  return by, f.lighting
end

scenario "the valley's light follows the day, and the campfire and the stove light the night" {
  scene = "valley", seed = 1,
  run = function(t)
    local cam = one(t, { "id" }, { has = { "Camera" } })[1]
    local stove = one(t, { "id" }, { has = { "Station" } })[1]
    local fire = one(t, { "id" }, { ["Describe.name"] = "campfire" })[1]
    -- Lit before the first tick, at dawn: warm, and low.
    local level, ambient = light(t, cam)
    local r, g, b = rgb(ambient)
    if math.abs(level - 0.62) > 0.001 or r < b + 80 then error("dawn: " .. ambient .. " at " .. level, 0) end

    -- A day, a tick at a time: the light changes every tick, a little.
    local day = math.floor(days(1) * (t:status().hz or 60) + 0.5)
    local at = {}
    local changed, worst = 0, 0
    local last = level
    for tick = 1, day do
      t:step(1)
      local now, amb = light(t, cam)
      if now ~= last then changed += 1 end
      worst = math.max(worst, math.abs(now - last))
      last = now
      at[tick] = { now, amb }
    end
    t:log(string.format("over a day the level changed on %d of %d ticks, by %.4f at most", changed, day, worst))
    if changed < day * 0.95 then error("the light changes every tick: " .. changed .. " of " .. day, 0) end
    if worst > 0.005 then error("the light changes smoothly: a tick changed it by " .. worst, 0) end
    -- And from one day into the next without a jump.
    if math.abs(at[day][1] - at[day - 1][1]) > 0.005 then error("the day turns smoothly", 0) end

    -- The day's light as the HUD's hours go (a day is 3,600 ticks; night
    -- falls at Colony.dusk, 0.65 of it).
    local function when(share) return at[math.floor(share * day + 0.5)] end
    local noon, dusk, midnight = when(0.325), when(0.65), when(0.825)
    t:log(string.format("dawn %.2f %s, noon %.2f %s, dusk %.2f %s, midnight %.2f %s",
      at[1][1], at[1][2], noon[1], noon[2], dusk[1], dusk[2], midnight[1], midnight[2]))
    if noon[1] < 0.999 or noon[2] ~= "#FFFFFF" then error("noon is white and full: " .. noon[2] .. " at " .. noon[1], 0) end
    if midnight[1] >= noon[1] - 0.5 then error("midnight is darker than noon: " .. midnight[1], 0) end
    r, g, b = rgb(dusk[2])
    if r < b + 100 then error("dusk is orange: " .. dusk[2], 0) end
    r, g, b = rgb(midnight[2])
    if b < r + 80 then error("midnight is blue: " .. midnight[2], 0) end
    -- Brighter through the morning, darker through the evening.
    if not (at[300][1] < at[900][1] and at[900][1] < noon[1]) then error("the morning brightens", 0) end
    if not (dusk[1] > at[2600][1] and at[2600][1] > midnight[1]) then error("the evening darkens", 0) end

    -- By day the colony's lamps are out: nothing listed.
    t:run_until { sim_time = days(1.325) }
    local by = lights(t)
    if by[stove] ~= nil or by[fire] ~= nil then error("no lamps at noon", 0) end
    t.expect:field(stove, "Light.on"):eq(false)

    -- At night the campfire and the stove light the colony, and the agent
    -- reads them in the semantic frame.
    t:run_until { sim_time = days(1.825) }
    local lit
    by, lit = lights(t)
    if by[fire] == nil or by[stove] == nil then error("the campfire and the stove are lit at night", 0) end
    if by[fire].intensity < 1.59 or by[fire].flicker == nil or by[fire].shadows ~= false then
      error("the campfire burns at night: " .. by[fire].intensity, 0)
    end
    t:log(string.format("midnight: ambient %s at %.2f; the campfire at (%d, %d), %d px of reach, %s at %.1f; the stove at (%d, %d)",
      lit.ambient, lit.level, by[fire].at[1], by[fire].at[2], by[fire].radius_px, by[fire].color, by[fire].intensity, by[stove].at[1], by[stove].at[2]))

    -- High contrast (the menu's settings) keeps the night light enough to
    -- see by.
    local settings = one(t, { "id" }, { has = { "UiSettings" } })[1]
    set(t, settings, "UiSettings.high_contrast", true)
    t:step(1)
    t.expect:field(cam, "Lighting.level"):gte(0.75)
    set(t, settings, "UiSettings.high_contrast", false)
    t:step(1)
    t.expect:field(cam, "Lighting.level"):lt(0.5)

    -- A vignette's camera is lit as the colony's is, from its first frame.
    local ada = one(t, { "id" }, { ["Colonist.name"] = "Ada" })[1]
    set(t, ada, "Needs.fed", 0)
    t:run_until { event = "colonist.died", max_ticks = 10 }
    local vcam = one(t, { "id" }, { has = { "Vignette" } })[1]
    local vl = t:call("world.get", { entity = vcam }).components.Lighting
    if vl == nil then error("the vignette's camera is lit", 0) end
    t.expect:field(vcam, "Lighting.level"):near(t.expect:field(cam, "Lighting.level"):get(), 1e-6)
    t:step(1)
    t.expect:field(vcam, "Camera.drew"):eq(true)
    t.expect:field(vcam, "Lighting.ambient"):eq(t.expect:field(cam, "Lighting.ambient"):get())
  end,
}

-- A valley laid out before the colony had lamps (a save from then): the
-- next tick gives the stove its fire's light and lays the campfire, and
-- the night is lit as any other.
return scenario "a valley laid out without lamps gets them" {
  scene = "valley", seed = 1,
  run = function(t)
    t:step(2)
    local stove = one(t, { "id" }, { has = { "Station" } })[1]
    local fire = one(t, { "id" }, { ["Describe.name"] = "campfire" })[1]
    local tx = t:call("edit.begin", { author = "test:outpost", note = "a valley from before the lamps" }).tx
    t:call("edit.despawn", { tx = tx, entity = fire })
    t:call("edit.remove", { tx = tx, entity = stove, component = "Lamp" })
    t:call("edit.remove", { tx = tx, entity = stove, component = "Light" })
    t:call("edit.commit", { tx = tx })
    t.expect:count({ has = { "Lamp" } }):eq(0)
    t:step(1)
    t.expect:count({ has = { "Lamp", "Light" } }):eq(2)
    t.expect:count({ has = { "Station", "Lamp" } }):eq(1)
    t.expect:count({ ["Describe.name"] = "campfire" }):eq(1)
    t:run_until { sim_time = days(0.825) }
    local by = lights(t)
    local n = 0
    for _ in pairs(by) do n += 1 end
    if n ~= 2 then error("the stove and the campfire light the night: " .. n .. " lights", 0) end
  end,
}
