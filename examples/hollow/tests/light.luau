-- 2D lighting (phase 5 item 3; section 12: "Hollow's caverns"). The hollow
-- is dark: a little cold light everywhere, the most at the cave mouth and
-- the least in the deep, gliding from room to room; the hero carries a
-- lantern that lights the rock around them and casts its shadows; what
-- should glow does (the hermit's fire, the gem, the way out, the deep's
-- crystals). The agent reads all of it in the semantic frame's lighting.

local LANTERN = "@persistent/lantern"

local function set(t, entity, path, value)
  local tx = t:call("edit.begin", { author = "test:hollow", note = "the light" }).tx
  t:call("edit.set", { tx = tx, entity = entity, path = path, value = value })
  t:call("edit.commit", { tx = tx })
end

-- The semantic frame's lighting, and where the hero is on the screen.
local function seen(t)
  local f = t:call("obs.frame", { format = "semantic" })
  if f.lighting == nil then error("the hollow is lit: the drawing camera has Lighting", 0) end
  local hero = nil
  for _, row in ipairs(f.entities) do
    if row[2] == "hero" then hero = row[4] end
  end
  if hero == nil then error("the hero is in the frame", 0) end
  return f.lighting, hero
end

local function listed(lit, id)
  for _, l in ipairs(lit.lights) do
    if l.id == id then return l end
  end
  return nil
end

local function ids(lit)
  local out = {}
  for _, l in ipairs(lit.lights) do table.insert(out, l.id) end
  return table.concat(out, ", ")
end

-- The lantern is listed, and it is at the hero, by their side.
local function lantern_by_hero(t, where)
  local lit, hero = seen(t)
  local l = listed(lit, LANTERN)
  if l == nil then error(where .. ": the lantern is listed: " .. ids(lit), 0) end
  if l.kind ~= "point" or l.color ~= "#FFB870" or l.flicker == nil or l.shadows ~= nil then
    error(where .. ": a warm flickering point light casting shadows: " .. l.kind .. " " .. l.color, 0)
  end
  local x, y = l.at[1], l.at[2]
  if x < hero[1] or x > hero[3] or y < hero[2] or y > hero[4] then
    error(string.format("%s: the lantern at (%d, %d) is by the hero (%d, %d to %d, %d)",
      where, x, y, hero[1], hero[2], hero[3], hero[4]), 0)
  end
  t:log(string.format("%s: level %.2f, the lantern at (%d, %d) by the hero (%d, %d to %d, %d), %d px of reach; lights: %s",
    where, lit.level, x, y, hero[1], hero[2], hero[3], hero[4], l.radius_px, ids(lit)))
  return lit
end

local function level(t)
  return t.expect:field("@camera", "Lighting.level"):get()
end

scenario "the hollow is dark and the hero's lantern lights it wherever they go" {
  scene = "hollow", seed = 1,
  run = function(t)
    t:step(1)
    -- The cave mouth: the most of the cold light.
    local lit = lantern_by_hero(t, "the cave mouth")
    if lit.ambient ~= "#8A94B8" or math.abs(lit.level - 0.6) > 0.005 or lit.shadows ~= true then
      error("the cave mouth's light: " .. lit.ambient .. " at " .. lit.level, 0)
    end

    -- The lantern goes where the hero goes: a second's run to the right.
    local x0 = t.expect:field("@player", "Transform.pos.x"):get()
    t.input:hold("move", { 1, 0 }, 60)
    t:step(60)
    local x = t.expect:field("@player", "Transform.pos.x"):get()
    local y = t.expect:field("@player", "Transform.pos.y"):get()
    if x < x0 + 3 then error("the hero ran: " .. x0 .. " to " .. x, 0) end
    t.expect:field("@lantern", "GlobalTransform.pos.x"):near(x + 0.3, 0.001)
    t.expect:field("@lantern", "GlobalTransform.pos.y"):near(y - 0.1, 0.001)
    lantern_by_hero(t, "run to the right")

    -- Into the deep, a dark room: the light glides down to its level, a
    -- little each tick, never at once.
    local before = level(t)
    set(t, "@player", "Transform.pos", { -19, 1.8 })
    local levels, biggest = {}, 0
    local last = before
    for _ = 1, 120 do
      t:step(1)
      local now = level(t)
      if now > last + 1e-6 then error("the light only falls going down: " .. last .. " to " .. now, 0) end
      biggest = math.max(biggest, last - now)
      last = now
      table.insert(levels, now)
    end
    t.expect:field("@player", "InRoom.room"):eq("deep-0-0")
    if biggest > 0.05 then error("the level glides, it does not snap: a tick fell " .. biggest, 0) end
    t.expect:field("@camera", "Lighting.level"):near(0.22, 0.002)
    t:log(string.format("into the deep: %.2f to %.2f, %.3f a tick at most; after 0.25 s %.3f, after 0.5 s %.3f",
      before, last, biggest, levels[15], levels[30]))
    lit = lantern_by_hero(t, "the deep's first room")
    -- Its glowing crystals light the rock round them in their colour.
    local glows = 0
    for _, l in ipairs(lit.lights) do
      if string.find(l.id, "glow", 1, true) then
        glows += 1
        if l.shadows ~= nil or l.color ~= "#9FE6FF" then error("a crystal's light: " .. l.id .. " " .. l.color, 0) end
      end
    end
    if glows ~= 2 then error("the deep's first room has two glowing crystals: " .. ids(lit), 0) end

    -- Deeper is darker.
    set(t, "@player", "Transform.pos", { -139, -68.2 })
    t:step(150)
    t.expect:field("@player", "InRoom.room"):eq("deep-5-5")
    t.expect:field("@camera", "Lighting.level"):lt(0.22)
    lantern_by_hero(t, "five rooms down")

    -- High contrast (the settings page) lifts the dark; off, it comes back.
    set(t, "@settings", "UiSettings.high_contrast", true)
    t:step(120)
    t.expect:field("@camera", "Lighting.level"):gte(0.798)
    set(t, "@settings", "UiSettings.high_contrast", false)
    t:step(150)
    t.expect:field("@camera", "Lighting.level"):lt(0.22)
    local se = t:call("script.errors", {})
    if se.total ~= 0 then error("script errors: " .. tostring(se.rows[1].message), 0) end
  end,
}

scenario "the hermit's fire, the gem, and the way out glow; the lantern goes out with the hero" {
  scene = "hollow", seed = 1,
  run = function(t)
    t:step(1)
    local lit = seen(t)
    local fire = listed(lit, "@a/fire")
    if fire == nil or fire.flicker == nil then error("the hermit's fire is listed, flickering: " .. ids(lit), 0) end

    -- The gem glints at the top of the shaft, and stops once it is taken.
    set(t, "@player", "Transform.pos", { 36, 19.8 })
    t:step(90)
    t.expect:field("@player", "InRoom.room"):eq("b")
    t.expect:field("@camera", "Lighting.level"):near(0.38, 0.002)
    lit = seen(t)
    local gem = listed(lit, "@b/gem")
    if gem == nil or gem.shadows ~= false then error("the gem's glint is listed: " .. ids(lit), 0) end
    set(t, "@gem", "Gem.taken", true)
    t:step(2)
    t.expect:field("@gem", "Sprite.visible"):eq(false)
    t.expect:field("@gem", "Light.on"):eq(false)
    if listed(seen(t), "@b/gem") ~= nil then error("a taken gem glints no more", 0) end

    -- The way out: daylight through the arch, over the floor before it.
    set(t, "@player", "Transform.pos", { 62, 1.8 })
    t:step(90)
    t.expect:field("@camera", "Lighting.level"):near(0.34, 0.002)
    lit = lantern_by_hero(t, "the end of the hall")
    local out = listed(lit, "@c/exit")
    if out == nil or out.color ~= "#FFF0C8" then error("the way out's light is listed: " .. ids(lit), 0) end

    -- Through it, the hero is out of sight, and so is the lantern; R, and
    -- it is lit again at the cave mouth.
    t.input:hold("move", { 1, 0 }, 120)
    t:run_until { event = "run.finished", max_frames = 240 }
    t:step(2)
    t.expect:field("@player", "Sprite.visible"):eq(false)
    t.expect:field("@lantern", "Light.on"):eq(false)
    t.expect:field("@lantern", "Sprite.visible"):eq(false)
    t.input:hold("restart", true, 2)
    t:step(3)
    t.expect:field("@player", "Sprite.visible"):eq(true)
    t.expect:field("@lantern", "Light.on"):eq(true)
    t.expect:field("@lantern", "Sprite.visible"):eq(true)
    t:step(120)
    t.expect:field("@player", "InRoom.room"):eq("a")
    t.expect:field("@camera", "Lighting.level"):near(0.6, 0.002)
    lantern_by_hero(t, "back at the cave mouth")
  end,
}

-- Image goldens (section 9): the lit hollow as a person sees it, drawn with
-- the software rasteriser. By the cave's step: the lantern's warm light
-- round the hero, the step's shadow thrown up and away across the cave,
-- the hermit's fire glowing past them, the rest in the cave's cold dark.
-- In the deep: two crystals glowing blue and lighting the rock round them,
-- the lantern's light on the floor and the wall by the hero.
return scenario "the lit hollow looks as it did" {
  scene = "hollow", seed = 1,
  run = function(t)
    set(t, "@player", "Transform.pos", { 14, 1.8 })
    t:step(90)
    t.expect:image("step")
    set(t, "@player", "Transform.pos", { -19, 1.8 })
    t:step(120)
    t.expect:image("deep")
  end,
}
